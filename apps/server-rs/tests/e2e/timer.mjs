// Drives playback over the websocket exactly as apps/client does and checks the
// runtime-data packets that come back.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';
// its own room: a stray aux timer or a half-finished run in the shared default
// room would otherwise break the "nothing is sent while stopped" assertions
// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const room = 'tmr-' + Math.random().toString(36).slice(2, 10);
const prefixed = `${base}/r/${room}`;
const wsUrl = base.replace(/^http/, 'ws') + `/ws?room=${room}`;

let failed = false;
const check = (label, actual, expected) => {
  const ok = actual === expected;
  if (!ok) {
    failed = true;
    // dump the packet history so a one-off failure is diagnosable from the log
    console.log(`  FAIL ${label} -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`);
    console.log(`       packets so far: ${runtime.length}`);
    console.log(`       last packet: ${JSON.stringify(runtime[runtime.length - 1]?.timer)}`);
    return;
  }
  console.log(`  ok   ${label}`);
};
const near = (label, actual, expected, tolerance) => {
  const ok = typeof actual === 'number' && Math.abs(actual - expected) <= tolerance;
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ~${expected} (±${tolerance})`}`,
  );
};

const api = (method, path, body) =>
  fetch(`${prefixed}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then((r) => (r.status === 204 ? null : r.json()));

// --- set up a known rundown -------------------------------------
const list = await api('POST', '/data/rundowns', { title: '__timer__' });
const rid = list.rundowns.filter((r) => r.title === '__timer__').pop().id;
await api('POST', `/data/rundowns/${rid}/load`);

const a = await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'TIMER_A',
  duration: 6000,
  timeWarning: 4000,
  timeDanger: 2000,
});
const b = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'TIMER_B', duration: 60000 });

// --- drive it ----------------------------------------------------
const ws = new WebSocket(wsUrl);
const runtime = [];
const send = (tag, payload) => ws.send(JSON.stringify({ tag, payload }));
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const latest = () => runtime[runtime.length - 1];

/**
 * Sends a command and resolves with the packet it caused, ignoring any heartbeat
 * that happens to arrive first. Without this the assertions race the heartbeat.
 */

/// Waits until a packet satisfying `predicate` arrives.
///
/// Waiting for "the next packet" is not enough: a heartbeat can land between
/// sending a command and it taking effect, and that packet predates the change.
async function until(predicate, what, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const packet = latest();
    if (packet && predicate(packet)) return packet;
    await wait(20);
  }
  throw new Error(`timed out waiting for ${what}`);
}

async function sendAndAwait(tag, payload, timeoutMs = 3000) {
  assertSocketAlive(tag);
  const from = runtime.length;
  send(tag, payload);
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (runtime.length > from) return runtime[from];
    await wait(20);
  }
  throw new Error(`no packet after '${tag}'`);
}

// A deploy evicts the Durable Object and closes open sockets. Without this the
// suite would carry on sending into a dead socket and fail on a stale packet,
// which reads as a mysterious flake rather than "the connection went away".
let socketClosed = null;
ws.addEventListener('close', (event) => {
  socketClosed = `code ${event.code}${event.reason ? ` (${event.reason})` : ''}`;
});

ws.addEventListener('message', (e) => {
  const packet = JSON.parse(e.data);
  if (packet.tag === 'runtime-data') runtime.push(packet.payload);
});

function assertSocketAlive(stage) {
  if (socketClosed !== null) {
    console.log(`\n  the websocket closed during "${stage}": ${socketClosed}`);
    console.log('  a deploy rolling over mid-run will do this; rerun against a settled deployment');
    process.exit(1);
  }
}

await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('socket error')));
  setTimeout(() => reject(new Error('connect timeout')), 10000);
});

await wait(400);
console.log('== on connect ==');
check('runtime-data sent immediately', runtime.length > 0, true);
check('starts stopped', latest()?.timer?.playback, 'stop');

console.log('== start ==');
assertSocketAlive('start');
send('start', undefined);
await wait(500);
check('playback is play', latest().timer.playback, 'play');
check('event A loaded', latest().eventNow?.id, a.id);
check('event B is next', latest().eventNext?.id, b.id);
check('duration reported', latest().timer.duration, 6000);
check('numEvents', latest().rundown.numEvents, 2);

console.log('== the server pushes on the heartbeat ==');
// the browser advances the timer itself from _engine (see engine-test.mjs);
// these packets are only the safety net for clients that cannot
const heartbeatMs = (await api('GET', '/data/settings')).heartbeatMs;
const before = runtime.length;
await wait(2600);
const pushes = runtime.length - before;
const ceiling = Math.ceil(2600 / heartbeatMs) + 1;
check(`at most ${ceiling} pushes in 2.6s (got ${pushes})`, pushes <= ceiling, true);
check('the packet carries engine inputs', typeof latest()._engine?.serverNow, 'number');
check('inputs carry the start time', typeof latest()._engine?.inputs?.startedAt, 'number');
// how the countdown advances between packets is engine-test.mjs' job

console.log('== pause freezes it ==');
send('pause', undefined);
const paused = await until((p) => p.timer.playback === 'pause', 'the pause to apply');
const atPause = paused.timer.current;
check('playback is pause', paused.timer.playback, 'pause');
await wait(1500);
check('current did not move while paused', latest().timer.current, atPause);

console.log('== resume ==');
send('start', undefined);
const resumed = await until((p) => p.timer.playback === 'play', 'the resume to apply');
check('playback is play', resumed.timer.playback, 'play');
// resuming re-anchors to a whole second so the countdown stays in step with the
// wall clock, which can move the value by up to half a second
near('resumes close to where it paused', resumed.timer.current, atPause, 500);
check('anchored to a whole second', resumed._engine.inputs.startedAt % 1000, 0);

console.log('== addtime ==');
assertSocketAlive('addtime');
send('addtime', 30000);
await wait(400);
check('addedTime recorded', latest().timer.addedTime, 30000);
check('duration extended', latest().timer.duration, 36000);

console.log('== load next ==');
assertSocketAlive('load next');
send('load', 'next');
await wait(400);
check('event B loaded', latest().eventNow?.id, b.id);
check('armed, not running', latest().timer.playback, 'armed');
check('addedTime reset', latest().timer.addedTime, 0);

console.log('== stop ==');
// await the packet the command caused: sleeping races the round trip, and over
// the internet the previous packet is what gets read
send('stop', undefined);
const stopped = await until((p) => p.timer.playback === 'stop', 'the stop to apply');
check('playback is stop', stopped.timer.playback, 'stop');
check('nothing loaded', stopped.eventNow, null);

console.log('== the tick stops when playback stops ==');
await wait(500);
const idle = runtime.length;
await wait(3000);
check('no ticks while stopped', runtime.length, idle);

// --- cleanup -----------------------------------------------------
ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
