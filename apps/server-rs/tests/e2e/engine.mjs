// Verifies that the WebAssembly timer engine running in the browser produces the
// same numbers the Rust server produces — the point of sharing one engine.
//
// It also measures how many packets the server actually sends, which is the cost model.
import { readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve } from 'node:path';

const base = process.argv[2] ?? 'http://127.0.0.1:8787';
// its own room: a stray aux timer or a half-finished run in the shared default
// room would otherwise break the "nothing is sent while stopped" assertions
const room = 'eng-' + (Math.floor(Date.now() / 1000) % 100000);
const prefixed = `${base}/r/${room}`;
// resolved from this file so the suite runs from any checkout;
// build it first with packages/ontime-wasm/build.sh
const pkg = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../packages/ontime-wasm/pkg');

let failed = false;
const check = (label, ok, detail = '') => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok || !detail ? '' : ` -> ${detail}`}`);
};

// --- load the engine the browser would load -----------------------
const wasm = await import(pathToFileURL(`${pkg}/ontime_timer.js`).href);
await wasm.default({ module_or_path: await readFile(`${pkg}/ontime_timer_bg.wasm`) });
const engine = new wasm.OntimeTimer();

console.log('== engine loads ==');
check('module initialises', typeof wasm.OntimeTimer === 'function');
check('reports its identity', JSON.parse(wasm.engine_info()).name === 'ontime-wasm');

// --- set up a rundown ---------------------------------------------
const api = (method, path, body) =>
  fetch(`${prefixed}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then((r) => (r.status === 204 ? null : r.json()));

const list = await api('POST', '/data/rundowns', { title: '__engine__' });
const rid = list.rundowns.filter((r) => r.title === '__engine__').pop().id;
await api('POST', `/data/rundowns/${rid}/load`);
await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'ENGINE_A', duration: 600000, timeWarning: 120000, timeDanger: 60000,
});

// --- drive playback -----------------------------------------------
const ws = new WebSocket(base.replace(/^http/, 'ws') + `/ws?room=${room}`);
const packets = [];
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

ws.addEventListener('message', (e) => {
  const p = JSON.parse(e.data);
  if (p.tag === 'runtime-data') packets.push({ at: Date.now(), payload: p.payload });
});

await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('socket error')));
  setTimeout(() => reject(new Error('connect timeout')), 10000);
});

ws.send(JSON.stringify({ tag: 'start', payload: undefined }));
await wait(600);

console.log('== the server sends the engine inputs ==');
const started = packets.find((p) => p.payload.timer.playback === 'play');
check('a play packet arrived', Boolean(started));
check('_engine block present', Boolean(started?.payload._engine));
check('carries serverNow', typeof started?.payload._engine?.serverNow === 'number');
check('carries inputs', Boolean(started?.payload._engine?.inputs));

// --- the local engine must agree with the server ------------------
engine.update(JSON.stringify(started.payload._engine), started.at);

console.log('== local engine agrees with the server at the same instant ==');
{
  const local = JSON.parse(engine.tick(started.at));
  const server = started.payload.timer;
  for (const key of ['duration', 'addedTime', 'playback', 'phase']) {
    check(`${key} matches`, local[key] === server[key], `local ${local[key]} vs server ${server[key]}`);
  }
  const drift = Math.abs(local.current - server.current);
  check(`current within 5ms (drift ${drift}ms)`, drift <= 5);
}

// --- and after time passes, against a later server packet ---------
console.log('== still agrees after several seconds ==');
const before = packets.length;
await wait(6500);
const later = packets[packets.length - 1];
check('server sent a later packet', packets.length > before);
{
  // Same inputs, same instant: this is the invariant that matters — one engine,
  // two hosts, identical arithmetic. Seeding from this packet cancels the network
  // latency between the server computing its value and us receiving it.
  const mirror = new wasm.OntimeTimer();
  mirror.update(JSON.stringify(later.payload._engine), later.at);
  const local = JSON.parse(mirror.tick(later.at));
  const drift = Math.abs(local.current - later.payload.timer.current);
  check(`identical to the server (drift ${drift}ms)`, drift <= 2,
    `local ${local.current} vs server ${later.payload.timer.current}`);
  check('phase matches', local.phase === later.payload.timer.phase);
}
{
  // The long-lived engine has only seen the first packet. Any gap against the
  // server is accumulated network jitter in the one-way delay, not drift in the
  // maths — over the public internet a few hundred ms is normal, on localhost 0.
  const local = JSON.parse(engine.tick(later.at));
  const gap = Math.abs(local.current - later.payload.timer.current);
  check(`6s without a packet, still within 500ms (gap ${gap}ms)`, gap <= 500);
}

// --- the local engine advances between packets --------------------
console.log('== local engine advances between server packets ==');
{
  const t = later.at;
  const a = JSON.parse(engine.tick(t));
  const b = JSON.parse(engine.tick(t + 1000));
  check(`counts down 1000ms in 1s (got ${a.current - b.current})`, a.current - b.current === 1000);
  check('clock advances too', engine.clock(t + 1000) - engine.clock(t) === 1000);
}

// --- cost model: how chatty is the server now? --------------------
console.log('== server packet rate matches the configured heartbeat ==');
const heartbeatMs = (await api('GET', '/data/settings')).heartbeatMs;
const windowMs = Math.max(10000, heartbeatMs * 4);
const expected = windowMs / heartbeatMs;

const windowStart = packets.length;
await wait(windowMs);
const seen = packets.length - windowStart;
const perHour = Math.round((seen / windowMs) * 3600000);
check(
  `~${expected} packets in ${windowMs}ms at a ${heartbeatMs}ms heartbeat (got ${seen})`,
  seen >= expected * 0.6 && seen <= expected * 1.4,
);
console.log(`       => ~${perHour} server wake-ups per hour of playback`);

// --- and none at all when stopped ---------------------------------
console.log('== nothing is sent while stopped ==');
ws.send(JSON.stringify({ tag: 'stop', payload: undefined }));
await wait(500);
const idle = packets.length;
await wait(8000);
check(`no packets while stopped (got ${packets.length - idle})`, packets.length === idle);

// --- cleanup -------------------------------------------------------
ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
