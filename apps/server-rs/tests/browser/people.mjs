// Headless walk-through of /admin People, /join and /my-rooms.
//
// Run with ./run.sh (see there); BROWSER_APP, BROWSER_API and ADMIN_PASSWORD
// override the local defaults.
import { launch } from './support.mjs';

const app = process.env.BROWSER_APP ?? 'http://localhost:4199';
const api = process.env.BROWSER_API ?? 'http://127.0.0.1:8788';
const password = process.env.ADMIN_PASSWORD ?? 'admin-e2e';
const name = `Browser ${Math.random().toString(36).slice(2, 8)}`;
const room = `browser-${Math.random().toString(36).slice(2, 8)}`;
const errors = [];
let failed = false;
const check = (label, ok) => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}`);
};

const browser = await launch();
const admin_page = await browser.newPage();
admin_page.on('console', (m) => m.type() === 'error' && errors.push(`admin: ${m.text()}`));
admin_page.on('response', (r) => r.status() === 404 && console.log(`    404 ${r.request().method()} ${r.url()}`));
admin_page.on('dialog', (d) => d.accept());

await admin_page.goto(`${app}/admin`);
await admin_page.fill('input[type=password]', password);
await admin_page.click('button[type=submit]');
await admin_page.waitForSelector('text=People');
check('admin shows the People section', true);

// a room to own
const token = await admin_page.evaluate(() => sessionStorage.getItem('ontime.admin.token'));
await fetch(`${api}/r/${room}/data/project`, {
  method: 'POST',
  headers: { 'content-type': 'application/json', Authorization: `Bearer ${token}` },
  body: JSON.stringify({ title: 'Browser check' }),
});

await admin_page.fill('input[placeholder^="Name"]', name);
await admin_page.click('text=Add and make a link');
const link = await admin_page.locator('code').first().textContent({ timeout: 5000 });
check('an invite link is shown', link?.includes('/join#code='));
await admin_page.waitForSelector(`td:has-text("${name}")`);
check('the person is listed', true);

// assign the owner through the room row's picker
await admin_page.click('text=Refresh');
const row = admin_page.locator('tr', { has: admin_page.locator(`a:has-text("${room}")`) });
await row.locator('select[aria-label="Add an owner"]').selectOption({ label: name });
await row.locator(`span:has-text("${name}")`).first().waitFor({ timeout: 5000 });
check('the owner chip appears on the room', true);

// the invitee, in a separate browser context
const person = await browser.newContext();
const page = await person.newPage();
page.on('console', (m) => m.type() === 'error' && errors.push(`person: ${m.text()}`));
await page.goto(link);
await page.waitForURL('**/my-rooms', { timeout: 10000 });
check('the link leads to /my-rooms', true);
check('the code is gone from the address', !page.url().includes('code='));
await page.waitForSelector(`text=Signed in as ${name}`);
check('signed in as the person', true);
check('sees the owned room', (await page.locator(`a:has-text("${room}")`).count()) === 1);
check(
  'no socket opened',
  !(await page.evaluate(() => performance.getEntriesByType('resource').some((r) => r.name.includes('/ws')))),
);

await page.reload();
await page.waitForSelector(`text=Signed in as ${name}`);
check('still signed in after reload', true);

const again = await person.newPage();
await again.goto(link);
await again.waitForSelector('text=already been used');
check('the link works only once', true);
await again.close();

await page.click('text=Sign out of this device');
await page.waitForSelector('text=not signed in');
check('sign out', true);

// clean up: remove the person and the room from the list
await admin_page.click('text=Refresh');
const person_row = admin_page.locator('tr', { has: admin_page.locator(`td:text-is("${name}")`) });
await person_row.locator('text=Remove').click();
await admin_page.waitForSelector(`td:text-is("${name}")`, { state: 'detached' });
check('person removed', true);
await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${token}` } });

await admin_page.screenshot({ path: '/tmp/browser-admin.png', fullPage: true });
await browser.close();

const real = errors.filter((e) => !e.includes('401'));
check(`no console errors (${real.length})`, real.length === 0);
for (const e of errors) console.log(`    ${e}`);
process.exit(failed ? 1 : 0);
