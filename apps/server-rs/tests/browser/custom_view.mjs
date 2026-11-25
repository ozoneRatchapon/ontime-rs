// Upstream's demo custom view, unchanged, in a real browser (plan 021): served
// by the Worker under a locked room, sandboxed, it still finds its room and
// shows the running event, because the room script points its socket there.
//
//   ADMIN_PASSWORD=admin-e2e node custom_view.mjs http://127.0.0.1:8787
import { launch } from './support.mjs';

const api = process.argv[2] ?? process.env.BROWSER_API ?? 'http://127.0.0.1:8788';
const password = process.env.ADMIN_PASSWORD;
if (!password) {
  console.log('  skipped: set ADMIN_PASSWORD');
  process.exit(0);
}

let failed = false;
const check = (label, ok, detail = '') => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` ${detail}`}`);
};

const master = (
  await (
    await fetch(`${api}/admin/login`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password }),
    })
  ).json()
).token;
const room = `view-${Math.random().toString(36).slice(2, 8)}`;
let token = master;
const call = (method, path, body) =>
  fetch(`${api}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', Authorization: `Bearer ${token}` },
    body: body ? JSON.stringify(body) : undefined,
  }).then((r) => r.json().catch(() => null));

const browser = await launch();
try {
  const list = await call('POST', '/data/rundowns', { title: 'Show' });
  const rid = list.rundowns.find((r) => r.title === 'Show').id;
  await call('POST', `/data/rundowns/${rid}/entry`, { type: 'event', title: 'Keynote', duration: 600000, cue: '1' });
  await call('POST', `/data/rundowns/${rid}/load`);
  token = (await call('POST', '/data/session/password', { lock: true })).token;
  await call('POST', '/data/custom-views/restore-demo');

  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`${api}/r/${room}/external/demo/?token=${token}`);
  // the integration API counts events from 1, as upstream's does
  const started = await call('GET', '/api/start/index/1');
  check('playback starts', started?.payload === 'success', JSON.stringify(started));

  const shown = await page
    .waitForFunction(() => document.getElementById('eventNow')?.textContent.includes('Keynote'), null, {
      timeout: 15000,
    })
    .then(() => true)
    .catch(() => false);
  check('the demo shows the room’s running event', shown, await page.locator('#eventNow').textContent());
  const timer = await page.locator('#timer').innerText();
  check('and its timer, playing', timer.includes('"playback": "play"'), timer);

  const sandboxed = await page.evaluate(() => {
    try {
      return String(document.cookie) === '' && window.origin === 'null';
    } catch {
      // a sandboxed page may not even read cookies
      return window.origin === 'null';
    }
  });
  check('it runs in an origin of its own', sandboxed);
  check('without page errors', errors.length === 0, errors.join('; '));
  await call('GET', '/api/stop');

  // the editor's Custom views panel, when the client is running too
  const app = process.env.BROWSER_APP;
  if (app) {
    const editor = await browser.newPage();
    await editor.goto(`${app}/editor?room=${room}&token=${token}&settings=settings__custom-views`);
    await editor.getByText('Custom views', { exact: true }).first().waitFor({ timeout: 20000 });
    check('the panel lists the demo', await editor.getByRole('cell', { name: 'demo', exact: true }).isVisible());
    await editor.getByRole('button', { name: 'New' }).click();
    await editor.locator('input[type=file]').last().setInputFiles({
      name: 'index.html',
      mimeType: 'text/html',
      buffer: Buffer.from('<!doctype html><html><head></head><body>Stage</body></html>'),
    });
    await editor.getByPlaceholder('my-view').fill('stage');
    await editor.getByRole('button', { name: 'Upload view' }).click();
    const row = editor.getByRole('cell', { name: 'stage', exact: true });
    const listed = await row
      .waitFor({ timeout: 10000 })
      .then(() => true)
      .catch(() => false);
    check('a page uploaded from the panel is listed', listed);
    const address = await editor.getByText(`/r/${room}/external/stage/`).first().textContent().catch(() => '');
    check('with its address on the Worker, token and all', address.includes(`token=${token}`), address);
    const stored = await call('GET', '/data/custom-views');
    check('and kept by the room', stored.views.some((v) => v.slug === 'stage'), JSON.stringify(stored));
  }
} finally {
  await browser.close();
  await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
