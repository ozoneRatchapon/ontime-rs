// Plan 007: the audit log. An owner, an operator and the room password each
// make a change, and the owner reads all three, correctly attributed.
//
// Like members.mjs it needs a worker with a master password:
//
//   npx wrangler dev --port 8788 --local --var SESSION_PASSWORD:admin-e2e
//   ADMIN_PASSWORD=admin-e2e node activity.mjs http://127.0.0.1:8788
//
// Cleans up after itself.
import { wait } from './support.mjs';

const base = process.argv[2] ?? 'http://127.0.0.1:8787';
const password = process.env.ADMIN_PASSWORD;
if (!password) {
  console.log("  skipped: set ADMIN_PASSWORD to the worker's SESSION_PASSWORD to run it");
  process.exit(0);
}

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};

const call = (path, { token, session, method = 'GET', body } = {}) =>
  fetch(`${base}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(session ? { 'X-Ontime-Session': session } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
const json = async (response) => response.json();

const suffix = Math.random().toString(36).slice(2, 10);
const room = `activity-${suffix}`;
const r = (path) => `/r/${room}${path}`;

const { token: master } = await json(await call('/admin/login', { method: 'POST', body: { password } }));
const redeem = async (code) => (await json(await call('/auth/redeem', { method: 'POST', body: { code } }))).token;

// --- people and a room ----------------------------------------------------
const owner = await json(await call('/admin/people', { token: master, method: 'POST', body: { name: `Owner ${suffix}` } }));
const owner_session = await redeem((await json(await call(`/admin/people/${owner.id}/invite`, { token: master, method: 'POST' }))).code);

await call(r('/data/project'), { token: master, method: 'POST', body: { title: 'Activity e2e' } });
const created = await json(await call(r('/data/rundowns'), { token: master, method: 'POST', body: { title: 'Run' } }));
const rundown = created.rundowns.find((x) => x.title === 'Run').id;
await call(r(`/data/rundowns/${rundown}/entry`), {
  token: master, method: 'POST', body: { id: `ev-${suffix}`, type: 'event', cue: '2.1', title: 'Keynote', duration: 600000, timeStart: 32400000 },
});
await call(r(`/data/rundowns/${rundown}/load`), { token: master, method: 'POST' });
await call(`/admin/rooms/${room}`, { token: master, method: 'PUT', body: { owners: [owner.id] } });

const made = await json(await call(`/auth/rooms/${room}/operators`, { token: owner_session, method: 'POST', body: { name: `Op ${suffix}` } }));
const operator_session = await redeem(made.code);
const set_password = await json(await call(r('/data/session/password'), { token: master, method: 'POST', body: { password: `pw-${suffix}` } }));
const room_token = set_password.token;

console.log('== three ways in, one change each ==');
check('the owner renames the project', (await call(r('/data/project'), { session: owner_session, method: 'POST', body: { title: 'Gala' } })).status, 200);
const secret = `phone-081-${suffix}`;
check('the room password edits a note', (await call(r(`/data/rundowns/${rundown}/entry`), {
  token: room_token, method: 'PUT', body: { id: `ev-${suffix}`, note: secret },
})).status, 200);

const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?${new URLSearchParams({ room, session: operator_session })}`);
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('socket error')));
});
ws.send(JSON.stringify({ tag: 'start', payload: undefined }));
await wait(800);
// a double tap is one line. Add-time changes the timer each time; a second
// pause would change nothing, and what changes nothing is not recorded.
ws.send(JSON.stringify({ tag: 'addtime', payload: 60000 }));
ws.send(JSON.stringify({ tag: 'addtime', payload: 60000 }));
await wait(800);
ws.close();

console.log('== who may read ==');
const read = (who, query = '') => call(r(`/data/activity${query}`), who);
check('the owner', (await read({ session: owner_session })).status, 200);
check('the admin', (await read({ token: master })).status, 200);
check('not the operator', (await read({ session: operator_session })).status, 403);
check('not the room password', (await read({ token: room_token })).status, 403);
check('not anyone', (await read({})).status, 401);

console.log('== what the owner sees ==');
const response = await read({ session: owner_session });
check('it is not cached', response.headers.get('cache-control'), 'no-store');
const page = await json(response);
const summary = page.lines.map((l) => [l.action, l.actor.kind === 'person' ? l.actor.name : l.actor.kind]);
check('newest first, each attributed', summary.slice(0, 4), [
  ['add-time', `Op ${suffix}`],
  ['start', `Op ${suffix}`],
  ['entry-edit', 'roomPassword'],
  ['project-data', `Owner ${suffix}`],
]);
check('the double tap merged', [page.lines[0].times, page.lines[0].amount], [2, 60000]);
check('start names the event', page.lines[1].subject, { cue: '2.1', title: 'Keynote' });
check('the edit names the field', page.lines[2].fields, ['note']);
check('but never its value', JSON.stringify(page).includes(secret), false);
check('the admin set-up is there too', page.lines.filter((l) => l.actor.kind === 'admin').map((l) => l.action), [
  'password-set', 'rundown-load', 'entry-add', 'rundown-create', 'project-data',
]);

console.log('== paging ==');
const first = await json(await read({ token: master }, '?limit=2'));
check('a short page', first.lines.length, 2);
check('says where to continue', typeof first.before, 'number');
const rest = await json(await read({ token: master }, `?before=${first.before}&limit=200`));
check('the rest follows on', rest.lines.length, page.lines.length - 2);
check('and ends', rest.before, null);

console.log('== a removed person keeps their name in old lines ==');
check('the owner removes the operator', (await call(`/auth/rooms/${room}/operators/${made.person.id}`, { token: owner_session, method: 'DELETE' })).status, 200);
const after = await json(await read({ session: owner_session }));
check('still named', after.lines.find((l) => l.action === 'start')?.actor?.name, `Op ${suffix}`);

console.log('== cleanup ==');
await call(r('/data/session/password'), { token: master, method: 'POST', body: { password: null } });
await call(`/admin/people/${owner.id}`, { token: master, method: 'DELETE' });
await call(`/admin/rooms/${room}`, { token: master, method: 'DELETE' });
check('no people left behind', (await json(await call('/admin/people', { token: master }))).people.filter((p) => p.name.endsWith(suffix)).length, 0);

process.exit(failed ? 1 : 0);
