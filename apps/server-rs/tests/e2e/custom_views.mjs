// A room's custom views (plan 021): single-file pages kept by the room and
// served from /r/<room>/external/<name>/, sandboxed and pointed at the room.
//
//   ADMIN_PASSWORD=admin-e2e node custom_views.mjs http://127.0.0.1:8787
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
const room = `views-${Math.random().toString(36).slice(2, 8)}`;
const call = (method, path, body, token = master) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, headers: r.headers, text: await r.text() }));
const json = (response) => {
  try {
    return JSON.parse(response.text);
  } catch {
    return null;
  }
};
const page = '<!doctype html><html><head><title>Stage</title></head><body><script>new WebSocket(`ws://${location.host}/ws`)</script></body></html>';

try {
  console.log('== the room keeps its pages ==');
  await call('POST', '/data/rundowns', { title: 'Show' });
  check('none at first', json(await call('GET', '/data/custom-views')), { views: [] });
  check('the demo is restored', (await call('POST', '/data/custom-views/restore-demo')).status, 201);
  check('an upload', (await call('POST', '/data/custom-views/stage/upload', { html: page })).status, 201);
  check('listed by name', json(await call('GET', '/data/custom-views')), { views: [{ slug: 'demo' }, { slug: 'stage' }] });
  check('a name is not taken twice', (await call('POST', '/data/custom-views/stage/upload', { html: page })).status, 409);
  check('the demo can be restored again', (await call('POST', '/data/custom-views/restore-demo')).status, 201);
  const download = await call('GET', '/data/custom-views/stage/download');
  check('downloads as uploaded', download.text, page);
  check('as a file', download.headers.get('content-disposition'), 'attachment; filename="stage-index.html"');

  console.log('== what is refused ==');
  const external = '<!doctype html><html><head><script src="https://x.example/a.js"></script></head></html>';
  const refused = await call('POST', '/data/custom-views/bad/upload', { html: external });
  check('an external script', [refused.status, json(refused)?.message], [400, 'External scripts are not allowed. Use inline <script> instead.']);
  check('not html', (await call('POST', '/data/custom-views/bad/upload', { html: 'hello' })).status, 400);
  check('a bad name', (await call('POST', '/data/custom-views/Bad_Name/upload', { html: page })).status, 400);
  check('no page sent', (await call('POST', '/data/custom-views/bad/upload', {})).status, 422);
  check('too large', (await call('POST', '/data/custom-views/big/upload', { html: `<html>${'x'.repeat(1_000_001)}</html>` })).status, 413);
  check('nothing refused was kept', json(await call('GET', '/data/custom-views')).views.length, 2);

  console.log('== a page as it is served ==');
  const served = await call('GET', '/external/stage/', null, null);
  check('served to anyone in an open room', served.status, 200);
  check('as html', served.headers.get('content-type'), 'text/html; charset=utf-8');
  const policy = served.headers.get('content-security-policy') ?? '';
  check('sandboxed', policy.startsWith('sandbox allow-scripts'), true);
  check('without the worker origin', policy.includes('allow-same-origin'), false);
  check('talking only to its worker', policy.includes(`connect-src ${new URL(base).origin}`), true);
  check('the room script first', served.text.indexOf('location.pathname.match') < served.text.indexOf('new WebSocket(`ws'), true);
  check('the page itself after it', served.text.endsWith('</script></body></html>'), true);
  check('index.html too', (await call('GET', '/external/stage/index.html', null, null)).status, 200);
  check('an unknown page', (await call('GET', '/external/nope/', null, null)).status, 404);
  check('a page named like a socket is still a page', (await call('GET', '/external/ws-stage/', null, null)).status, 404);

  console.log('== a locked room ==');
  const lock = json(await call('POST', '/data/session/password', { lock: true }));
  check('locked', lock?.protected, true);
  check('its page needs the token', (await call('GET', '/external/stage/', null, null)).status, 401);
  check('and opens with it', (await call('GET', `/external/stage/?token=${lock.token}`, null, null)).status, 200);
  check('nobody else changes its pages', (await call('DELETE', '/data/custom-views/stage', null, null)).status, 401);

  console.log('== removed, and logged ==');
  check('deleted', (await call('DELETE', '/data/custom-views/stage')).status, 204);
  check('deleting again is no error', (await call('DELETE', '/data/custom-views/stage')).status, 204);
  check('gone', (await call('GET', '/external/stage/', null, lock.token)).status, 404);
  const log = json(await call('GET', '/data/activity'));
  const actions = log.lines.map((line) => line.action);
  check('adds logged', actions.filter((a) => a === 'custom-view-add').length, 3);
  check('the removal logged', actions.filter((a) => a === 'custom-view-remove').length, 1);

  console.log('== the OBS pages (plan 022) ==');
  check('added in one go', (await call('POST', '/data/custom-views/restore-obs')).status, 201);
  const slugs = json(await call('GET', '/data/custom-views')).views.map((v) => v.slug);
  check('both listed', ['obs-countdown', 'obs-timer'].every((s) => slugs.includes(s)), true);
  check('put back, not refused, the second time', (await call('POST', '/data/custom-views/restore-obs')).status, 201);
  const obs = await call('GET', `/external/obs-timer/?token=${lock.token}`, null, null);
  check('served like any page', [obs.status, obs.text.includes('location.pathname.match')], [200, true]);
  check('Google Fonts allowed for a theme', (obs.headers.get('content-security-policy') ?? '').includes('https://fonts.googleapis.com'), true);
} finally {
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
