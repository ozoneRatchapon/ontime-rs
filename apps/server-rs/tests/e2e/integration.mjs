// The integration API (plan 009): what Companion, a Stream Deck or a script
// sees. `GET /r/<room>/api/<action>/...` and websocket actions go through the
// same parser, so both are driven here, against a real room.
//
// The contract tests pin what each request parses to against upstream; this
// pins the wiring: the room changes, the screens hear it, a screen can be
// renamed, and only a token (never a cookie) opens a protected room.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};

const room = 'integration-' + Math.random().toString(36).slice(2, 10);
// the master token, on a gated deployment (see README)
const master = process.env.ONTIME_TOKEN;
const auth = (token) => (token ? { Authorization: `Bearer ${token}` } : {});
const rest = (method, path, body, token = master) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...auth(token) },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));
/** An integration call: `{ status, body }`. */
const api = async (path, { token = master, headers = {}, method = 'GET' } = {}) => {
  const response = await fetch(`${base}/r/${room}/api${path}`, { method, headers: { ...auth(token), ...headers } });
  return { status: response.status, body: await response.json().catch(() => null) };
};
const poll = async () => (await api('/poll')).body.payload;

/** A screen: its id, every packet it has had, and a way to wait for one. */
const screen = async () => {
  const query = new URLSearchParams({ room, ...(master ? { token: master } : {}) });
  const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?${query}`);
  const packets = [];
  const waiters = [];
  ws.addEventListener('message', (event) => {
    const packet = JSON.parse(event.data);
    packets.push(packet);
    for (const waiter of [...waiters]) {
      if (waiter.test(packet)) {
        waiters.splice(waiters.indexOf(waiter), 1);
        waiter.resolve(packet);
      }
    }
  });
  const next = (test, what) =>
    new Promise((resolve, reject) => {
      const seen = packets.find(test);
      if (seen) return resolve(seen);
      waiters.push({ test, resolve });
      setTimeout(() => reject(new Error(`no ${what}`)), 10000);
    });
  const init = await next((p) => p.tag === 'client-init', 'client-init');
  return {
    id: init.payload.clientId,
    send: (tag, payload) => ws.send(JSON.stringify({ tag, payload })),
    next,
    fresh: () => packets.splice(0),
    close: () => ws.close(),
  };
};

console.log('== a room to drive ==');
await rest('POST', '/data/settings', { timezoneOffset: 0 });
const list = await rest('POST', '/data/rundowns', { title: '__integration__' });
const rid = list.rundowns.filter((r) => r.title === '__integration__').pop().id;
await rest('POST', `/data/rundowns/${rid}/load`);
const ids = [];
for (const [title, cue] of [
  ['First', 'A1'],
  ['Second', 'B2'],
  ['Third', 'C3'],
]) {
  const entry = await rest('POST', `/data/rundowns/${rid}/entry`, { title, cue, duration: 600000 });
  ids.push(entry.id);
}

console.log('== the greeting ==');
const hello = await api('');
check('GET /api answers', hello.status, 200);
check('with upstream greeting', hello.body.message, 'You have reached Ontime API server');
check('the version', (await api('/version')).body.payload, '4.11.0');

console.log('== playback, by position, cue and step ==');
const watcher = await screen();
watcher.fresh();
const started = await api('/start/index/2');
check('start/index/2 is accepted', started.status, 202);
check('and answers success', started.body.payload, 'success');
const playing = await watcher.next(
  (p) => p.tag === 'runtime-data' && p.payload.timer.playback === 'play',
  'runtime-data after start',
);
check('the screens hear it, event 2 playing', playing.payload.eventNow?.id, ids[1]);

check('load/cue/C3', (await api('/load/cue/C3')).status, 202);
let state = await poll();
check('event 3 is armed', [state.eventNow?.id, state.timer.playback], [ids[2], 'armed']);
check('load/next after the last wraps to the top', (await api('/load/next')).status, 202);
check('event 1 is armed', (await poll()).eventNow?.id, ids[0]);
check('load/previous at the top keeps the first', (await api('/load/previous')).status, 202);
check('still event 1', (await poll()).eventNow?.id, ids[0]);

check('start', (await api('/start')).status, 202);
check('addtime/add/60000', (await api('/addtime/add/60000')).status, 202);
check('the minute is added', (await poll()).timer.addedTime, 60000);
check('addtime/remove/30000', (await api('/addtime/remove/30000')).status, 202);
check('half of it taken back', (await poll()).timer.addedTime, 30000);
check('pause', (await api('/pause')).status, 202);
check('paused', (await poll()).timer.playback, 'pause');

console.log('== refusals carry upstream messages ==');
const outOfRange = await api('/start/index/9');
check('an index past the end is refused', outOfRange.status, 400);
check('with upstream message', outOfRange.body.message, 'Event index not recognised or out of range 9');
const tooLarge = await api('/addtime/3600001');
check('more than an hour is refused', [tooLarge.status, tooLarge.body.message], [400, 'Payload too large: 3600001']);
const unknown = await api('/explode');
check('an unknown action', [unknown.status, unknown.body.message], [400, 'Unhandled message explode']);
check('a POST is not the integration API', (await api('/start', { method: 'POST' })).status, 405);

console.log('== the overlay, the aux timers, an edit ==');
check('message text', (await api('/message/timer/text/Wrap%20up')).status, 202);
const shown = await api('/message/timer/visible/true');
check('the reply is the overlay', [shown.body.payload.timer.text, shown.body.payload.timer.visible], ['Wrap up', true]);
const aux = await api('/auxtimer/1/duration/90000');
check('the reply is the aux timer', aux.body.payload.duration, 90000);
check('auxtimer/1/start', (await api('/auxtimer/1/start')).status, 202);
check('the aux timer runs', (await poll()).auxtimer1.playback, 'start');
const renamed = await api(`/change/${ids[1]}/title/Renamed`);
check('change title', renamed.status, 202);
const refetch = await watcher.next((p) => p.tag === 'refetch' && p.payload.target === 'rundown', 'refetch');
check('the screens refetch the rundown', refetch.payload.rundownId, rid);
const rundown = await rest('GET', `/data/rundowns/${rid}`);
// a path segment reaches the field as sent, still encoded, as upstream
check('the title is changed', rundown.entries[ids[1]].title, 'Renamed');
check('a field the entry lacks', (await api(`/change/${ids[1]}/speed/2`)).body.message, 'Invalid property or value');

console.log('== the websocket takes the same actions, and answers ==');
watcher.fresh();
watcher.send('version');
check('version over the socket', (await watcher.next((p) => p.tag === 'version', 'version reply')).payload, '4.11.0');
watcher.send('start', { cue: 'A1' });
await watcher.next((p) => p.tag === 'start', 'start reply');
check('start by cue over the socket', (await poll()).eventNow?.id, ids[0]);
watcher.send('addtime', { add: 5000 });
await watcher.next((p) => p.tag === 'addtime', 'addtime reply');
check('addtime {add} over the socket', (await poll()).timer.addedTime, 5000);

console.log('== screens: rename, redirect, identify ==');
const stage = await screen();
watcher.send('client', { target: stage.id, rename: 'Stage left' });
const rename = await stage.next((p) => p.tag === 'client-rename', 'client-rename');
check('the screen is told its new name', rename.payload, { name: 'Stage left', target: stage.id });
const listed = await watcher.next(
  (p) => p.tag === 'client-list' && p.payload[stage.id]?.name === 'Stage left',
  'client-list with the new name',
);
check('the client list shows it', listed.payload[stage.id].name, 'Stage left');
check('redirect over HTTP', (await api(`/client?target=${stage.id}&redirect=%2Ftimer`)).status, 202);
const redirect = await stage.next((p) => p.tag === 'client-redirect', 'client-redirect');
check('the screen is sent to /timer', redirect.payload.path, '/timer');
// identify takes a boolean, which a query string cannot carry: the socket can
watcher.send('client', { target: stage.id, identify: true });
const flagged = await watcher.next(
  (p) => p.tag === 'client-list' && p.payload[stage.id]?.identify === true,
  'client-list with the screen identified',
);
check('identify over the socket', flagged.payload[stage.id].identify, true);
const missing = await api('/client?target=nobody&rename=X');
check('an unknown screen', [missing.status, missing.body.message], [400, 'Client "nobody" not found']);
stage.close();

if (master) {
  console.log('== an edit from outside is in the activity log ==');
  // reading the log is for an owner or the admin; wrangler dev has neither
  const log = await (await fetch(`${base}/r/${room}/data/activity`, { headers: auth(master) })).json();
  const edit = log.lines.find((line) => line.action === 'entry-edit');
  check('the change is logged as an entry edit', edit?.subject?.title, 'Renamed');
}

console.log('== a flood is slowed, not served ==');
const floodStart = Date.now();
const flood = await Promise.all(
  Array.from({ length: 60 }, () => fetch(`${base}/r/${room}/api/pause`, { headers: auth(master) })),
);
const floodSeconds = (Date.now() - floodStart) / 1000;
const limited = flood.filter((r) => r.status === 429);
const accepted = flood.filter((r) => r.status === 202).length;
check('a flood is refused in part', limited.length > 0, true);
// the bucket's own bound: the burst of 40, and 20 a second while the calls
// arrive, however spread out the network makes them
check(
  `no more taken than the limit allows (${accepted} in ${floodSeconds.toFixed(2)}s)`,
  accepted <= 40 + Math.ceil(20 * floodSeconds) + 1,
  true,
);
check('with Retry-After', Number(limited[0]?.headers.get('retry-after')) >= 1, true);
check('polling still answers', (await api('/poll')).status, 202);
// twenty a second: two seconds refill the whole burst
await new Promise((resolve) => setTimeout(resolve, 2100));
check('and calls flow again', (await api('/pause')).status, 202);

console.log('== a protected room takes a token, never a cookie ==');
const claim = await rest('POST', '/data/session/password', { password: 'integration-secret' }, master);
const roomToken = claim?.token;
check('the room is protected', claim?.protected, true);
check('without a token', (await api('/pause', { token: null })).status, 401);
check('with ?token=', (await fetch(`${base}/r/${room}/api/pause?token=${roomToken}`)).status, 202);
check('with a Bearer token', (await api('/pause', { token: roomToken })).status, 202);
const cookie = { Cookie: `token=${JSON.stringify({ token: roomToken, scope: 'rw' })}` };
check('a cookie alone is refused', (await api('/pause', { token: null, headers: cookie })).status, 401);
await rest('POST', '/data/session/password', { password: null }, roomToken);

console.log('== a room that does not exist ==');
const ghost = await fetch(`${base}/r/never-made-${Math.random().toString(36).slice(2, 8)}/api/start`, {
  headers: auth(master),
});
check('is not created by an integration call', ghost.status, 404);

watcher.close();
await api('/stop');
await rest('POST', '/data/rundowns/default/load');
await rest('DELETE', `/data/rundowns/${rid}`);
if (master) {
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: auth(master) });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
