// Roll mode: the rundown follows the wall clock without an operator.
//
// The rundown is built around the server's own clock so the suite works
// whenever it runs, and uses short events so it finishes in seconds.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};
const near = (label, actual, expected, tolerance) => {
  const ok = typeof actual === 'number' && Math.abs(actual - expected) <= tolerance;
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ~${expected} (±${tolerance})`}`,
  );
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const room = 'roll-' + Math.random().toString(36).slice(2, 10);
// the master token, so the suite can create its throwaway room on a gated
// deployment (see README); without it the suite runs against wrangler dev
const token = process.env.ONTIME_TOKEN;
const socketUrl = () =>
  `${base.replace(/^http/, 'ws')}/ws?${new URLSearchParams({ room, ...(token ? { token } : {}) })}`;
const api = (method, path, body) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));

await api('POST', '/data/settings', { timezoneOffset: 0 });

// read the server's clock so the rundown can be positioned against it
const probe = new WebSocket(socketUrl());
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

// created first, then positioned against a freshly read clock: setting the
// times up front lets slow round trips eat the lead before the test begins
const a = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'ROLL_A', duration: 5000 });
const b = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'ROLL_B', duration: 5000 });

const ws = new WebSocket(socketUrl());
const runtime = [];
const send = (tag, payload) => {
  assertSocketAlive(tag);
  ws.send(JSON.stringify({ tag, payload }));
};
const latest = () => runtime[runtime.length - 1];
// a deploy rolling over closes the socket; without noticing, the suite would
// carry on and fail on a stale packet, which reads as a mysterious flake
let socketClosed = null;
ws.addEventListener('close', (event) => {
  socketClosed = `code ${event.code}${event.reason ? ` (${event.reason})` : ''}`;
});
ws.addEventListener('message', (e) => {
  const p = JSON.parse(e.data);
  if (p.tag === 'runtime-data') runtime.push(p.payload);
});

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

// now that everything is in place, put the events just ahead of the clock
const clockNow = latest().clock;
const startA = clockNow + 4000;
await api('PUT', `/data/rundowns/${rid}/entry`, {
  id: a.id,
  timeStart: startA,
  timeEnd: startA + 5000,
});
await api('PUT', `/data/rundowns/${rid}/entry`, {
  id: b.id,
  timeStart: startA + 5000,
  timeEnd: startA + 10000,
});
await wait(300);

console.log('== rolling before the show waits for it ==');
send('roll', undefined);
await until((p) => p.timer.playback === 'roll', 'roll to engage');
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
await until((p) => p.timer.playback === 'stop', 'the stop to apply');
check('stopped', latest().timer.playback, 'stop');
check('nothing loaded', latest().eventNow, null);

console.log('== rolling mid-event joins it part way ==');
// B is under way by now; rolling again should pick it up, not restart it
send('roll', undefined);
const joined = await until((p) => p.timer.playback === 'roll', 'roll to re-engage');
check('joined an event already running', joined.timer.playback, 'roll');
check('it is not pending', joined.timer.phase !== 'pending', true);

send('stop', undefined);
await wait(300);
ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);
if (token) {
  // take it off the admin room list again
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${token}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
