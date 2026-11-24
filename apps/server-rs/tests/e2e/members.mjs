// Phase 2: operators, owners managing their team, and members-only rooms.
// Over REST and over the websocket, because a socket is checked once at connect.
//
// Like admin.mjs it needs a worker with a master password:
//
//   npx wrangler dev --port 8788 --local --var SESSION_PASSWORD:admin-e2e
//   ADMIN_PASSWORD=admin-e2e node members.mjs http://127.0.0.1:8788
//
// Takes a little over a minute: it waits out the room's 60 s membership cache
// to prove a removed operator loses control. Cleans up after itself.
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
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
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
const room = `members-${suffix}`;
const r = (path) => `/r/${room}${path}`;

const { token: master } = await json(await call('/admin/login', { method: 'POST', body: { password } }));

// --- people --------------------------------------------------------------
const redeem = async (code) => (await json(await call('/auth/redeem', { method: 'POST', body: { code } }))).token;
const owner = await json(
  await call('/admin/people', { token: master, method: 'POST', body: { name: `Owner ${suffix}` } }),
);
const owner_invite = await json(await call(`/admin/people/${owner.id}/invite`, { token: master, method: 'POST' }));
const owner_session = await redeem(owner_invite.code);

// --- a room with something to run ----------------------------------------
await call(r('/data/project'), { token: master, method: 'POST', body: { title: 'Members e2e' } });
const created = await json(await call(r('/data/rundowns'), { token: master, method: 'POST', body: { title: 'Run' } }));
const rundown = created.rundowns.find((x) => x.title === 'Run').id;
await call(r(`/data/rundowns/${rundown}/entry`), {
  token: master,
  method: 'POST',
  body: { type: 'event', cue: '1', title: 'One', duration: 600000, timeStart: 32400000 },
});
await call(r(`/data/rundowns/${rundown}/load`), { token: master, method: 'POST' });
check(
  'admin assigns the owner',
  (await call(`/admin/rooms/${room}`, { token: master, method: 'PUT', body: { owners: [owner.id] } })).status,
  200,
);

console.log('== an owner builds their team ==');
check('team needs a session', (await call(`/auth/rooms/${room}/team`)).status, 401);
const made = await call(`/auth/rooms/${room}/operators`, {
  token: owner_session,
  method: 'POST',
  body: { name: `Op ${suffix}` },
});
check('owner adds an operator', made.status, 200);
const { person: operator, code: operator_code } = await json(made);
const operator_session = await redeem(operator_code);
check('the operator signs in', typeof operator_session, 'string');
const me = await json(await call('/auth/me', { token: operator_session }));
check(
  'the operator sees the room as operator',
  me.rooms.map((x) => [x.name, x.role]),
  [[room, 'operator']],
);
check(
  'an operator cannot add people',
  (await call(`/auth/rooms/${room}/operators`, { token: operator_session, method: 'POST', body: { name: 'x' } }))
    .status,
  403,
);
const team = await json(await call(`/auth/rooms/${room}/team`, { token: owner_session }));
check('team lists both', [team.owners.length, team.operators.length], [1, 1]);
check(
  'the room check is not reachable from outside',
  (await call('/auth/check', { method: 'POST', body: { token: operator_session, room } })).status,
  404,
);

console.log('== with the switch off nothing changes ==');
check(
  'anyone may edit an open room',
  (await call(r('/data/project'), { method: 'POST', body: { title: 'Members e2e' } })).status,
  200,
);

console.log('== only an owner (or admin) flips the switch ==');
const flip = (on, who = {}) => call(r('/data/members-only'), { method: 'POST', body: { on }, ...who });
check('anonymous', (await flip(true)).status, 403);
check('operator', (await flip(true, { session: operator_session })).status, 403);
check('owner', (await flip(true, { session: owner_session })).status, 200);
const listed = (await json(await call('/admin/rooms', { token: master }))).rooms.find((x) => x.name === room);
check('the admin list shows it', listed?.membersOnly, true);

