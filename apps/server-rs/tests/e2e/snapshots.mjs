// The room's own undo history (plan 018): a copy before every change that
// removes things (and every ten minutes of ordinary edits), a list of them,
// and a restore that can itself be undone.
//
//   ADMIN_PASSWORD=admin-e2e node snapshots.mjs http://127.0.0.1:8788
//
// The undo history is the Worker's; the native server answers 404 for it.
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

const login = await fetch(`${base}/admin/login`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ password }),
});
const master = (await login.json()).token;
const room = `undo-${Math.random().toString(36).slice(2, 8)}`;
const call = (method, path, body, token = master) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, body: await r.json().catch(() => null) }));
const entries = async (rid) => Object.keys((await call('GET', `/data/rundowns/${rid}`)).body.entries).sort();

try {
  console.log('== a room with three events ==');
  const created = await call('POST', '/data/rundowns', { title: 'Undo' });
  const rid = created.body.rundowns.find((r) => r.title === 'Undo').id;
  for (const id of ['a', 'b', 'c']) {
    await call('POST', `/data/rundowns/${rid}/entry`, { id, type: 'event', duration: 600000 });
  }
  check('three events', await entries(rid), ['a', 'b', 'c']);

  console.log('== who may see the history ==');
  // an open room lets anyone in, but undoing is for an owner or the admin
  check('not anyone', (await call('GET', '/data/snapshots', null, null)).status, 403);
  check('the admin', (await call('GET', '/data/snapshots')).status, 200);

  console.log('== a mistake, undone ==');
  await call('PUT', `/data/rundowns/${rid}`, { entries: [] }); // the whole rundown replaced by mistake
  check('the events are gone', await entries(rid), []);
  const history = (await call('GET', '/data/snapshots')).body.snapshots;
  check('the replace was copied first', history.length >= 1, true);
  const undo = await call('POST', `/data/snapshots/${history[0]}/restore`);
  check('restore answers', undo.status, 200);
  check('the events are back, exactly as before the mistake', await entries(rid), ['a', 'b', 'c']);

  console.log('== and the undo can be undone ==');
  const after = (await call('GET', '/data/snapshots')).body.snapshots;
  check('the restore kept the state it replaced', after.length, history.length + 1);
  await call('POST', `/data/snapshots/${after[0]}/restore`);
  check('restoring that brings the mistake back', await entries(rid), []);
  check('an unknown copy', (await call('POST', '/data/snapshots/123/restore')).status, 404);

  console.log('== it is in the activity log ==');
  const log = (await call('GET', '/data/activity')).body;
  check(
    'logged as a restore',
    log.lines.some((line) => line.action === 'restore'),
    true,
  );
} finally {
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
