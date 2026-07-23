// Covers the operator controls that previously did nothing when pressed:
// aux timers, the message overlay, the run report and the client list.
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

// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const room = 'ops-' + Math.random().toString(36).slice(2, 10);
const api = (method, path, body) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));

// --- a rundown to run --------------------------------------------------
const list = await api('POST', '/data/rundowns', { title: '__ops__' });
const rid = list.rundowns.filter((r) => r.title === '__ops__').pop().id;
await api('POST', `/data/rundowns/${rid}/load`);
const a = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'OPS_A', duration: 60000 });
const b = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'OPS_B', duration: 60000 });

// --- connect -----------------------------------------------------------
const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
const runtime = [];
const clientLists = [];
const send = (tag, payload) => ws.send(JSON.stringify({ tag, payload }));
const latest = () => runtime[runtime.length - 1];

let socketClosed = null;
ws.addEventListener('close', (event) => {
  socketClosed = `code ${event.code}${event.reason ? ` (${event.reason})` : ''}`;
});
ws.addEventListener('message', (e) => {
  const p = JSON.parse(e.data);
  if (p.tag === 'runtime-data') runtime.push(p.payload);
  if (p.tag === 'client-list') clientLists.push(p.payload);
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
  failed = true;
  console.log(`  FAIL timed out waiting for ${what}`);
  return latest() ?? {};
}

/// The same, for the client list, which arrives on its own packet.
async function untilList(predicate, what, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const list = clientLists[clientLists.length - 1];
    if (list && predicate(list)) return list;
    await wait(20);
  }
  failed = true;
  console.log(`  FAIL timed out waiting for ${what}`);
  return clientLists[clientLists.length - 1] ?? {};
}

await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('socket error')));
  setTimeout(() => reject(new Error('connect timeout')), 10000);
});
await wait(500);

console.log('== aux timers are in the runtime store ==');
for (const key of ['auxtimer1', 'auxtimer2', 'auxtimer3']) {
  const t = latest()[key];
  check(`${key} present and stopped`, t?.playback, 'stop');
}
check('aux1 defaults to five minutes', latest().auxtimer1.duration, 300000);
check('aux1 counts down by default', latest().auxtimer1.direction, 'count-down');

console.log('== an aux timer runs on its own, with playback stopped ==');
send('auxtimer', { 1: { duration: 30000 } });
await until((p) => p.auxtimer1.duration === 30000, 'the aux duration');
check('duration set', latest().auxtimer1.duration, 30000);

send('auxtimer', { 1: 'start' });
// waiting for it to have counted is the assertion: if the server did not keep
// ticking for an aux timer while playback is stopped, this never arrives
const counted = await until(
  (p) => p.auxtimer1.playback === 'start' && p.auxtimer1.current <= 28000,
  'aux1 to count down while playback is stopped',
);
check('playback is still stopped', counted.timer.playback, 'stop');
check('aux1 is running', counted.auxtimer1.playback, 'start');
near('aux1 counted down a couple of seconds', counted.auxtimer1.current, 30000 - 2500, 2000);
check('aux2 untouched', counted.auxtimer2.playback, 'stop');

console.log('== pause freezes it ==');
send('auxtimer', { 1: 'pause' });
await until((p) => p.auxtimer1.playback === 'pause', 'the aux pause');
const atPause = latest().auxtimer1.current;
check('aux1 paused', latest().auxtimer1.playback, 'pause');
await wait(1500);
check('did not move while paused', latest().auxtimer1.current, atPause);

console.log('== stop resets to the configured duration ==');
send('auxtimer', { 1: 'stop' });
await until((p) => p.auxtimer1.playback === 'stop', 'the aux stop');
check('aux1 stopped', latest().auxtimer1.playback, 'stop');
check('reset to 30s', latest().auxtimer1.current, 30000);

