// Plan 007 on the native host: it records the same activity as the Worker.
// The room password and the admin each make changes, a socket command is
// recorded, paging works, and only the admin may read.
//
//   PORT=4299 ONTIME_DATA_DIR=/tmp/native-audit SESSION_PASSWORD=native-e2e cargo run
//   MASTER_TOKEN=<blake3 hex of "ontime-room-v1:native-e2e"> node tests/activity.mjs
//
// The master token is `hash_password` in ontime-core. One way to get it:
//   node -e 'console.log(require("<node_modules>/blake3-wasm").hash("ontime-room-v1:native-e2e").toString("hex"))'
const base = process.argv[2] ?? 'http://127.0.0.1:4299';
let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};
const call = (path, { token, method = 'GET', body } = {}) =>
  fetch(`${base}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
const room = `nat-${Math.random().toString(36).slice(2, 8)}`;
const r = (p) => `/r/${room}${p}`;
const master = process.env.MASTER_TOKEN;

check(
  'the admin creates the room',
  (await call(r('/data/project'), { token: master, method: 'POST', body: { title: 'Native' } })).status,
  200,
);
const created = await (
  await call(r('/data/rundowns'), { token: master, method: 'POST', body: { title: 'Run' } })
).json();
const rundown = created.rundowns.find((x) => x.title === 'Run').id;
await call(r(`/data/rundowns/${rundown}/entry`), {
  token: master,
  method: 'POST',
  body: { id: 'e1', type: 'event', cue: '1', title: 'Opening', duration: 60000 },
});
await call(r(`/data/rundowns/${rundown}/load`), { token: master, method: 'POST' });
const { token: room_token } = await (
  await call(r('/data/session/password'), { token: master, method: 'POST', body: { password: 'pw' } })
).json();
check(
  'the room password edits',
  (
    await call(r(`/data/rundowns/${rundown}/entry`), {
      token: room_token,
      method: 'PUT',
      body: { id: 'e1', note: 'secret-value' },
    })
  ).status,
  200,
);

const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}&token=${room_token}`);
await new Promise((res, rej) => {
  ws.onopen = res;
  ws.onerror = () => rej(new Error('socket'));
});
ws.send(JSON.stringify({ tag: 'start', payload: undefined }));
await new Promise((res) => setTimeout(res, 500));
ws.close();

check('the room password cannot read', (await call(r('/data/activity'), { token: room_token })).status, 403);
const response = await call(r('/data/activity'), { token: master });
check('the admin can', response.status, 200);
check('not cached', response.headers.get('cache-control'), 'no-store');
const page = await response.json();
check(
  'newest first, attributed',
  page.lines.map((l) => [l.action, l.actor.kind]),
  [
    ['start', 'roomPassword'],
    ['entry-edit', 'roomPassword'],
    ['password-set', 'admin'],
    ['rundown-load', 'admin'],
    ['entry-add', 'admin'],
    ['rundown-create', 'admin'],
    ['project-data', 'admin'],
  ],
);
check('start names the event', page.lines[0].subject, { cue: '1', title: 'Opening' });
check('no values', JSON.stringify(page).includes('secret-value'), false);
const first = await (await call(r('/data/activity?limit=3'), { token: master })).json();
const rest = await (await call(r(`/data/activity?before=${first.before}`), { token: master })).json();
check('paging covers it all', first.lines.length + rest.lines.length, page.lines.length);
console.log(`room=${room}`);
process.exit(failed ? 1 : 0);
