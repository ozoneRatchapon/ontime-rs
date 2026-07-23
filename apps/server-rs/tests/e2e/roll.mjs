// Roll mode: the rundown follows the wall clock without an operator.
//
// The rundown is built around the server's own clock so the suite works
// whenever it runs, and uses short events so it finishes in seconds.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const near = (label, actual, expected, tolerance) => {
  const ok = typeof actual === 'number' && Math.abs(actual - expected) <= tolerance;
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ~${expected} (±${tolerance})`}`);
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const room = 'roll-' + (Math.floor(Date.now() / 1000) % 100000);
const api = (method, path, body) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));

await api('POST', '/data/settings', { timezoneOffset: 0 });

// read the server's clock so the rundown can be positioned against it
const probe = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
const serverClock = await new Promise((resolve, reject) => {
  probe.addEventListener('message', (e) => {
    const p = JSON.parse(e.data);
    if (p.tag === 'runtime-data') resolve(p.payload.clock);
  });
  probe.addEventListener('error', () => reject(new Error('socket error')));
  setTimeout(() => reject(new Error('no clock')), 10000);
});
probe.close();

const list = await api('POST', '/data/rundowns', { title: '__roll__' });
const rid = list.rundowns.filter((r) => r.title === '__roll__').pop().id;
await api('POST', `/data/rundowns/${rid}/load`);

// A starts in 4s and runs 5s; B follows immediately and runs 5s
const startA = serverClock + 4000;
const a = await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'ROLL_A', timeStart: startA, duration: 5000, timeEnd: startA + 5000,
});
const b = await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'ROLL_B', timeStart: startA + 5000, duration: 5000, timeEnd: startA + 10000,
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

console.log('== rolling before the show waits for it ==');
send('roll', undefined);
await wait(700);
check('playback is roll', latest().timer.playback, 'roll');
check('the first event is armed', latest().eventNow?.title, 'ROLL_A');
check('phase is pending', latest().timer.phase, 'pending');
near('counting down to the start', latest().timer.secondaryTimer, 3300, 2000);

console.log('== it starts itself when the time comes ==');
await wait(4500);
check('now playing', latest().eventNow?.title, 'ROLL_A');
check('no longer pending', latest().timer.phase !== 'pending', true);
check('secondary timer cleared', latest().timer.secondaryTimer, null);
near('the event timer is running', latest().timer.current, 4000, 3000);

console.log('== it hands over to the next event by itself ==');
await wait(5000);
check('rolled into B', latest().eventNow?.title, 'ROLL_B');
check('still rolling', latest().timer.playback, 'roll');

console.log('== stop leaves roll mode ==');
send('stop', undefined);
await wait(600);
check('stopped', latest().timer.playback, 'stop');
check('nothing loaded', latest().eventNow, null);

console.log('== rolling mid-event joins it part way ==');
// B is under way by now; rolling again should pick it up, not restart it
send('roll', undefined);
await wait(700);
const joined = latest();
check('joined an event already running', joined.timer.playback, 'roll');
check('it is not pending', joined.timer.phase !== 'pending', true);

send('stop', undefined);
await wait(300);
ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
