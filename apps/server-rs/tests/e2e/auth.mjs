// Verifies per-room password protection: an unprotected room stays open, a
// protected one rejects everything without a token, and rooms cannot see each other.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const room = 'secure-' + Math.random().toString(36).slice(2, 10);
const call = (path, { token, method = 'GET', body } = {}) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });

console.log('== a fresh room is open ==');
check('settings readable', (await call('/data/settings')).status, 200);

console.log('== claiming the room with a password ==');
const claim = await call('/data/session/password', { method: 'POST', body: { password: 'show-secret' } });
check('claim accepted', claim.status, 200);
const { protected: isProtected, token } = await claim.json();
check('reports protected', isProtected, true);
check('returns a token', typeof token === 'string' && token.length > 32, true);

console.log('== now everything requires the token ==');
check('read without token', (await call('/data/settings')).status, 401);
check('read with wrong token', (await call('/data/settings', { token: 'nope' })).status, 401);
check('read with token', (await call('/data/settings', { token })).status, 200);
check('write without token', (await call('/data/project', { method: 'POST', body: { title: 'hax' } })).status, 401);
check('rundown without token', (await call('/data/rundowns')).status, 401);
check('rundown with token', (await call('/data/rundowns', { token })).status, 200);

console.log('== the token also travels as a query param and a cookie ==');
check('query param', (await fetch(`${base}/r/${room}/data/settings?token=${token}`)).status, 200);
check(
  'cookie',
  (await fetch(`${base}/r/${room}/data/settings`, {
    headers: { Cookie: `token=${JSON.stringify({ token, scope: 'rw' })}` },
  })).status,
  200,
);

console.log('== the room cannot be re-claimed without the token ==');
check(
  'reclaim rejected',
  (await call('/data/session/password', { method: 'POST', body: { password: 'attacker' } })).status,
  401,
);

console.log('== the websocket is gated too ==');
const tryWs = (query) =>
  new Promise((resolve) => {
    const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}${query}`);
    const done = (result) => { try { ws.close(); } catch {} resolve(result); };
    ws.addEventListener('message', () => done('connected'));
    ws.addEventListener('error', () => done('rejected'));
    ws.addEventListener('close', () => done('rejected'));
    setTimeout(() => done('timeout'), 6000);
  });
check('without token', await tryWs(''), 'rejected');
check('with token', await tryWs(`&token=${token}`), 'connected');

console.log('== another room is unaffected ==');
check('neighbour still open', (await fetch(`${base}/r/${room}-other/data/settings`)).status, 200);
check('default room still open', (await fetch(`${base}/data/settings`)).status, 200);

console.log('== clearing the password reopens the room ==');
const cleared = await call('/data/session/password', { method: 'POST', token, body: { password: null } });
check('clear accepted', cleared.status, 200);
check('reports unprotected', (await cleared.json()).protected, false);
check('open again', (await call('/data/settings')).status, 200);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
