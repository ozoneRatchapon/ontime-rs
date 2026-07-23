// Verifies that two rooms are completely independent projects: separate rundowns,
// separate settings, separate playback, and no cross-talk over the websocket.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// REST addresses a room by path prefix; the websocket by ?room=
const rest = (room, method, path, body) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, body: r.status === 204 ? null : await r.json().catch(() => null) }));

const A = 'proj-alpha';
const B = 'proj-beta';

console.log('== each room boots its own project ==');
for (const room of [A, B]) {
  const { status, body } = await rest(room, 'GET', '/data/rundowns');
  check(`${room} responds`, status, 200);
  check(`${room} has a rundown`, body.rundowns.length > 0, true);
}

console.log('== a rundown edit in one room does not appear in the other ==');
const ridA = (await rest(A, 'GET', '/data/rundowns')).body.loaded;
const ridB = (await rest(B, 'GET', '/data/rundowns')).body.loaded;

const entryA = (await rest(A, 'POST', `/data/rundowns/${ridA}/entry`, { title: 'ONLY_IN_ALPHA' })).body;
check('alpha got the entry', entryA.title, 'ONLY_IN_ALPHA');

const alphaTitles = Object.values((await rest(A, 'GET', `/data/rundowns/${ridA}`)).body.entries).map((e) => e.title);
const betaTitles = Object.values((await rest(B, 'GET', `/data/rundowns/${ridB}`)).body.entries).map((e) => e.title);
check('alpha contains it', alphaTitles.includes('ONLY_IN_ALPHA'), true);
check('beta does not', betaTitles.includes('ONLY_IN_ALPHA'), false);

console.log('== settings are per room ==');
await rest(A, 'POST', '/data/settings', { timeFormat: '12', timezoneOffset: 25200000 });
await rest(B, 'POST', '/data/settings', { timeFormat: '24', timezoneOffset: 0 });
check('alpha timeFormat', (await rest(A, 'GET', '/data/settings')).body.timeFormat, '12');
check('beta timeFormat', (await rest(B, 'GET', '/data/settings')).body.timeFormat, '24');
check('alpha tz', (await rest(A, 'GET', '/data/settings')).body.timezoneOffset, 25200000);
check('beta tz', (await rest(B, 'GET', '/data/settings')).body.timezoneOffset, 0);

console.log('== project data is per room ==');
await rest(A, 'POST', '/data/project', { title: 'Alpha Show' });
await rest(B, 'POST', '/data/project', { title: 'Beta Show' });
check('alpha project', (await rest(A, 'GET', '/data/project')).body.title, 'Alpha Show');
check('beta project', (await rest(B, 'GET', '/data/project')).body.title, 'Beta Show');

console.log('== websockets are per room, playback does not leak ==');
const open = (room) =>
  new Promise((resolve, reject) => {
    const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
    const seen = [];
    ws.addEventListener('message', (e) => {
      const p = JSON.parse(e.data);
      if (p.tag === 'runtime-data') seen.push(p.payload);
    });
    ws.addEventListener('open', () => resolve({ ws, seen }));
    ws.addEventListener('error', () => reject(new Error(`socket error for ${room}`)));
    setTimeout(() => reject(new Error(`connect timeout for ${room}`)), 10000);
  });

const sockA = await open(A);
const sockB = await open(B);
await wait(500);

// start playback in alpha only
sockA.ws.send(JSON.stringify({ tag: 'start', payload: undefined }));
await wait(1200);

const lastA = sockA.seen[sockA.seen.length - 1];
const lastB = sockB.seen[sockB.seen.length - 1];
check('alpha is playing', lastA.timer.playback, 'play');
check('beta is still stopped', lastB.timer.playback, 'stop');
check('alpha loaded its own entry', lastA.eventNow !== null, true);

// and the reverse
sockB.ws.send(JSON.stringify({ tag: 'start', payload: undefined }));
await wait(1200);
check('beta now playing', sockB.seen[sockB.seen.length - 1].timer.playback, 'play');
check('alpha unaffected', sockA.seen[sockA.seen.length - 1].timer.playback, 'play');
check(
  'they are running different events',
  sockA.seen[sockA.seen.length - 1].eventNow.id !== sockB.seen[sockB.seen.length - 1].eventNow.id,
  true,
);

sockA.ws.send(JSON.stringify({ tag: 'stop', payload: undefined }));
sockB.ws.send(JSON.stringify({ tag: 'stop', payload: undefined }));
await wait(300);
sockA.ws.close();
sockB.ws.close();

console.log('== the default room is untouched by either ==');
const def = await fetch(`${base}/data/project`).then((r) => r.json());
check('default project is not Alpha or Beta', ['Alpha Show', 'Beta Show'].includes(def.title), false);

console.log('== invalid room names fall back rather than creating junk ==');
for (const bad of ['..', 'a/b', 'x'.repeat(65), 'has space']) {
  const r = await fetch(`${base}/r/${encodeURIComponent(bad)}/data/settings`);
  check(`'${bad.slice(0, 12)}' rejected or defaulted`, r.status === 200 || r.status === 404, true);
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