console.log('== counting up ==');
send('auxtimer', { 2: { duration: 0 } });
send('auxtimer', { 2: { direction: 'count-up' } });
send('auxtimer', { 2: 'start' });
const countedUp = await until(
  (p) => p.auxtimer2.direction === 'count-up' && p.auxtimer2.current >= 2000,
  'aux2 to count up',
);
check('aux2 counts up', countedUp.auxtimer2.direction, 'count-up');
near('aux2 counted up a couple of seconds', countedUp.auxtimer2.current, 2500, 2000);
send('auxtimer', { 2: 'stop' });

console.log('== the message overlay ==');
send('message', { timer: { text: 'STAND BY' } });
const withText = await until((p) => p.message?.timer?.text === 'STAND BY', 'the overlay text');
check('text set', withText.message?.timer?.text, 'STAND BY');
check('visible untouched', withText.message.timer.visible, false);

send('message', { timer: { visible: true, blink: true } });
const shown = await until((p) => p.message.timer.visible && p.message.timer.blink, 'the overlay to show');
check('visible set', shown.message.timer.visible, true);
check('blink set', shown.message.timer.blink, true);
// a partial patch must not reset the other fields
check('text preserved', shown.message.timer.text, 'STAND BY');

send('message', { secondary: 'on air in 5' });
const secondary = await until((p) => p.message.secondary === 'on air in 5', 'the secondary message');
check('secondary set', secondary.message.secondary, 'on air in 5');
check('timer text still there', secondary.message.timer.text, 'STAND BY');

send('message', { timer: { blackout: true, secondarySource: 'aux1' } });
const blacked = await until((p) => p.message.timer.blackout, 'the blackout');
check('blackout set', blacked.message.timer.blackout, true);
check('secondarySource set', blacked.message.timer.secondarySource, 'aux1');

console.log('== the report records what actually ran ==');
check('starts empty', await api('GET', '/data/report'), {});

send('start', undefined);
await until((p) => p.timer.playback === 'play', 'playback to start');
let report = await api('GET', '/data/report');
check('event A has a start', typeof report[a.id]?.startedAt, 'number');
check('event A has no end yet', report[a.id]?.endedAt, null);

send('load', 'next');
await until((p) => p.eventNow?.title === 'OPS_B', 'the next event to load');
report = await api('GET', '/data/report');
check('event A now has an end', typeof report[a.id]?.endedAt, 'number');

send('start', undefined);
await until((p) => p.timer.playback === 'play', 'playback to restart');
send('stop', undefined);
await until((p) => p.timer.playback === 'stop', 'playback to stop');
report = await api('GET', '/data/report');
check('event B was recorded', typeof report[b.id]?.startedAt, 'number');
check('event B has an end', typeof report[b.id]?.endedAt, 'number');

console.log('== the report can be cleared ==');
await api('DELETE', `/data/report/${a.id}`);
report = await api('GET', '/data/report');
check('A cleared', a.id in report, false);
check('B kept', b.id in report, true);

await api('DELETE', '/data/report/all');
check('all cleared', await api('GET', '/data/report'), {});

console.log('== the client list ==');
send('client-set', { name: 'Stage Manager' });
await wait(400);
const list1 = clientLists[clientLists.length - 1];
check('a list was broadcast', typeof list1, 'object');
const entries = Object.values(list1 ?? {});
check('one client listed', entries.length, 1);
check('name applied', entries[0]?.name, 'Stage Manager');

send('client-set-path', '/timer');
await wait(400);
const list2 = clientLists[clientLists.length - 1];
check('path applied', Object.values(list2)[0]?.path, '/timer');
check('name preserved', Object.values(list2)[0]?.name, 'Stage Manager');

console.log('== a second connection appears in the list ==');
const ws2 = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
await new Promise((resolve) => {
  ws2.addEventListener('open', resolve);
  setTimeout(resolve, 5000);
});
ws2.send(JSON.stringify({ tag: 'client-set', payload: { name: 'Lighting' } }));
const twoListed = await untilList((l) => Object.keys(l).length === 2, 'both clients to be listed');
check('two clients listed', Object.keys(twoListed).length, 2);

ws2.close();
const backToOne = await untilList((l) => Object.keys(l).length === 1, 'the list to drop back to one');
check('back to one after disconnect', Object.keys(backToOne).length, 1);

// --- cleanup -----------------------------------------------------------
ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
