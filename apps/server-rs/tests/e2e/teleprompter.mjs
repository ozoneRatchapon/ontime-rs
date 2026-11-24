// Plan 008, T2: the room's shared teleprompter, end to end.
//
//   npx wrangler dev --port 8787 --local
//   node teleprompter.mjs http://127.0.0.1:8787
//
// A controller socket drives the transport and a follower socket sees the
// same sync; edits keep the reader in place; Ontime loading an event moves the
// reader in event mode; playback stops at the end of an event on the alarm.
import { wait } from './support.mjs';

const base = process.argv[2] ?? 'http://127.0.0.1:8787';
let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const room = `tp-${Math.random().toString(36).slice(2, 8)}`;
const call = (path, { method = 'GET', body } = {}) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
const json = async (response) => response.json();

// a socket keeping the latest teleprompter keys it was sent
const socket = () =>
  new Promise((resolve, reject) => {
    const ws = new WebSocket(`${base.replace(/^http/, 'ws')}/ws?room=${room}`);
    const seen = { sync: null, state: null, syncs: [] };
    ws.addEventListener('message', (event) => {
      const packet = JSON.parse(event.data);
      if (packet.tag !== 'runtime-data') return;
      if (packet.payload.teleprompterSync) {
        seen.sync = packet.payload.teleprompterSync;
        seen.syncs.push(seen.sync);
      }
      if (packet.payload.teleprompter) seen.state = packet.payload.teleprompter;
    });
    ws.addEventListener('open', () =>
      resolve({ ws, seen, send: (payload) => ws.send(JSON.stringify({ tag: 'teleprompter', payload })), raw: (tag, payload) => ws.send(JSON.stringify({ tag, payload })) }),
    );
    ws.addEventListener('error', () => reject(new Error('socket error')));
  });

// --- a room with three events ----------------------------------------------
const created = await json(await call('/data/rundowns', { method: 'POST', body: { title: 'Prompt' } }));
const rundown = created.rundowns.find((r) => r.title === 'Prompt').id;
const add = (id, cue, note) =>
  call(`/data/rundowns/${rundown}/entry`, { method: 'POST', body: { id, type: 'event', cue, title: `Title ${cue}`, note, duration: 600000 } });
await add('ev-a', '1', 'Good evening and welcome to the show tonight');
await add('ev-b', '2', 'Our first guest needs no introduction at all');
await add('ev-c', '3', 'Thank you and good night');
await call(`/data/rundowns/${rundown}/load`, { method: 'POST' });

console.log('== settings ==');
const settings = await json(await call('/data/teleprompter/settings'));
check('defaults', [settings.script, settings.charsPerLine, settings.heading], ['note', 32, 'title']);
// upstream also refuses a body which is not an object; our hosts read such a
// body as {}, an empty patch, which changes nothing
for (const body of [{ script: 123 }, { script: 'custom-missing' }, { heading: 'banner' }, { charsPerLine: 9 }, { charsPerLine: 32.5 }, { charsPerLine: '32' }, { charsPerLine: null }]) {
  check(`refused ${JSON.stringify(body)}`, (await call('/data/teleprompter/settings', { method: 'POST', body })).status, 422);
}
const patched = await call('/data/teleprompter/settings', { method: 'POST', body: { charsPerLine: 20 } });
check('a partial patch', [patched.status, (await json(patched)).charsPerLine], [200, 20]);
check('nothing else changed', (await json(await call('/data/teleprompter/settings'))).heading, 'title');

console.log('== the script ==');
const script = await json(await call('/data/teleprompter/script'));
check('one event per note, in order', script.events.map((e) => e.id), ['ev-a', 'ev-b', 'ev-c']);
check('cut at 20 characters', script.events[0].lines.map((l) => l.kind), ['heading', 'text', 'text', 'text', 'blank']);
check('the revision is a number', typeof script.revision, 'number');
const titles = await json(await call('/data/teleprompter/script?source=title'));
check('another source is built for the asker', [titles.revision, titles.events[0].lines[1].text], [0, 'Title 1']);

console.log('== a controller drives, a follower follows ==');
const controller = await socket();
const follower = await socket();
await wait(300);
controller.send({ goto: { cue: '2' } });
await wait(400);
check('goto by cue', follower.seen.state?.event?.id, 'ev-b');
controller.send({ speed: 40 });
controller.send('play');
await wait(500);
check('the follower sees it playing', [follower.seen.sync?.playback, follower.seen.sync?.speed], ['play', 40]);
check('both count from the same sync', JSON.stringify(follower.seen.sync), JSON.stringify(controller.seen.sync));
check('rows from the same script', follower.seen.sync?.revision, script.revision);
const playingSince = follower.seen.sync.at;
await wait(1500);
check('no new sync while nothing happens', follower.seen.sync.at, playingSince);

console.log('== an edit above the reader during playback ==');
await call(`/data/rundowns/${rundown}/entry`, { method: 'PUT', body: { id: 'ev-a', note: 'A much longer welcome which now takes many more lines than it did before' } });
await wait(600);
check('the script revision changed', follower.seen.sync?.revision !== script.revision, true);
check('the reader is still in their event', follower.seen.state?.event?.id, 'ev-b');
check('still playing', follower.seen.sync?.playback, 'play');

console.log('== playback stops at the end of the event ==');
controller.send({ speed: 100 });
await wait(4000);
check('paused at the end', [follower.seen.state?.playback, follower.seen.state?.ended], ['pause', 'event']);
check('still in event 2', follower.seen.state?.event?.id, 'ev-b');

console.log('== event by event, the reader follows the loaded event ==');
controller.raw('load', { id: 'ev-c' });
await wait(500);
check('moved to the loaded event', follower.seen.state?.event?.id, 'ev-c');
controller.send({ mode: 'script' });
controller.raw('load', { id: 'ev-a' });
await wait(500);
check('script mode ignores loading', follower.seen.state?.event?.id, 'ev-c');

console.log('== commands ==');
controller.send('top');
await wait(300);
check('top', follower.seen.state?.event?.id, 'ev-a');
controller.send('next');
await wait(300);
check('next', follower.seen.state?.event?.id, 'ev-b');
controller.send({ goto: { index: 3 } });
await wait(300);
check('goto by index', follower.seen.state?.event?.id, 'ev-c');
controller.send({ blackout: 'toggle' });
await wait(300);
check('blackout', follower.seen.state?.blackout, 'on');
const before = follower.seen.state;
controller.send({ goto: { cue: 'nope' } });
controller.send('dance');
await wait(300);
check('refused commands change nothing', follower.seen.state, before);

console.log('== the source follows its custom field ==');
await call('/data/custom-fields', { method: 'POST', body: { label: 'Lines', type: 'text', colour: '' } });
check('read a custom field', (await call('/data/teleprompter/settings', { method: 'POST', body: { script: 'custom-Lines' } })).status, 200);
await call('/data/custom-fields/Lines', { method: 'PUT', body: { label: 'Speech' } });
check('renamed with it', (await json(await call('/data/teleprompter/settings'))).script, 'custom-Speech');
await call('/data/custom-fields/Speech', { method: 'DELETE' });
check('back to the note when deleted', (await json(await call('/data/teleprompter/settings'))).script, 'note');

console.log('== a socket joining late lands in the same place ==');
const late = await socket();
await wait(400);
check('same sync as the others', JSON.stringify(late.seen.sync), JSON.stringify(follower.seen.sync));

for (const s of [controller, follower, late]) s.ws.close();
process.exit(failed ? 1 : 0);
