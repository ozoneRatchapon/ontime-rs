// The admin room list: master-password gate, rooms reporting themselves, owner
// labels, live clients, and the room name not being spoofable.
//
// Needs a worker started with a master password, and that password here:
//
//   npx wrangler dev --port 8788 --local --var SESSION_PASSWORD:admin-e2e
//   ADMIN_PASSWORD=admin-e2e node admin.mjs http://127.0.0.1:8788
//
// Without ADMIN_PASSWORD it skips, so the rest of run.sh can keep running
// against an open worker.
import { openSocket, wait } from './support.mjs';

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
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};

const suffix = Math.random().toString(36).slice(2, 10);
const room = `admin-${suffix}`;
const second = `admin2-${suffix}`;
const spoofed = `spoofed-${suffix}`;

const call = (path, { token, method = 'GET', body, headers = {} } = {}) =>
  fetch(`${base}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...headers,
    },
    body: body ? JSON.stringify(body) : undefined,
  });

const listed = async (token) => {
  const response = await call('/admin/rooms', { token });
  return (await response.json()).rooms;
};
const find = async (token, name) => (await listed(token)).find((record) => record.name === name);

console.log('== the gate ==');
check('list without a token', (await call('/admin/rooms')).status, 401);
check('list with a wrong token', (await call('/admin/rooms', { token: 'nope' })).status, 401);
check('wrong password', (await call('/admin/login', { method: 'POST', body: { password: 'wrong' } })).status, 401);
const login = await call('/admin/login', { method: 'POST', body: { password } });
check('right password', login.status, 200);
const { token } = await login.json();
check('returns the master token', typeof token === 'string' && token.length === 64, true);
check('list with the token', (await call('/admin/rooms', { token })).status, 200);
check('token also works as ?token=', (await fetch(`${base}/admin/rooms?token=${token}`)).status, 200);

console.log('== internal paths are not reachable from outside ==');
check('report via /admin', (await call('/admin/report', { token, method: 'POST', body: { room: 'x' } })).status, 404);
check('registry via a room path', (await call(`/r/${room}/registry/rooms`, { token })).status, 404);

console.log('== a refused room never shows up ==');
check('creating without the master token', (await call(`/r/${room}/data/rundowns`)).status, 403);
check('not listed', await find(token, room), undefined);

console.log('== creating a room registers it ==');
check(
  'create',
  (await call(`/r/${room}/data/project`, { token, method: 'POST', body: { title: 'Admin e2e' } })).status,
  200,
);
const created = await find(token, room);
check('listed', created?.name, room);
check('with its title', created?.title, 'Admin e2e');
check('seen just now', Math.abs(Date.now() - (created?.lastSeen ?? 0)) < 60_000, true);

console.log('== a change shows up without waiting for the report interval ==');
await call(`/r/${room}/data/project`, { token, method: 'POST', body: { title: 'Admin e2e renamed' } });
check('new title listed at once', (await find(token, room))?.title, 'Admin e2e renamed');
await call(`/r/${room}/data/project`, { token, method: 'POST', body: { title: 'Admin e2e' } });

console.log('== owner and note ==');
const labelled = await call(`/admin/rooms/${room}`, {
  token,
  method: 'PUT',
  body: { owner: 'อ.ทดสอบ', note: 'e2e' },
});
check('saved', labelled.status, 200);
const after = await find(token, room);
check('owner', after?.owner, 'อ.ทดสอบ');
check('note', after?.note, 'e2e');
check('title kept', after?.title, 'Admin e2e');
check('bad room name refused', (await call('/admin/rooms/bad%20name', { token, method: 'PUT', body: {} })).status, 400);

console.log('== a room older than the registry is added by hand ==');
const old_room = `old-${suffix}`;
check(
  'register',
  (await call(`/admin/rooms/${old_room}`, { token, method: 'PUT', body: { owner: 'Ann' } })).status,
  200,
);
const old_record = await find(token, old_room);
check('listed, never seen in use', [old_record?.owner, old_record?.lastSeen], ['Ann', 0]);

console.log('== live clients ==');
const empty = await (await call(`/r/${room}/data/clients`, { token })).json();
check('nobody yet', empty.clients.length, 0);
const socket = await openSocket(base, room);
socket.send('client-set-path', '/timer');
await wait(500);
const one = await (await call(`/r/${room}/data/clients`, { token })).json();
check('one screen', one.clients.length, 1);
check('on the timer', one.clients[0]?.path, '/timer');
socket.close();
await wait(500);
check('clients of a room that does not exist', (await call(`/r/nope-${suffix}/data/clients`, { token })).status, 404);

console.log('== a caller cannot choose the name a room reports under ==');
check(
  'create with a spoofed header',
  (
    await call(`/data/project?room=${second}`, {
      token,
      method: 'POST',
      body: { title: 'Second' },
      headers: { 'X-Ontime-Room': spoofed },
    })
  ).status,
  200,
);
check('listed under its real name', (await find(token, second))?.title, 'Second');
check('not under the spoofed one', await find(token, spoofed), undefined);

console.log('== forgetting keeps the data ==');
check('forget', (await call(`/admin/rooms/${room}`, { token, method: 'DELETE' })).status, 200);
check('gone from the list', await find(token, room), undefined);
const project = await (await call(`/r/${room}/data/project`, { token })).json();
check('room data intact', project.title, 'Admin e2e');
check('forget again', (await call(`/admin/rooms/${room}`, { token, method: 'DELETE' })).status, 404);

for (const name of [second, old_room]) {
  await call(`/admin/rooms/${name}`, { token, method: 'DELETE' });
}

process.exit(failed ? 1 : 0);
