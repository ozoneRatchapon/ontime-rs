// The OBS pages (plan 022) in a real browser, under a locked room: the
// countdown keeps counting while the room is idle and sends nothing, takes its
// title and its theme from the address and the project, and the timer shows
// the running event, pauses, goes over time, and hides when nothing plays.
//
//   ADMIN_PASSWORD=admin-e2e node obs_views.mjs http://127.0.0.1:8788
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
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> ${detail}`}`);
};
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const master = (
  await (
    await fetch(`${api}/admin/login`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password }),
    })
  ).json()
).token;
const room = `obs-${Math.random().toString(36).slice(2, 8)}`;
let token = master;
const call = (method, path, body) =>
  fetch(`${api}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', Authorization: `Bearer ${token}` },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, body: await r.json().catch(() => null) }));

const browser = await launch();
const problems = [];
const open = async (path) => {
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  page.on('pageerror', (error) => problems.push(`${path}: ${error.message}`));
  page.on('console', (message) => {
    if (/Content Security Policy|Refused to/i.test(message.text())) problems.push(`${path}: ${message.text()}`);
  });
  await page.goto(`${api}/r/${room}/external/${path}${path.includes('?') ? '&' : '?'}token=${token}`);
  return page;
};

try {
  const list = (await call('POST', '/data/rundowns', { title: 'Show' })).body;
  const rid = list.rundowns.find((r) => r.title === 'Show').id;
  await call('POST', `/data/rundowns/${rid}/entry`, { type: 'event', title: 'Keynote', cue: '1', timeStart: 36000000, duration: 600000 });
  await call('POST', `/data/rundowns/${rid}/entry`, { type: 'event', title: 'Short', cue: '2', timeStart: 36600000, duration: 3000 });
  await call('POST', `/data/rundowns/${rid}/load`);
  await call('POST', '/data/project', { title: 'Gala 2026' });
  token = (await call('POST', '/data/session/password', { lock: true })).body.token;

  console.log('== added in one go ==');
  check('restore-obs answers', (await call('POST', '/data/custom-views/restore-obs')).status === 201);
  const views = (await call('GET', '/data/custom-views')).body.views.map((v) => v.slug);
  check('both pages listed', views.includes('obs-countdown') && views.includes('obs-timer'), JSON.stringify(views));
  check('again, they are put back, not refused', (await call('POST', '/data/custom-views/restore-obs')).status === 201);

  console.log('== the countdown ==');
  // three minutes from the room's clock (UTC on a fresh Worker), to the minute
  const inThree = new Date(Date.now() + 3 * 60000);
  const to = `${String(inThree.getUTCHours()).padStart(2, '0')}:${String(inThree.getUTCMinutes()).padStart(2, '0')}`;
  const countdown = await open(`obs-countdown/?to=${to}&accent=ff0000&label=เริ่มในอีก&font=Prompt`);
  await countdown.waitForFunction(() => /^\d\d:\d\d$/.test(document.getElementById('count').textContent), null, { timeout: 15000 });
  const first = await countdown.locator('#count').textContent();
  check('counts down to the time in the address', /^0[23]:\d\d$/.test(first), first);
  await wait(2200);
  const later = await countdown.locator('#count').textContent();
  check('and keeps counting while the room sends nothing', later !== first, `${first} then ${later}`);
  await countdown.waitForFunction(() => document.getElementById('title').textContent === 'Gala 2026', null, { timeout: 10000 }).catch(() => {});
  check('the project title, read through the room', (await countdown.locator('#title').textContent()) === 'Gala 2026');
  check('the label from the address, in Thai', (await countdown.locator('#label').textContent()) === 'เริ่มในอีก');
  const accent = await countdown.locator('#count').evaluate((el) => getComputedStyle(el).color);
  check('the accent colour from the address', accent === 'rgb(255, 0, 0)', accent);
  const see_through = await countdown.evaluate(() => getComputedStyle(document.body).backgroundColor);
  check('see-through by default', see_through === 'rgba(0, 0, 0, 0)', see_through);
  check('the first event is named', (await countdown.locator('#next').textContent()) === 'Keynote');

  console.log('== the timer ==');
  const timer = await open('obs-timer/?pos=tl');
  await wait(1500);
  check('hidden while nothing plays', await timer.locator('#box').evaluate((el) => el.classList.contains('hidden')));
  check('in the corner asked for', await timer.locator('#box').evaluate((el) => el.classList.contains('tl')));
  await call('GET', '/api/start/index/1');
  await timer.waitForFunction(() => !document.getElementById('box').classList.contains('hidden'), null, { timeout: 10000 });
  const running = await timer.locator('#time').textContent();
  check('shows the running event', /^(10:00|09:5\d)$/.test(running), running);
  check('with its title', (await timer.locator('#title').textContent()) === 'Keynote');
  await wait(2200);
  const runningLater = await timer.locator('#time').textContent();
  check('counting down', runningLater !== running, `${running} then ${runningLater}`);
  await call('GET', '/api/pause');
  await wait(800);
  const pausedAt = await timer.locator('#time').textContent();
  await wait(1500);
  check('standing still when paused', (await timer.locator('#time').textContent()) === pausedAt, pausedAt);
  check('dimmed when paused', await timer.locator('#box').evaluate((el) => el.classList.contains('paused')));
  await call('GET', '/api/start/index/2');
  await timer.waitForFunction(() => document.getElementById('time').textContent.startsWith('-'), null, { timeout: 15000 }).catch(() => {});
  const over = await timer.locator('#time').textContent();
  check('over time, with a minus sign', over.startsWith('-'), over);
  check('in the danger colour', await timer.locator('#time').evaluate((el) => el.classList.contains('danger')));
  await call('GET', '/api/stop');
  await timer.waitForFunction(() => document.getElementById('box').classList.contains('hidden'), null, { timeout: 10000 }).catch(() => {});
  check('hidden again once stopped', await timer.locator('#box').evaluate((el) => el.classList.contains('hidden')));

  check('no page errors and nothing the policy refused', problems.length === 0, problems.join(' | '));
} finally {
  await browser.close();
  await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
