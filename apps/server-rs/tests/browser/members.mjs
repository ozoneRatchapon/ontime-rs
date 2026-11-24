// Browser walk-through of phase 2: owner team panel, members-only switch,
// operator editing, and the notice an anonymous editor sees.
//
// Run with ./run.sh (see there); BROWSER_APP, BROWSER_API and ADMIN_PASSWORD
// override the local defaults.
import { chromium } from '@playwright/test';
const app = process.env.BROWSER_APP ?? 'http://localhost:4199';
const api = process.env.BROWSER_API ?? 'http://127.0.0.1:8788';
const password = process.env.ADMIN_PASSWORD ?? password;
const suffix = Math.random().toString(36).slice(2, 8);
const room = `ui-${suffix}`;
const errors = [];
let failed = false;
const check = (label, ok) => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}`);
};
const call = (path, { token, method = 'GET', body } = {}) =>
  fetch(`${api}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }).then((r) => r.json());

// setup through the API: a room, an owner and their invite link
const { token: master } = await call('/admin/login', { method: 'POST', body: { password: password } });
await call(`/r/${room}/data/project`, { token: master, method: 'POST', body: { title: 'UI check' } });
const owner = await call('/admin/people', { token: master, method: 'POST', body: { name: `Owner ${suffix}` } });
await call(`/admin/rooms/${room}`, { token: master, method: 'PUT', body: { owners: [owner.id] } });
const { code } = await call(`/admin/people/${owner.id}/invite`, { token: master, method: 'POST' });

const browser = await chromium.launch();
const watch = (page, who) => {
  page.on('console', (m) => m.type() === 'error' && errors.push(`${who}: ${m.text()}`));
  page.on('dialog', (d) => d.accept());
};

// --- the owner
const owner_ctx = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
const op_page = await owner_ctx.newPage();
watch(op_page, 'owner');
await op_page.goto(`${app}/join#code=${code}`);
await op_page.waitForURL('**/my-rooms');
const row = op_page.locator('tr', { has: op_page.locator(`a:has-text("${room}")`) });
await row.waitFor();
check('the owner sees the room as owner', (await row.locator('text=owner').count()) > 0);
await row.locator('text=Team').click();
await op_page.fill('input[placeholder="Operator name"]', `Op ${suffix}`);
await op_page.click('text=Add operator and make a link');
const link = await op_page.locator('code').first().textContent({ timeout: 5000 });
check('an operator link is shown', link?.includes('/join#code='));
await op_page.waitForSelector(`td:text-is("Op ${suffix}")`);
check('the operator is in the team', true);

await op_page.locator('label:has-text("Editing needs sign-in")').click();
await op_page.waitForSelector('text=only the people below can edit');
check('the switch turns on', true);

// --- anonymous editor
const anon = await browser.newPage();
watch(anon, 'anonymous');
await anon.goto(`${app}/editor?room=${room}`);
await anon.waitForSelector('text=This room needs sign-in to edit', { timeout: 15000 });
check('an anonymous editor is told it cannot edit', true);

// --- the operator
const operator_ctx = await browser.newContext();
const operator = await operator_ctx.newPage();
watch(operator, 'operator');
await operator.goto(link);
await operator.waitForURL('**/my-rooms');
await operator.locator(`a:has-text("${room}")`).waitFor();
check('the operator sees the room', (await operator.locator(`a:has-text("${room}")`).count()) === 1);
check('as operator, without a Team button', (await operator.locator('text=Team').count()) === 0);
await operator.goto(`${app}/editor?room=${room}`);
await operator.waitForTimeout(4000);
check(
  'the operator gets no notice in the editor',
  (await operator.locator('text=This room needs sign-in to edit').count()) === 0,
);

// cleanup
await op_page.locator('label:has-text("Editing needs sign-in")').click();
await op_page.waitForSelector('text=anyone with the link can edit');
check('the switch turns off', true);
await browser.close();
const people = (await call('/admin/people', { token: master })).people.filter((p) => p.name.endsWith(suffix));
for (const p of people)
  await fetch(`${api}/admin/people/${p.id}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });

const real = errors.filter((e) => !e.includes('401') && !e.includes('403'));
check(`no unexpected console errors (${real.length})`, real.length === 0);
for (const e of real) console.log(`    ${e}`);
process.exit(failed ? 1 : 0);
