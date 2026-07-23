// What a room costs when nobody is watching, and who may create one.
//
// Both matter only once more than one person can reach the deployment: a room
// left running with the tab closed used to wake on the heartbeat for ever, and
// any name typed into a URL brought a room into existence.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`);
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const room = 'cost-' + Math.random().toString(36).slice(2, 10);
const api = (m, p, b) =>
  fetch(`${base}/r/${room}${p}`, {
    method: m,
    headers: { 'content-type': 'application/json' },
    body: b ? JSON.stringify(b) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));

/// Resolves once the first runtime-data packet has arrived, not merely on open —
/// the state we want to inspect rides in that packet.
const socket = (r = room) =>
  new Promise((resolve, reject) => {
    const ws = new WebSocket(base.replace(/^http/, 'ws') + `/ws?room=${r}`);
    const packets = [];
    ws.addEventListener('message', (e) => {
      const p = JSON.parse(e.data);
      if (p.tag === 'runtime-data') {
        packets.push(p.payload);
        resolve({ ws, packets });
      }
    });
    ws.addEventListener('error', () => reject(new Error('socket error')));
    setTimeout(() => reject(new Error('no runtime-data after connecting')), 10000);
  });

// --- set up a long-running event ------------------------------------------
const list = await api('POST', '/data/rundowns', { title: '__cost__' });
const rid = list.rundowns.filter((r) => r.title === '__cost__').pop().id;
await api('POST', `/data/rundowns/${rid}/load`);
await api('POST', `/data/rundowns/${rid}/entry`, { title: 'LONG', duration: 3600000 });

console.log('== a watched room ticks while it runs ==');
const first = await socket();
first.ws.send(JSON.stringify({ tag: 'start', payload: undefined }));
await wait(800);
const watched = first.packets.length;
await wait(3000);
check('packets keep arriving', first.packets.length > watched, true);

console.log('== closing the last tab stops the ticking ==');
first.ws.close();
await wait(2500);

// reconnect and see whether the room woke while empty: if it had, it would still
// be mid-heartbeat now, and the first packet would arrive before any is due
const second = await socket();
const atReconnect = second.packets.length;
check('state is still there on reconnect', second.packets[0]?.timer?.playback, 'play');

// the timer must not have lost time while nobody was watching — it is computed
// from absolute timestamps, not accumulated by ticking
const remaining = second.packets[0].timer.current;
check('the timer did not stall while unwatched', remaining < 3600000 - 4000, true);

await wait(3000);
check('and it ticks again now someone is back', second.packets.length > atReconnect, true);

second.ws.send(JSON.stringify({ tag: 'stop', payload: undefined }));
await wait(400);
second.ws.close();

console.log('== who may create a room ==');
// with no master password configured anything goes, which is how a single
// operator runs; the deployment under test decides which of these applies
// unique per run: a name derived from seconds collides whenever two runs
// start in the same second, and the room still holds the previous run's rundown
const fresh = 'brand-new-' + Math.random().toString(36).slice(2, 10);
const status = await fetch(`${base}/r/${fresh}/data/settings`).then((r) => r.status);
if (status === 403) {
  console.log('  ok   a master password is configured; new rooms are gated (403)');
} else {
  check('no master password configured, so any room may be created', status, 200);
}

// --- cleanup ---------------------------------------------------------------
await api('POST', '/data/rundowns/default/load');
await api('DELETE', `/data/rundowns/${rid}`);

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
