// What a room shows its audience without a token (plan 023), from outside:
// off until turned on, never a note, a skipped event or the token, a public
// socket that can only listen, and closed the moment the room turns it off.
//
//   ADMIN_PASSWORD=admin-e2e node public_feed.mjs http://127.0.0.1:8787
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
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const master = (
  await (
    await fetch(`${base}/admin/login`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password }),
    })
  ).json()
).token;
const room = `public-${Math.random().toString(36).slice(2, 8)}`;
let token = master;
const call = (method, path, body, auth = token) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(auth ? { Authorization: `Bearer ${auth}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, headers: r.headers, text: await r.text() }));
const json = (r) => {
  try {
    return JSON.parse(r.text);
  } catch {
    return null;
  }
};

/** A public socket, and everything it hears. */
function listen() {
  const heard = [];
  let closed = null;
  const socket = new WebSocket(`${base.replace(/^http/, 'ws')}/public/ws?room=${room}`);
  socket.onmessage = (event) => heard.push(JSON.parse(event.data));
  socket.onclose = (event) => (closed = { code: event.code, reason: event.reason });
  const opened = new Promise((resolve) => {
    socket.onopen = () => resolve(true);
    socket.onerror = () => resolve(false);
  });
  return { socket, heard, opened, closed: () => closed };
}
const until = async (test, ms = 5000) => {
  for (let waited = 0; waited < ms; waited += 100) {
    if (test()) return true;
    await wait(100);
  }
  return false;
};

try {
  console.log('== a room with a show ==');
  const list = json(await call('POST', '/data/rundowns', { title: 'Day 1' }));
  const rid = list.rundowns.find((r) => r.title === 'Day 1').id;
  await call('POST', `/data/rundowns/${rid}/load`);
  await call('POST', '/data/custom-fields', { label: 'Presenter', type: 'text', colour: '#000000' });
  await call('POST', '/data/custom-fields', { label: 'Backstage', type: 'text', colour: '#000000' });
  const add = (entry) => call('POST', `/data/rundowns/${rid}/entry`, { type: 'event', duration: 1800000, ...entry });
  await add({ id: 'a', cue: '1', title: 'Opening', timeStart: 36000000, note: 'secret note', custom: { Presenter: 'Ozone', Backstage: 'crew only' } });
  await add({ id: 'b', cue: '2', title: 'Keynote', timeStart: 37800000, note: 'mic 3' });
  await add({ id: 's', cue: '3', title: 'Cancelled talk', timeStart: 39600000, skip: true });
  await call('POST', '/data/project', { title: 'Road to Mainnet' });
  token = json(await call('POST', '/data/session/password', { lock: true })).token;

  console.log('== off until the room turns it on ==');
  check('no feed', (await call('GET', '/public', null, null)).status, 404);
  check('no page', (await call('GET', '/public/agenda/', null, null)).status, 404);
  const early = listen();
  check('no socket', await early.opened, false);
  check('reads as off', json(await call('GET', '/data/public-feed')), { enabled: false, fields: [] });
  check('not turned on by a stranger', (await call('POST', '/data/public-feed', { enabled: true }, null)).status, 401);
  check('only real custom fields', (await call('POST', '/data/public-feed', { enabled: true, fields: ['Nope'] })).status, 400);
  const on = await call('POST', '/data/public-feed', { enabled: true, fields: ['Presenter'] });
  check('turned on by the room', [on.status, json(on)], [200, { enabled: true, fields: ['Presenter'] }]);

  console.log('== what anyone may read ==');
  const feed = await call('GET', '/public', null, null);
  check('readable without a token', feed.status, 200);
  check('from any site', feed.headers.get('access-control-allow-origin'), '*');
  const body = json(feed);
  check('the agenda, without the skipped event', body.agenda.entries.map((e) => e.title), ['Opening', 'Keynote']);
  check('the named field, by its label', body.agenda.entries[0].fields, { Presenter: 'Ozone' });
  check('the project title', body.agenda.title, 'Road to Mainnet');
  for (const secret of ['secret note', 'mic 3', 'crew only', 'Cancelled talk', token]) {
    check(`never "${secret.slice(0, 12)}"`, feed.text.includes(secret), false);
  }
  check('and the room itself still needs its token', (await call('GET', '/data/rundowns', null, null)).status, 401);
  const page = await call('GET', '/public/agenda/', null, null);
  check('the attendee page', [page.status, (page.headers.get('content-security-policy') ?? '').startsWith('sandbox allow-scripts')], [200, true]);

  console.log('== the audience socket only listens ==');
  const audience = listen();
  check('opens without a token', await audience.opened, true);
  check('hears the agenda first', await until(() => audience.heard.some((m) => m.payload?.agenda)), true);
  for (const tag of ['start', 'stop', 'addtime']) audience.socket.send(JSON.stringify({ tag, payload: tag === 'addtime' ? 60000 : undefined }));
  audience.socket.send(JSON.stringify({ tag: 'client-set', payload: { name: 'intruder' } }));
  await wait(800);
  check('what it sends changes nothing', json(await call('GET', '/public', null, null)).live.playback, 'stop');
  const clients = json(await call('GET', '/data/clients'));
  check('and it is not one of the room’s clients', clients.clients.length, 0);

  console.log('== it hears the show ==');
  audience.heard.length = 0;
  await call('GET', '/api/start/index/1');
  check('playback reaches it', await until(() => audience.heard.some((m) => m.payload?.live?.playback === 'play')), true);
  check('with what is on now', audience.heard.find((m) => m.payload?.live)?.payload.live.now, 'a');
  audience.heard.length = 0;
  check('the edit is taken', (await call('PUT', `/data/rundowns/${rid}/entry`, { id: 'b', title: 'Keynote: Firedancer' })).status, 200);
  check('an edit reaches it', await until(() => audience.heard.some((m) => m.payload?.agenda?.entries?.some((e) => e.title === 'Keynote: Firedancer'))), true);
  const quiet = audience.heard.length;
  await wait(2500);
  check('a tick alone sends nothing', audience.heard.length, quiet);

  console.log('== turned off ==');
  await call('POST', '/data/public-feed', { enabled: false });
  check('the audience is let go', await until(() => audience.closed() !== null), true);
  check('with a reason', audience.closed()?.reason, 'This room closed its public page');
  check('and nothing is readable', (await call('GET', '/public', null, null)).status, 404);
  // the activity log is for an owner or the admin
  const log = json(await call('GET', '/data/activity', null, master));
  check('both changes logged', log.lines.filter((l) => l.action === 'public-feed').length, 2);
} finally {
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
