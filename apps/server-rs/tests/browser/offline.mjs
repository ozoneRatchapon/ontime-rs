// What a show sees when the venue's network drops: a timer screen and an
// operator's editor each lose the connection for a while, then get it back.
//
// It measures rather than asserts most of it, since what is right during an
// outage is a choice; it fails only on what is wrong whatever is chosen: a
// screen that does not come back, or comes back showing the wrong time.
//
// Run with ./run.sh (see there); BROWSER_APP, BROWSER_API and ADMIN_PASSWORD
// override the local defaults.
import { launch } from './support.mjs';
const app = process.env.BROWSER_APP ?? 'http://localhost:4199';
const api = process.env.BROWSER_API ?? 'http://127.0.0.1:8788';
const password = process.env.ADMIN_PASSWORD ?? 'admin-e2e';
const room = `ui-offline-${Math.random().toString(36).slice(2, 8)}`;
let failed = false;
const check = (label, ok, detail = '') => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` ${detail}`}`);
};
const note = (label, value) => console.log(`  ..   ${label}: ${value}`);
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
// how long each outage lasts: short ones are absorbed by the connection itself,
// longer ones outlast the client's 10 s silence watchdog
const OUTAGE_S = Number(process.env.OUTAGE_S ?? 30);

const login = await fetch(`${api}/admin/login`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ password }),
});
const master = login.ok ? (await login.json()).token : null;
const auth = master ? { Authorization: `Bearer ${master}` } : {};
const call = (path, { method = 'GET', body } = {}) =>
  fetch(`${api}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...auth },
    body: body ? JSON.stringify(body) : undefined,
  }).then((r) => r.json().catch(() => null));
const playback = async () => (await call('/api/poll')).payload.timer;

// one long event, playing
const created = await call('/data/rundowns', { method: 'POST', body: { title: 'Outage' } });
const rundown = created.rundowns.find((r) => r.title === 'Outage').id;
await call(`/data/rundowns/${rundown}/entry`, {
  method: 'POST',
  body: { id: 'ev-a', type: 'event', cue: '1', title: 'Keynote', duration: 1_800_000 },
});
await call(`/data/rundowns/${rundown}/load`, { method: 'POST' });
await call('/api/start');

/** "mm:ss" or "hh:mm:ss" on screen, as seconds; null when not a time. */
const seconds = (text) => {
  const parts = text.trim().replace(/^-/, '').split(':').map(Number);
  if (parts.length < 2 || parts.some(Number.isNaN)) return null;
  return parts.reduce((total, part) => total * 60 + part, 0);
};
const shown = async (page) => seconds(await page.locator('.timer').first().innerText());
const until = async (done, ms) => {
  const start = Date.now();
  while (Date.now() - start < ms) {
    if (await done()) return Date.now() - start;
    await wait(200);
  }
  return null;
};

const browser = await launch();
try {
  console.log(`== a timer screen loses the network for ${OUTAGE_S} seconds ==`);
  const screenContext = await browser.newContext({ viewport: { width: 1280, height: 720 } });
  const screen = await screenContext.newPage();
  const socketLog = [];
  screen.on(
    'console',
    (m) =>
      /WebSocket|socket/i.test(m.text()) && socketLog.push(`${((Date.now() - t0) / 1000).toFixed(1)}s ${m.text()}`),
  );
  let t0 = Date.now();
  await screen.goto(`${app}/timer?room=${room}`);
  await screen.getByTestId('timer-view').waitFor({ timeout: 15000 });
  await until(async () => (await shown(screen)) !== null, 15000);
  await wait(1500);
  const before = await shown(screen);
  check('the screen shows the running timer', before !== null && before > 0, `(${before})`);

  t0 = Date.now();
  await screenContext.setOffline(true);
  const samples = [];
  for (let s = 0; s < OUTAGE_S; s += 5) {
    if (s === 5) await call('/api/pause'); // the operator pauses while the screen cannot hear it
    samples.push(`${s}s=${await shown(screen)}`);
    await wait(5000);
  }
  const offlineEnd = await shown(screen);
  note('shown when the network dropped', before);
  note('shown during the outage', samples.join(', '));
  note(`shown ${OUTAGE_S} s into the outage`, offlineEnd);
  note('socket messages', socketLog.join(' | ') || 'none');

  const server = await playback();
  await screenContext.setOffline(false);
  const resync = await until(async () => {
    const value = await shown(screen);
    return value !== null && Math.abs(value - Math.round(server.current / 1000)) <= 1;
  }, 45000);
  check('it comes back by itself once the network returns', resync !== null, '(not within 45 s)');
  if (resync !== null) note('back in step after', `${(resync / 1000).toFixed(1)} s`);
  check('and shows the paused time the operator set meanwhile', resync !== null);

  console.log('== the operator loses the network and presses start ==');
  const operatorContext = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const editor = await operatorContext.newPage();
  await editor.goto(`${app}/editor?room=${room}`);
  const start = editor.locator('button[class*="play"]:not([class*="playback"])').first();
  await start.waitFor({ timeout: 20000 });
  await wait(1500);
  await operatorContext.setOffline(true);
  await wait(OUTAGE_S * 1000 - 3000);
  const enabled = await start.isEnabled();
  note('start button while offline', enabled ? 'still enabled' : 'disabled');
  if (enabled) await start.click();
  await wait(1500);
  const offlineBanner = await editor
    .getByText(/offline|disconnected|connection lost|reconnect/i)
    .first()
    .isVisible()
    .catch(() => false);
  note('the editor says the connection is lost', offlineBanner ? 'yes' : 'no');
  await wait(3000);
  const duringOutage = await playback();
  note('the server while the operator is offline', duringOutage.playback);
  await operatorContext.setOffline(false);
  await wait(8000);
  const afterOutage = await playback();
  note(
    'the press made while offline',
    afterOutage.playback === 'play' ? 'arrived after reconnecting' : `was lost (still ${afterOutage.playback})`,
  );
  const back = await until(async () => {
    const value = await shown(screen);
    return value !== null && Math.abs(value - Math.round(afterOutage.current / 1000)) <= 1;
  }, 15000);
  check('the screen still agrees with the server', back !== null);
} finally {
  await browser.close();
  if (master) await fetch(`${api}/admin/rooms/${room}`, { method: 'DELETE', headers: auth });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
