// People and invite links: the admin adds a person and makes them a link,
// opening it signs a device in, and that person sees the rooms they own.
//
// Like admin.mjs it needs a worker with a master password:
//
//   npx wrangler dev --port 8788 --local --var SESSION_PASSWORD:admin-e2e
//   ADMIN_PASSWORD=admin-e2e node people.mjs http://127.0.0.1:8788
//
// Every person and room it makes is removed again at the end.

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

const suffix = Math.random().toString(36).slice(2, 10);
const room = `people-${suffix}`;
const iphone =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Version/18.0 Mobile/15E148 Safari/604.1';

const { token: master } = await (await call('/admin/login', { method: 'POST', body: { password } })).json();

console.log('== the admin side is behind the master password ==');
check('people without a token', (await call('/admin/people')).status, 401);
check('people with the token', (await call('/admin/people', { token: master })).status, 200);

console.log('== adding a person and an invite ==');
check(
  'a person needs a name',
  (await call('/admin/people', { token: master, method: 'POST', body: { name: ' ' } })).status,
  400,
);
const person = await (
  await call('/admin/people', { token: master, method: 'POST', body: { name: `สมศรี ${suffix}` } })
).json();
check('created', typeof person.id, 'string');
const first = await (await call(`/admin/people/${person.id}/invite`, { token: master, method: 'POST' })).json();
const invite = await (await call(`/admin/people/${person.id}/invite`, { token: master, method: 'POST' })).json();
check('expires in 7 days', Math.round((invite.expiresAt - Date.now()) / 86_400_000), 7);
check(
  'an unknown person has no invite',
  (await call('/admin/people/not-a-person/invite', { token: master, method: 'POST' })).status,
  404,
);

const listed = async () =>
  (await (await call('/admin/people', { token: master })).json()).people.find((p) => p.id === person.id);
check('listed with a pending invite', (await listed())?.inviteExpiresAt, invite.expiresAt);
check('no hash leaks into the list', JSON.stringify(await listed()).includes('Hash'), false);

console.log('== opening the link ==');
check(
  'the replaced link fails',
  (await call('/auth/redeem', { method: 'POST', body: { code: first.code } })).status,
  401,
);
check('a made-up code fails', (await call('/auth/redeem', { method: 'POST', body: { code: 'guess' } })).status, 401);
const redeemed = await call('/auth/redeem', {
  method: 'POST',
  body: { code: invite.code },
  headers: { 'User-Agent': iphone },
});
check('the live link works', redeemed.status, 200);
check('not cacheable', redeemed.headers.get('cache-control'), 'no-store');
const { token: session, person: who } = await redeemed.json();
check('as the right person', who.id, person.id);
check('only once', (await call('/auth/redeem', { method: 'POST', body: { code: invite.code } })).status, 401);
const seen = await listed();
check('device recorded', seen?.devices, ['iPhone Safari']);
check('invite used up', seen?.inviteExpiresAt, null);

console.log('== who am I ==');
check('me without a session', (await call('/auth/me')).status, 401);
check('me with the master token', (await call('/auth/me', { token: master })).status, 401);
const me = await (await call('/auth/me', { token: session })).json();
check('name', me.person.name, `สมศรี ${suffix}`);
check('owns nothing yet', me.rooms, []);
check('a session is not an admin', (await call('/admin/people', { token: session })).status, 401);

console.log('== assigning an owner ==');
check(
  'a stranger cannot own a room',
  (
    await call(`/admin/rooms/${room}`, {
      token: master,
      method: 'PUT',
      body: { owners: ['0190a000-0000-7000-8000-000000000000'] },
    })
  ).status,
  400,
);
const assigned = await call(`/admin/rooms/${room}`, {
  token: master,
  method: 'PUT',
  body: { owners: [person.id], note: 'e2e' },
});
check('assigned', assigned.status, 200);
const record = await assigned.json();
check('owners on the room', record.owners, [person.id]);
check('note saved with it', record.note, 'e2e');
const mine = (await (await call('/auth/me', { token: session })).json()).rooms;
check(
  'the owner sees the room',
  mine.map((r) => r.name),
  [room],
);

console.log('== signing out ==');
const second_invite = await (await call(`/admin/people/${person.id}/invite`, { token: master, method: 'POST' })).json();
const { token: laptop } = await (
  await call('/auth/redeem', { method: 'POST', body: { code: second_invite.code } })
).json();
check('logout', (await call('/auth/logout', { token: session, method: 'POST' })).status, 200);
check('that device is out', (await call('/auth/me', { token: session })).status, 401);
check('the other is still in', (await call('/auth/me', { token: laptop })).status, 200);
check(
  'sign out everywhere',
  (await call(`/admin/people/${person.id}/sessions`, { token: master, method: 'DELETE' })).status,
  200,
);
check('the other is out too', (await call('/auth/me', { token: laptop })).status, 401);

console.log('== removing the person ==');
check('remove', (await call(`/admin/people/${person.id}`, { token: master, method: 'DELETE' })).status, 200);
check('gone', await listed(), undefined);
const rooms = (await (await call('/admin/rooms', { token: master })).json()).rooms;
check('off the room', rooms.find((r) => r.name === room)?.owners, []);
check('remove again', (await call(`/admin/people/${person.id}`, { token: master, method: 'DELETE' })).status, 404);

await call(`/admin/rooms/${room}`, { token: master, method: 'DELETE' });
process.exit(failed ? 1 : 0);
