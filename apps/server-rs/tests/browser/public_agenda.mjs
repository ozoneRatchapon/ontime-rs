// The attendee page (plan 023) on a phone-sized Chromium, with no token: the
// agenda, what is on now and how long is left, the room's chosen field, Thai,
// and the page closing when the room turns its feed off. With the client
// running too (BROWSER_APP), the editor's switch turns the feed on.
//
//   ADMIN_PASSWORD=admin-e2e node public_agenda.mjs http://127.0.0.1:8788
import { launch } from './support.mjs';

const api = process.argv[2] ?? process.env.BROWSER_API ?? 'http://127.0.0.1:8788';
const app = process.env.BROWSER_APP;
const password = process.env.ADMIN_PASSWORD;
if (!password) {
  console.log('  skipped: set ADMIN_PASSWORD');
  process.exit(0);
}

let failed = false;
const check = (label, ok, detail = '') => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> ${detail}`}`);
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
const room = `agenda-${Math.random().toString(36).slice(2, 8)}`;
let token = master;
const call = (method, path, body) =>
  fetch(`${api}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', Authorization: `Bearer ${token}` },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, body: await r.json().catch(() => null) }));

const browser = await launch();
const problems = [];
try {
  const list = (await call('POST', '/data/rundowns', { title: 'Day 1' })).body;
  const rid = list.rundowns.find((r) => r.title === 'Day 1').id;
  await call('POST', `/data/rundowns/${rid}/load`);
  await call('POST', '/data/custom-fields', { label: 'Presenter', type: 'text', colour: '#000000' });
  const add = (entry) => call('POST', `/data/rundowns/${rid}/entry`, { type: 'event', duration: 1800000, ...entry });
  await add({ cue: '1', title: 'Opening', timeStart: 36000000, note: 'secret note', custom: { Presenter: 'Ozone' } });
  await add({ cue: '2', title: 'Keynote', timeStart: 37800000 });
  await call('POST', '/data/project', { title: 'Road to Mainnet' });
  token = (await call('POST', '/data/session/password', { lock: true })).body.token;

  if (app) {
    console.log('== turned on from the editor ==');
    const editor = await browser.newPage();
    await editor.goto(`${app}/editor?room=${room}&token=${token}&settings=network`);
    const toggle = editor.getByRole('switch', { name: 'Attendee page' });
    await toggle.waitFor({ timeout: 20000 });
    await toggle.click();
    const shown = await editor
      .getByText(`/r/${room}/public/agenda/`)
      .first()
      .waitFor({ timeout: 10000 })
      .then(() => true)
      .catch(() => false);
    check('the switch turns it on and shows the link', shown);
    await editor.getByRole('checkbox').first().click();
    await new Promise((resolve) => setTimeout(resolve, 800));
    await editor.close();
  } else {
    await call('POST', '/data/public-feed', { enabled: true, fields: ['Presenter'] });
  }
  const feed = (await call('GET', '/data/public-feed')).body;
  check('on, with the presenter shown', feed.enabled && feed.fields.includes('Presenter'), JSON.stringify(feed));

  console.log('== the page, as an attendee opens it ==');
  const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
  page.on('pageerror', (error) => problems.push(error.message));
  page.on('console', (message) => {
    if (/Content Security Policy|Refused to/i.test(message.text())) problems.push(message.text());
  });
  await page.goto(`${api}/r/${room}/public/agenda/?lang=th&theme=dark`);
  await page.waitForFunction(() => document.querySelectorAll('#list li').length >= 2, null, { timeout: 15000 });
  check('the agenda, in order', (await page.locator('#list .what').allTextContents()).join(' | ') === 'Opening | Keynote');
  check('with the planned times', (await page.locator('#list .time').allTextContents()).join(' ') === '10:00 10:30');
  check('and the presenter', (await page.locator('#list .fields').first().textContent()) === 'Ozone');
  check('the title', (await page.locator('#title').textContent()) === 'Road to Mainnet');
  check('in Thai', (await page.locator('#status').textContent()).includes('ยังไม่เริ่ม'));
  check('no note anywhere', !(await page.content()).includes('secret note'));

  await call('GET', '/api/start/index/1');
  await page.waitForFunction(() => !document.getElementById('now').hidden, null, { timeout: 10000 }).catch(() => {});
  check('what is on now', (await page.locator('#now-title').textContent()) === 'Opening');
  const left = await page.locator('#left').textContent();
  check('and how long is left', /^(30:00|29:5\d)$/.test(left), left);
  await new Promise((resolve) => setTimeout(resolve, 2200));
  check('counting by itself', (await page.locator('#left').textContent()) !== left);
  check('live, in Thai', (await page.locator('#status').textContent()).includes('กำลังไลฟ์'));
  check('next', (await page.locator('#next').textContent()) === 'ถัดไป: Keynote');
  check(
    'whether it runs late',
    /ช้ากว่ากำหนด|เร็วกว่ากำหนด|ตรงเวลา/.test(await page.locator('#status').textContent()),
    await page.locator('#status').textContent(),
  );
  await page.screenshot({ path: '/tmp/public-agenda.png' });

  console.log('== closed by the room ==');
  await call('POST', '/data/public-feed', { enabled: false });
  await page.waitForFunction(() => !document.getElementById('empty').hidden, null, { timeout: 10000 }).catch(() => {});
  check('the page says so', (await page.locator('#empty').textContent()) === 'กำหนดการนี้ยังไม่เปิดให้ดู');
  check('and keeps nothing of the show', (await page.locator('#list li').count()) === 0);
  check('no page errors, nothing refused', problems.length === 0, problems.join(' | '));
  await call('GET', '/api/stop');
} finally {
  await browser.close();
  await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
