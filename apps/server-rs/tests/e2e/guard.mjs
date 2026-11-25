// The guessing guard (plan 019): wrong guesses at the admin password, the
// master token and a room token are counted per client; past the free ten,
// the client waits, and while it waits even the right answer is refused.
//
// Needs a worker started with a master password, and that password here:
//
//   npx wrangler dev --port 8788 --local --var SESSION_PASSWORD:admin-e2e
//   ADMIN_PASSWORD=admin-e2e node guard.mjs http://127.0.0.1:8788
//
// Each check pretends to be its own client with `CF-Connecting-IP`, which a
// local worker takes as sent. On a deployment Cloudflare overwrites it, so
// against one this suite would lock the machine running it out instead: it
// refuses to run there.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';
const password = process.env.ADMIN_PASSWORD;
if (!password) {
  console.log("  skipped: set ADMIN_PASSWORD to the worker's SESSION_PASSWORD to run it");
  process.exit(0);
}
if (!/^http:\/\/(127\.0\.0\.1|localhost)[:/]/.test(`${base}/`)) {
  console.log('  skipped: only against a local worker, which takes CF-Connecting-IP as sent');
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
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const FREE = 10;

// a fresh TEST-NET-3 address per client, so runs and checks never share a count
const client = () => `203.0.113.${1 + Math.floor(Math.random() * 254)}`;
let used = new Set();
const fresh = () => {
  let ip = client();
  while (used.has(ip)) ip = client();
  used.add(ip);
  return ip;
};

const call = (path, { ip, token, method = 'GET', body } = {}) =>
  fetch(`${base}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(ip ? { 'CF-Connecting-IP': ip } : {}),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });

const login = (ip, guess) => call('/admin/login', { ip, method: 'POST', body: { password: guess } });

console.log('== the admin password ==');
{
  const ip = fresh();
  const statuses = [];
  for (let n = 0; n <= FREE; n++) statuses.push((await login(ip, `guess-${n}`)).status);
  check('the free ten and the one that locks are answered', statuses, Array(FREE + 1).fill(401));
  const locked = await login(ip, password);
  check('the right password is refused while locked', locked.status, 429);
  check('with a Retry-After', locked.headers.get('Retry-After'), '1');
  check('another client is not affected', (await login(fresh(), password)).status, 200);
  await wait(1100);
  check('let in once the wait is over', (await login(ip, password)).status, 200);
}

const master = await (await login(fresh(), password)).json().then((body) => body.token);

console.log('== the master token at an admin route ==');
{
  const ip = fresh();
  for (let n = 0; n <= FREE; n++) await call('/admin/rooms', { ip, token: `guess-${n}` });
  check('the master token is refused while locked', (await call('/admin/rooms', { ip, token: master })).status, 429);
  check('asking with no token guesses nothing', (await call('/admin/rooms', { ip })).status, 401);
}

console.log('== a room token ==');
const room = 'guard-' + Math.random().toString(36).slice(2, 10);
const claim = await call(`/r/${room}/data/session/password`, {
  ip: fresh(),
  token: master,
  method: 'POST',
  body: { password: 'room-secret' },
});
check('room claimed', claim.status, 200);
const { token } = await claim.json();
{
  const ip = fresh();
  for (let n = 0; n <= FREE; n++) await call(`/r/${room}/data/settings`, { ip, token: `guess-${n}` });
  const locked = await call(`/r/${room}/data/settings`, { ip, token });
  check('the room token is refused while locked', locked.status, 429);
  check('no token is still just a 401', (await call(`/r/${room}/data/settings`, { ip })).status, 401);
  check('the integration API is behind the same door', (await call(`/r/${room}/api/version`, { ip, token })).status, 429);
  check('another client is let in', (await call(`/r/${room}/data/settings`, { ip: fresh(), token })).status, 200);
}
{
  const ip = fresh();
  for (let n = 0; n < 3 * FREE; n++) await call(`/r/${room}/data/settings`, { ip, token });
  check('right answers are never counted', (await call(`/r/${room}/data/settings`, { ip, token })).status, 200);
}
{
  // screens left open after the password changed, behind one venue address
  const ip = fresh();
  for (let n = 0; n < 3 * FREE; n++) await call(`/r/${room}/data/settings`, { ip, token: 'last-weeks-token' });
  check('a stale token retried is counted once', (await call(`/r/${room}/data/settings`, { ip, token })).status, 200);
}

console.log('== an open room ==');
const open = 'guard-open-' + Math.random().toString(36).slice(2, 10);
check(
  'room created',
  (await call(`/r/${open}/data/project`, { ip: fresh(), token: master, method: 'POST', body: { title: 'open' } }))
    .status,
  200,
);
{
  const ip = fresh();
  for (let n = 0; n < 3 * FREE; n++) await call(`/r/${open}/data/settings`, { ip, token: 'stale-token' });
  check('a stale token that is let in is not counted', (await call(`/r/${open}/data/settings`, { ip, token: 'stale' })).status, 200);
  for (let n = 0; n <= FREE; n++) await call(`/r/${open}/data/activity`, { ip, token: `guess-${n}` });
  check('guessing the master token at an admin-only route is', (await call(`/r/${open}/data/activity`, { ip, token: master })).status, 429);
}

process.exit(failed ? 1 : 0);
