// Checks the ahead/behind-schedule figure against a real running rundown.
//
// This is the number an operator acts on, and it used to be a single-event
// approximation. The unit tests pin the arithmetic; this pins the wiring —
// that the right values reach it from a live rundown.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const near = (label, actual, expected, tolerance) => {
  const ok = typeof actual === 'number' && Math.abs(actual - expected) <= tolerance;
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ~${expected} (±${tolerance})`}`);
};
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const room = 'offset-' + (Math.floor(Date.now() / 1000) % 100000);
const api = (method, path, body) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));

// The rundown is planned to start at whatever the server's clock reads now, so
// starting immediately is "on time" regardless of when this suite runs.
await api('POST', '/data/settings', { timezoneOffset: 0 });
const serverClock = await (async () => {
  const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
  const clock = await new Promise((resolve, reject) => {
    ws.addEventListener('message', (e) => {
      const p = JSON.parse(e.data);
      if (p.tag === 'runtime-data') resolve(p.payload.clock);
    });
    ws.addEventListener('error', () => reject(new Error('socket error')));
    setTimeout(() => reject(new Error('no clock')), 10000);
  });
  ws.close();
  return clock;
})();

const list = await api('POST', '/data/rundowns', { title: '__offset__' });
const rid = list.rundowns.filter((r) => r.title === '__offset__').pop().id;
await api('POST', `/data/rundowns/${rid}/load`);

// planned to start ~1s from now so "start immediately" is on schedule
const plannedStart = serverClock + 1000;
const a = await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'OFF_A', duration: 60000, timeStart: plannedStart,
});
await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'OFF_B', duration: 60000, timeStart: plannedStart + 60000, linkStart: true,
});

const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
const runtime = [];
const send = (tag, payload) => ws.send(JSON.stringify({ tag, payload }));
const latest = () => runtime[runtime.length - 1];
ws.addEventListener('message', (e) => {
  const p = JSON.parse(e.data);
  if (p.tag === 'runtime-data') runtime.push(p.payload);
});
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('socket error')));
  setTimeout(() => reject(new Error('connect timeout')), 10000);
});
await wait(400);

console.log('== nothing running means no offset ==');
check('absolute is zero', latest().offset.absolute, 0);
check('relative is zero', latest().offset.relative, 0);

console.log('== starting on time ==');
send('start', undefined);
await wait(1500);
near('absolute ~0', latest().offset.absolute, 0, 3000);
near('relative ~0', latest().offset.relative, 0, 3000);
check('expectedRundownEnd is reported', typeof latest().offset.expectedRundownEnd, 'number');

console.log('== adding time puts the run behind ==');
send('addtime', 120000);
await wait(600);
near('absolute grew by the added time', latest().offset.absolute, 120000, 4000);

console.log('== removing it again brings it back ==');
send('addtime', -120000);
await wait(600);
near('absolute back to ~0', latest().offset.absolute, 0, 4000);

console.log('== pausing accrues lateness while paused ==');
send('pause', undefined);
await wait(2500);
near('paused time counted', latest().offset.absolute, 2500, 2500);

send('start', undefined);
await wait(600);

console.log('== starting the next event late shows as behind ==');
// event B was planned to start at +60s; starting it now is early by design,
// so what matters is that the figure tracks the plan rather than staying at zero
const beforeLoad = latest().offset.absolute;
send('start', 'next');
await wait(800);
check('offset changed with the new event', latest().offset.absolute !== beforeLoad, true);
check('event B is loaded', latest().eventNow?.title, 'OFF_B');

console.log('== stopping clears it ==');
send('stop', undefined);
await wait(600);
check('absolute cleared', latest().offset.absolute, 0);
check('relative cleared', latest().offset.relative, 0);

ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