console.log('== members-only over REST ==');
check('anyone may still read', (await call(r('/data/rundowns'))).status, 200);
const refused = await call(r('/data/project'), { method: 'POST', body: { title: 'Hijacked' } });
check('an anonymous edit is refused', refused.status, 403);
check('with a clear reason', (await json(refused)).message.includes('sign-in'), true);
check(
  'a made-up session is refused',
  (await call(r('/data/project'), { session: 'nope', method: 'POST', body: { title: 'x' } })).status,
  403,
);
check(
  'the operator may edit',
  (await call(r('/data/project'), { session: operator_session, method: 'POST', body: { title: 'Members e2e' } }))
    .status,
  200,
);
check(
  'the master may edit',
  (await call(r('/data/project'), { token: master, method: 'POST', body: { title: 'Members e2e' } })).status,
  200,
);
const info = async (who) => json(await call(r('/data/session/info'), who));
check('info: anonymous cannot edit', (await info({})).canEdit, false);
check('info: operator can', (await info({ session: operator_session })).canEdit, true);

console.log('== members-only over the websocket ==');
const socket = (session) =>
  new Promise((resolve, reject) => {
    const query = new URLSearchParams({ room, ...(session ? { session } : {}) });
    const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?${query}`);
    const packets = [];
    ws.addEventListener('message', (event) => packets.push(JSON.parse(event.data)));
    ws.addEventListener('open', () =>
      resolve({ ws, packets, send: (tag, payload) => ws.send(JSON.stringify({ tag, payload })) }),
    );
    ws.addEventListener('error', () => reject(new Error('socket error')));
  });
const playback = (packets) => packets.filter((p) => p.tag === 'runtime-data').at(-1)?.payload?.timer?.playback;

const viewer = await socket();
await wait(300);
const before = playback(viewer.packets);
check('a viewer may watch', typeof before, 'string');
viewer.send('start', undefined);
await wait(800);
check('but its start is ignored', [playback(viewer.packets), before === 'play'], [before, false]);
check(
  'and it is told why',
  viewer.packets.some((p) => p.tag === 'log' && p.payload.text.includes('sign-in')),
  true,
);

const op_socket = await socket(operator_session);
await wait(300);
op_socket.send('start', undefined);
await wait(800);
check("the operator's start runs", playback(op_socket.packets), 'play');
check('and the viewer sees it', playback(viewer.packets), 'play');
op_socket.send('stop', undefined);
await wait(500);
viewer.ws.close();
op_socket.ws.close();

console.log('== a member gets past a room password ==');
check(
  'the admin sets a room password',
  (await call(r('/data/session/password'), { token: master, method: 'POST', body: { password: `pw-${suffix}` } }))
    .status,
  200,
);
check('anonymous cannot even read now', (await call(r('/data/rundowns'))).status, 401);
check(
  'the operator reads without the password',
  (await call(r('/data/rundowns'), { session: operator_session })).status,
  200,
);
check(
  'and edits',
  (await call(r('/data/project'), { session: operator_session, method: 'POST', body: { title: 'Members e2e' } }))
    .status,
  200,
);
await call(r('/data/session/password'), { token: master, method: 'POST', body: { password: null } });

console.log('== removing an operator ==');
check(
  'the owner removes them',
  (
    await call(`/auth/rooms/${room}/operators/${operator.id}`, {
      session: undefined,
      token: owner_session,
      method: 'DELETE',
    })
  ).status,
  200,
);
check(
  'they are signed out (invited here, in no room now)',
  (await call('/auth/me', { token: operator_session })).status,
  401,
);
console.log("  … waiting out the room's 60 s membership cache");
await wait(61_000);
check(
  'and can no longer edit',
  (await call(r('/data/project'), { session: operator_session, method: 'POST', body: { title: 'x' } })).status,
  403,
);

console.log('== cleanup ==');
check('switch off', (await flip(false, { session: owner_session })).status, 200);
check(
  'anyone may edit again',
  (await call(r('/data/project'), { method: 'POST', body: { title: 'Members e2e' } })).status,
  200,
);
await call(`/admin/people/${owner.id}`, { token: master, method: 'DELETE' });
await call(`/admin/rooms/${room}`, { token: master, method: 'DELETE' });
check(
  'no people left behind',
  (await json(await call('/admin/people', { token: master }))).people.filter((p) => p.name.endsWith(suffix)).length,
  0,
);

process.exit(failed ? 1 : 0);
