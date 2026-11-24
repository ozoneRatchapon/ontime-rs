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
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ~${expected} (±${tolerance})`}`,
  );
};
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const room = 'offset-' + Math.random().toString(36).slice(2, 10);
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

// The rundown is planned to start at whatever the server's clock reads now, so
// starting immediately is "on time" regardless of when this suite runs.
await api('POST', '/data/settings', { timezoneOffset: 0 });
const serverClock = await (async () => {
  const ws = new WebSocket(socketUrl());
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

const a = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'OFF_A', duration: 60000 });
const b = await api('POST', `/data/rundowns/${rid}/entry`, {
  title: 'OFF_B',
  duration: 60000,
  linkStart: true,
});

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

/// Waits until a packet satisfying `predicate` arrives. A heartbeat can land
/// between a command and its effect, so "the next packet" is not enough.
async function until(predicate, what, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const packet = latest();
    if (packet && predicate(packet)) return packet;
    await wait(20);
  }
  failed = true;
  console.log(`  FAIL timed out waiting for ${what}`);
  return latest() ?? {};
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

// only now, with the round trips behind us, place the plan against the clock —
// otherwise the setup time itself reads as the show running late
const plannedStart = latest().clock + 1500;
await api('PUT', `/data/rundowns/${rid}/entry`, {
  id: a.id,
  timeStart: plannedStart,
  timeEnd: plannedStart + 60000,
});
await api('PUT', `/data/rundowns/${rid}/entry`, {
  id: b.id,
  timeStart: plannedStart + 60000,
  timeEnd: plannedStart + 120000,
});
await wait(300);

console.log('== nothing running means no offset ==');
check('absolute is zero', latest().offset.absolute, 0);
check('relative is zero', latest().offset.relative, 0);

console.log('== starting on time ==');
send('start', undefined);
const started = await until((p) => p.timer.playback === 'play', 'playback to start');
near('absolute ~0', started.offset.absolute, 0, 3000);
near('relative ~0', latest().offset.relative, 0, 3000);
check('expectedRundownEnd is reported', typeof latest().offset.expectedRundownEnd, 'number');

console.log('== adding time puts the run behind ==');
send('addtime', 120000);
const added = await until((p) => p.timer.addedTime === 120000, 'the added time');
near('absolute grew by the added time', added.offset.absolute, 120000, 4000);

console.log('== removing it again brings it back ==');
send('addtime', -120000);
const removed = await until((p) => p.timer.addedTime === 0, 'the added time to be removed');
near('absolute back to ~0', removed.offset.absolute, 0, 4000);

console.log('== pausing accrues lateness while paused ==');
send('pause', undefined);
await until((p) => p.timer.playback === 'pause', 'the pause to apply');
await wait(2500);
// a paused show keeps losing time against the schedule, so the figure grows
near('paused time counted', latest().offset.absolute, 2500, 2500);

send('start', undefined);
await until((p) => p.timer.playback === 'play', 'the resume to apply');

console.log('== starting the next event late shows as behind ==');
// event B was planned to start at +60s; starting it now is early by design,
// so what matters is that the figure tracks the plan rather than staying at zero
const beforeLoad = latest().offset.absolute;
send('start', 'next');
const onB = await until((p) => p.eventNow?.title === 'OFF_B', 'event B to load');
check('event B is loaded', onB.eventNow?.title, 'OFF_B');
check('offset changed with the new event', onB.offset.absolute !== beforeLoad, true);

console.log('== the editor switches the offset mode ==');
// the Absolute / Relative toggle in the rundown header sends this
send('offsetmode', 'relative');
const relative = await until((p) => p.offset.mode === 'relative', 'relative mode');
check('mode is relative', relative.offset.mode, 'relative');
check('the expected end is still reported', typeof relative.offset.expectedRundownEnd, 'number');
check('the day the run is on is reported', relative.rundown.currentDay, 0);

console.log('== stopping clears it ==');
send('stop', undefined);
const cleared = await until((p) => p.timer.playback === 'stop', 'the stop to apply');
check('absolute cleared', cleared.offset.absolute, 0);
check('relative cleared', cleared.offset.relative, 0);
check('nothing is expected once stopped', cleared.offset.expectedRundownEnd, null);
// a setting of the room, not of the run, as upstream keeps it
check('the mode survives the stop', cleared.offset.mode, 'relative');
send('offsetmode', 'absolute');
await until((p) => p.offset.mode === 'absolute', 'absolute mode again');

ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);
if (token) {
  // take it off the admin room list again
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${token}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
