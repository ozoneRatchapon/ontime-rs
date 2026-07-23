// Checks the websocket handshake, ping/pong, and that a REST mutation
// broadcasts a refetch to connected clients.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';
const wsUrl = base.replace(/^http/, 'ws') + '/ws';

const received = [];
const ws = new WebSocket(wsUrl);

const done = new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error('timed out waiting for packets')), 10000);

  ws.addEventListener('open', async () => {
    ws.send(JSON.stringify({ tag: 'ping', payload: 1 }));

    // give the pong a moment, then mutate over REST
    await new Promise((r) => setTimeout(r, 300));
    const list = await fetch(`${base}/data/rundowns`).then((r) => r.json());
    await fetch(`${base}/data/rundowns/${list.loaded}/entry`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ title: 'WS_PROBE' }),
    });
  });

  ws.addEventListener('message', (event) => {
    received.push(JSON.parse(event.data));
    if (received.some((p) => p.tag === 'refetch')) {
      clearTimeout(timer);
      resolve();
    }
  });

  ws.addEventListener('error', (e) => {
    clearTimeout(timer);
    reject(new Error(`socket error: ${e.message ?? e}`));
  });
});

let failed = false;
const check = (label, actual, expected) => {
  const ok = actual === expected;
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got '${actual}' want '${expected}'`}`);
};

try {
  await done;
} catch (error) {
  console.log(`  FAIL ${error.message}`);
  console.log(`  received: ${JSON.stringify(received)}`);
  process.exit(1);
}

const init = received.find((p) => p.tag === 'client-init');
const pong = received.find((p) => p.tag === 'pong');
const refetch = received.find((p) => p.tag === 'refetch');

check('client-init on connect', Boolean(init), true);
check('client-init carries clientId', typeof init?.payload?.clientId, 'string');
check('ping is answered with pong', Boolean(pong), true);
check('mutation broadcasts refetch', refetch?.payload?.target, 'rundown');
check('refetch carries a revision', typeof refetch?.payload?.revision, 'number');
check('refetch carries a rundownId', typeof refetch?.payload?.rundownId, 'string');

ws.close();
console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
