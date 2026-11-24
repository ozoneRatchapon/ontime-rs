// Behaviour distilled from upstream ontime v4.11 → v4.15 (plan 003, A1–A9),
// over the real HTTP and websocket surface. Its own throwaway room.
import { wait } from './support.mjs';

const base = process.argv[2] ?? 'http://127.0.0.1:8787';
const room = 'up-' + Math.random().toString(36).slice(2, 10);
const r = (path) => `${base}/r/${room}${path}`;
// a deployment with a master password only lets the master create a room
const token = process.env.ONTIME_TOKEN;

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const call = async (method, path, body) => {
  const response = await fetch(r(path), {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  return { status: response.status, body: text ? JSON.parse(text) : null };
};

const H = 3_600_000;
const M = 60_000;
await call('POST', '/data/settings', { timezoneOffset: 7 * H });
const created = await call('POST', '/data/rundowns', { title: 'สอบ วันที่ 1' });
check('a Thai rundown title is accepted', created.status, 200);
const rid = created.body.rundowns.find((x) => x.title === 'สอบ วันที่ 1').id;
const event = (id, cue, title, start) => ({ id, cue, title, type: 'event', timeStart: start, duration: 10 * M, timeStrategy: 'lock-duration' });
check(
  'a rundown is replaced in place',
  (await call('PUT', `/data/rundowns/${rid}`, { entries: [event('e1', '1', 'One', 9 * H), event('e2', '2', 'Two', 9 * H + 10 * M)] })).status,
  200,
);
await call('POST', `/data/rundowns/${rid}/load`);

// --- the socket --------------------------------------------------------------
const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?${new URLSearchParams({ room, ...(token ? { token } : {}) })}`);
const packets = [];
ws.addEventListener('message', (e) => packets.push(JSON.parse(e.data)));
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('socket error')));
});
const send = (tag, payload) => ws.send(JSON.stringify({ tag, payload }));
const last = (tag) => packets.filter((p) => p.tag === tag).at(-1);

console.log('== A1: a Pong carries the room time of day ==');
const sent = Date.now();
send('ping', 1234.5);
await wait(500);
const pong = last('pong');
check('the payload is echoed', pong?.payload, 1234.5);
const expected = (sent + 7 * H) % (24 * H);
check('clock is the time of day in the room timezone (±5 s)', Math.abs((pong?.clock ?? -1e9) - expected) < 5000, true);

console.log('== A2: aux timers carry their names ==');
const named = await call('POST', '/data/settings', { auxTimerNames: [' Q&A ', 'Break'] });
check('names are cleaned on save', named.body.auxTimerNames, ['Q&A', 'Break', '']);
await wait(500);
check('the runtime shows the name', last('runtime-data')?.payload?.auxtimer1?.name, 'Q&A');
check('names that are not a list are refused', (await call('POST', '/data/settings', { auxTimerNames: 'x' })).status, 400);
check('a badly typed setting is refused', (await call('POST', '/data/settings', { timeFormat: 24 })).status, 400);
check('and nothing was reset', (await call('GET', '/data/settings')).body.auxTimerNames, ['Q&A', 'Break', '']);

console.log('== A3: replacing the loaded rundown keeps playback ==');
send('start', undefined);
await wait(800);
check('playing', last('runtime-data')?.payload?.timer?.playback, 'play');
await call('PUT', `/data/rundowns/${rid}`, { entries: [event('e1', '1', 'One, revised', 9 * H), event('e3', '3', 'Three', 9 * H + 10 * M)] });
await wait(800);
const after = last('runtime-data')?.payload;
check('still playing', after?.timer?.playback, 'play');
check('the running event shows the new title', after?.eventNow?.title, 'One, revised');
check('next is the new entry', after?.eventNext?.id, 'e3');
await call('PUT', `/data/rundowns/${rid}`, { entries: [event('e3', '3', 'Three', 9 * H + 10 * M)] });
await wait(800);
check('without the running event it stops', last('runtime-data')?.payload?.timer?.playback, 'stop');

console.log('== A4: custom field values follow the field ==');
await call('POST', '/data/custom-fields', { label: 'Mic', type: 'text', colour: '' });
await call('PUT', `/data/rundowns/${rid}/entry`, { id: 'e3', custom: { Mic: '2', Ghost: 'x' } });
check('unknown values are dropped', (await call('GET', `/data/rundowns/${rid}`)).body.entries.e3.custom, { Mic: '2' });
await call('PUT', '/data/custom-fields/Mic', { label: 'Mic 2' });
check('a rename moves the value', (await call('GET', `/data/rundowns/${rid}`)).body.entries.e3.custom, { Mic_2: '2' });
await call('DELETE', '/data/custom-fields/Mic_2');
check('a delete removes it', (await call('GET', `/data/rundowns/${rid}`)).body.entries.e3.custom, {});

console.log('== A5: renumber as the client asks, and input that does not apply ==');
await call('PUT', `/data/rundowns/${rid}`, { entries: ['a', 'b', 'c'].map((id, i) => event(id, `x${i}`, id, 9 * H + i * 10 * M)) });
const renumbered = await call('PATCH', `/data/rundowns/${rid}/renumber`, { ids: ['c', 'a'], prefix: 'A', start: '10', increment: '5' });
check('only the given cues, in order', ['a', 'b', 'c'].map((id) => renumbered.body.entries[id].cue), ['A 15', 'x1', 'A 10']);
check('a bad number is 422', (await call('PATCH', `/data/rundowns/${rid}/renumber`, { ids: ['a'], prefix: '', start: '.5', increment: '1' })).status, 422);
check('a duplicate id is 409', (await call('POST', `/data/rundowns/${rid}/entry`, { type: 'event', id: 'a' })).status, 409);
check('a type change is 422', (await call('PUT', `/data/rundowns/${rid}/entry`, { id: 'a', type: 'delay' })).status, 422);

console.log('== A6: share links need a full base URL ==');
check('without a protocol: 422', (await call('POST', '/data/session/url', { baseUrl: '192.168.1.10:4001', path: 'timer' })).status, 422);
const share = await call('POST', '/data/session/url', { baseUrl: 'https://example.pages.dev', path: 'timer', authenticate: false, lockConfig: false, lockNav: false });
check('a full one works', share.body.url, 'https://example.pages.dev/timer');

console.log('== A9: rundown titles ==');
check('symbols only: 422', (await call('POST', '/data/rundowns', { title: '-,.' })).status, 422);
check('rename to emoji only: 422', (await call('PATCH', `/data/rundowns/${rid}`, { title: '🎉' })).status, 422);

ws.close();
if (token) {
  // take it off the admin room list again
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${token}` } });
}
process.exit(failed ? 1 : 0);
