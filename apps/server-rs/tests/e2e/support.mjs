// Shared helpers for the end-to-end suites.
//
// The socket guard is the important one: a deploy rolling over evicts the Durable
// Object and closes open sockets. Without noticing, a suite carries on sending into
// a dead socket and then fails on a stale packet, which reads as a mysterious flake
// rather than "the connection went away".

/** Opens a websocket for a room and collects the packets a suite cares about. */
export async function openSocket(base, room, { timeoutMs = 10000 } = {}) {
  const query = room ? `?room=${room}` : '';
  const ws = new WebSocket(base.replace(/^http/, 'ws') + `/ws${query}`);

  const runtime = [];
  const clientLists = [];
  let closed = null;

  ws.addEventListener('close', (event) => {
    closed = `code ${event.code}${event.reason ? ` (${event.reason})` : ''}`;
  });
  ws.addEventListener('message', (event) => {
    const packet = JSON.parse(event.data);
    if (packet.tag === 'runtime-data') runtime.push(packet.payload);
    if (packet.tag === 'client-list') clientLists.push(packet.payload);
  });

  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve);
    ws.addEventListener('error', () => reject(new Error('socket error')));
    setTimeout(() => reject(new Error('connect timeout')), timeoutMs);
  });

  /** Fails loudly if the connection went away, naming what we were doing. */
  const assertAlive = (stage) => {
    if (closed !== null) {
      console.log(`\n  the websocket closed during "${stage}": ${closed}`);
      console.log('  a deploy rolling over mid-run will do this; rerun against a settled deployment');
      process.exit(1);
    }
  };

  return {
    ws,
    runtime,
    clientLists,
    assertAlive,
    latest: () => runtime[runtime.length - 1],
    send: (tag, payload) => {
      assertAlive(tag);
      ws.send(JSON.stringify({ tag, payload }));
    },
    close: () => ws.close(),
  };
}

export const wait = (ms) => new Promise((r) => setTimeout(r, ms));
