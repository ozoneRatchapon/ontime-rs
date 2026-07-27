// The report archive: a real run is preserved permanently and survives the
// rundown being edited or deleted — the reason the room is kept year to year.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';
const room = 'archive-' + Math.random().toString(36).slice(2, 10);

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const wait = ms => new Promise(r => setTimeout(r, ms));
const api = (m, p, b) => fetch(`${base}/r/${room}${p}`, {
  method: m, headers: { 'content-type': 'application/json' },
  body: b ? JSON.stringify(b) : undefined,
}).then(r => r.status === 204 ? null : r.json().catch(() => null));

// two days, each with one short event
const mk = async (title, evTitle) => {
  const list = await api('POST', '/data/rundowns', { title });
  const rid = list.rundowns.filter(r => r.title === title).pop().id;
  await api('POST', `/data/rundowns/${rid}/entry`, { title: evTitle, duration: 4000 });
  return rid;
};
const d1 = await mk('DAY_ONE', 'opening');
const d2 = await mk('DAY_TWO', 'keynote');
await api('POST', `/data/rundowns/${d1}/load`);

const ws = new WebSocket(base.replace(/^http/, 'ws') + `/ws?room=${room}`);
const send = (t, p) => ws.send(JSON.stringify({ tag: t, payload: p }));
const runtime = [];
ws.addEventListener('message', e => { const p = JSON.parse(e.data); if (p.tag === 'runtime-data') runtime.push(p.payload); });
await new Promise((res, rej) => { ws.addEventListener('open', res); ws.addEventListener('error', () => rej(new Error('sock'))); setTimeout(() => rej(new Error('timeout')), 10000); });
await wait(400);

console.log('== run day 1 for real ==');
send('start', undefined);
await wait(1500);
const rep = await api('GET', '/data/report');
check('day 1 recorded a run', Object.keys(rep).length >= 1, true);

console.log('== archive starts empty ==');
check('no runs yet', (await api('GET', '/data/report/archive')).length, 0);

console.log('== switching mid-run does NOT archive or lose the report ==');
send('stop', undefined);
await wait(300);
await api('POST', `/data/rundowns/${d2}/load`); // peek at day 2
await wait(400);
await api('POST', `/data/rundowns/${d1}/load`); // come back to day 1
await wait(400);
check('nothing archived by switching', (await api('GET', '/data/report/archive')).length, 0);
check('day 1 report still intact', Object.keys(await api('GET', '/data/report')).length >= 1, true);

console.log('== explicit snapshot preserves the run and clears live ==');
const snap = await api('POST', '/data/report/archive');
check('snapshot saved', snap.archived, true);
const arch = await api('GET', '/data/report/archive');
check('one run archived', arch.length, 1);
check('it is day 1', arch[0]?.rundownTitle, 'DAY_ONE');
check('event title baked in', arch[0]?.events?.[0]?.title, 'opening');
check('real start time', typeof arch[0]?.events?.[0]?.startedAt, 'number');
check('live report cleared after save', Object.keys(await api('GET', '/data/report')).length, 0);

console.log('== the archived run survives day 1 being deleted ==');
await api('DELETE', `/data/rundowns/${d1}`);
const arch2 = await api('GET', '/data/report/archive');
check('still there after delete', arch2.length, 1);
check('title still analysable', arch2[0]?.events?.[0]?.title, 'opening');
check('planned duration preserved', arch2[0]?.events?.[0]?.plannedDuration, 4000);
ws.close();
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${d2}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
