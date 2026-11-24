// Browser check of plan 007: the Activity panel on /my-rooms (owner) and on
// /admin (admin), plus the people events.
//
// Run with ./run.sh (see there); BROWSER_APP, BROWSER_API and ADMIN_PASSWORD
// override the local defaults.
import { chromium } from '@playwright/test';
const app = process.env.BROWSER_APP ?? 'http://localhost:4199';
const api = process.env.BROWSER_API ?? 'http://127.0.0.1:8788';
const password = process.env.ADMIN_PASSWORD ?? password;
const suffix = Math.random().toString(36).slice(2, 8);
const room = `ui-act-${suffix}`;
const errors = [];
let failed = false;
const check = (label, ok) => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}`);
};
const call = (path, { token, session, method = 'GET', body } = {}) =>
  fetch(`${api}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(session ? { 'X-Ontime-Session': session } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  }).then((r) => r.json());

const { token: master } = await call('/admin/login', { method: 'POST', body: { password: password } });
await call(`/r/${room}/data/project`, { token: master, method: 'POST', body: { title: 'Activity UI' } });
const created = await call(`/r/${room}/data/rundowns`, { token: master, method: 'POST', body: { title: 'Run' } });
const rundown = created.rundowns.find((x) => x.title === 'Run').id;
await call(`/r/${room}/data/rundowns/${rundown}/entry`, {
  token: master,
  method: 'POST',
  body: { id: `e-${suffix}`, type: 'event', cue: '2.1', title: 'Keynote' },
});
const owner = await call('/admin/people', { token: master, method: 'POST', body: { name: `Owner ${suffix}` } });
await call(`/admin/rooms/${room}`, { token: master, method: 'PUT', body: { owners: [owner.id] } });
const { code } = await call(`/admin/people/${owner.id}/invite`, { token: master, method: 'POST' });

const browser = await chromium.launch();
const watch = (page, who) => page.on('console', (m) => m.type() === 'error' && errors.push(`${who}: ${m.text()}`));

// --- the owner, on /my-rooms
const owner_page = await (await browser.newContext()).newPage();
watch(owner_page, 'owner');
await owner_page.goto(`${app}/join#code=${code}`);
await owner_page.waitForURL('**/my-rooms');
const session = await owner_page.evaluate(() => Object.entries(localStorage).find(([k]) => k.includes('session'))?.[1]);
await call(`/r/${room}/data/rundowns/${rundown}/entry`, {
  session,
  method: 'PUT',
  body: { id: `e-${suffix}`, note: `secret-${suffix}` },
});
const row = owner_page.locator('tr', { has: owner_page.locator(`a:has-text("${room}")`) });
await row.locator('button:has-text("Activity")').click();
await owner_page.waitForSelector('text=edited 2.1 Keynote (note)', { timeout: 10000 });
check('the owner sees their edit, by field name', true);
check('attributed to them', (await owner_page.locator(`td:text-is("Owner ${suffix}")`).count()) > 0);
check('and the admin set-up', (await owner_page.locator('text=added 2.1 Keynote').count()) === 1);
check('never the value', (await owner_page.locator(`text=secret-${suffix}`).count()) === 0);
await owner_page.screenshot({ path: '/tmp/browser-activity-owner.png', fullPage: true });
await owner_page.locator('button:has-text("Hide activity")').click();
check('the panel closes', (await owner_page.locator('text=edited 2.1 Keynote').count()) === 0);

// --- the admin, on /admin
const admin_page = await (await browser.newContext()).newPage();
watch(admin_page, 'admin');
await admin_page.goto(`${app}/admin`);
await admin_page.fill('input[type="password"]', password);
await admin_page.keyboard.press('Enter');
const admin_row = admin_page.locator('tr', { has: admin_page.locator(`a:has-text("${room}")`) });
await admin_row.waitFor({ timeout: 10000 });
await admin_row.locator('button:has-text("Activity")').click();
await admin_page.waitForSelector('text=created the rundown Run', { timeout: 10000 });
check('the admin sees the room activity', true);
check('including the owner edit', (await admin_page.locator('text=edited 2.1 Keynote (note)').count()) === 1);
await admin_page.screenshot({ path: '/tmp/browser-activity-admin.png', fullPage: true });
await admin_page
  .locator('section', { has: admin_page.locator('h2:has-text("People activity")') })
  .locator('button:has-text("Show")')
  .click();
await admin_page.waitForSelector(`text=added the person Owner ${suffix}`, { timeout: 10000 });
check('the admin sees people events', true);
check('the owner signing in', (await admin_page.locator('text=signed in with an invite link').count()) > 0);
check(
  'and who looks after the room',
  (await admin_page.locator(`text=changed who looks after the room (owners) in ${room}`).count()) === 1,
);
await admin_page.locator('h2:has-text("People activity")').scrollIntoViewIfNeeded();
await admin_page.screenshot({ path: '/tmp/browser-activity-people.png' });

await browser.close();
await fetch(`${api}/admin/people/${owner.id}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });

check(`no console errors (${errors.length})`, errors.length === 0);
for (const e of errors) console.log(`    ${e}`);
process.exit(failed ? 1 : 0);
