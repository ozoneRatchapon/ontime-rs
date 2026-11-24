// Browser check of plan 008: a controller and a remote screen of the same room
// scroll together from the shared transport in the room's Durable Object.
//
//   (apps/server-rs)  npx wrangler dev --port 8787 --local
//   (apps/client)     VITE_WORKER_URL=http://127.0.0.1:8787 pnpm build && npx vite preview --port 4199
//   node apps/server-rs/tests/browser/teleprompter.mjs <dir holding playwright-core>
//
// wrangler dev does not watch packages/ontime-core: restart it after a core change.
import { createRequire } from 'node:module';

const { chromium } = createRequire(process.argv[2])('playwright-core');
const app = 'http://localhost:4199';
const api = 'http://127.0.0.1:8787';
const room = `ui-tp-${Math.random().toString(36).slice(2, 8)}`;
const errors = [];
let failed = false;
const check = (label, ok, detail = '') => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` ${detail}`}`);
};
const call = (path, { method = 'GET', body } = {}) =>
  fetch(`${api}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });

// a room with three events and long enough notes to scroll
const created = await (await call('/data/rundowns', { method: 'POST', body: { title: 'Prompt' } })).json();
const rundown = created.rundowns.find((r) => r.title === 'Prompt').id;
const long = (word) => Array.from({ length: 40 }, (_, i) => `${word} ${i + 1}`).join(' ');
for (const [id, cue, note] of [
  ['ev-a', '1', long('Welcome')],
  ['ev-b', '2', long('Guest')],
  ['ev-c', '3', 'Good night everyone'],
]) {
  await call(`/data/rundowns/${rundown}/entry`, {
    method: 'POST',
    body: { id, type: 'event', cue, title: `Part ${cue}`, note, duration: 600000 },
  });
}
await call(`/data/rundowns/${rundown}/load`, { method: 'POST' });

// the reading row, measured the way upstream's own e2e measures it
const readingRow = (page) =>
  page.getByTestId('teleprompter-scroller').evaluate((scroller) => {
    const content = scroller.querySelector('.teleprompter__content');
    const row = scroller.querySelector('.teleprompter__row');
    if (!content || !row) throw new Error('No rows rendered');
    const moved = -new DOMMatrix(getComputedStyle(content).transform).m42;
    return (scroller.scrollTop + moved) / Number.parseFloat(getComputedStyle(row).height);
  });

const browser = await chromium.launch();
const open = async (role) => {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
  page.on('console', (m) => m.type() === 'error' && errors.push(`${role}: ${m.text()}`));
  page.on('pageerror', (e) => errors.push(`${role}: ${e.message}`));
  await page.goto(`${app}/teleprompter?room=${room}&role=${role}`);
  await page.getByTestId('teleprompter-view').waitFor({ timeout: 15000 });
  await page.locator('.teleprompter__row').first().waitFor({ state: 'attached', timeout: 15000 });
  return page;
};

const controller = await open('controller');
const remote = await open('remote');
check('both show the script', (await remote.getByText('Welcome 1 Welcome 2', { exact: false }).count()) > 0);
check('the remote has no controls', (await remote.getByTestId('teleprompter-controls').count()) === 0);

await controller.mouse.move(640, 650);
await controller.getByTestId('teleprompter-play').click({ timeout: 5000 });
const start = await readingRow(remote);
await remote.waitForTimeout(3000);
const [a, b] = [await readingRow(controller), await readingRow(remote)];
check('the remote moves with the controller', b > start + 0.3, `start ${start} remote ${b}`);
check('both read the same line', Math.abs(a - b) < 0.6, `controller ${a} remote ${b}`);

await controller.mouse.move(640, 650);
await controller.getByTestId('teleprompter-play').click();
await remote.waitForTimeout(800);
const paused = await readingRow(remote);
await remote.waitForTimeout(1500);
check('pausing stops both', Math.abs((await readingRow(remote)) - paused) < 0.05);

const late = await open('remote');
await late.waitForTimeout(800);
check(
  'a screen joining late lands on the same line',
  Math.abs((await readingRow(late)) - paused) < 0.6,
  `${await readingRow(late)} vs ${paused}`,
);

await call(`/data/rundowns/${rundown}/entry`, {
  method: 'PUT',
  body: { id: 'ev-c', note: 'Thank you and safe travels home' },
});
await remote
  .getByText('safe travels home', { exact: false })
  .waitFor({ timeout: 8000 })
  .then(
    () => check('an edit reaches the screens', true),
    () => check('an edit reaches the screens', false),
  );

await remote.screenshot({ path: '/tmp/teleprompter-remote.png' });
await controller.mouse.move(640, 650);
await controller.screenshot({ path: '/tmp/teleprompter-controller.png' });
await browser.close();

const real = errors.filter((e) => !e.includes('401') && !e.includes('403'));
check(`no console errors (${real.length})`, real.length === 0);
for (const e of real) console.log(`    ${e}`);
process.exit(failed ? 1 : 0);
