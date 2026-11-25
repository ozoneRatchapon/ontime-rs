import type { AddressInfo } from 'node:net';

import { record } from '@record';
import { dispatchFromAdapter } from '@upstream/apps/server/src/api-integration/integration.controller.ts';
import { integrationRouter } from '@upstream/apps/server/src/api-integration/integration.router.ts';
// The integration API contract (RIIR 5): what upstream's own router and
// dispatcher make of each request. Its services are replaced by recorders
// (integration-stubs/, laid over the upstream tree by generate.sh), so a case
// records which service call a request turns into, or the error it is refused
// with. tests/integration_contract_tests.rs makes the same requests of our
// parser and compares.
//
// HTTP cases go through a real Express app with upstream's integrationRouter,
// so Express's own path and query handling is part of the contract. Websocket
// cases call dispatchFromAdapter directly, as WebsocketAdapter does.
import express from 'express';

// the entries `change` is checked against: one of each kind
const rundown = {
  id: 'rundown',
  entries: {
    e1: {
      id: 'e1',
      type: 'event',
      title: 'Opening',
      note: '',
      cue: '1',
      skip: false,
      flag: false,
      countToEnd: false,
      colour: '',
      custom: {},
      timeWarning: 120000,
      timeDanger: 60000,
      endAction: 'none',
      timerType: 'count-down',
      linkStart: false,
      timeStrategy: 'lock-duration',
      duration: 600000,
      timeStart: 36000000,
      timeEnd: 36600000,
    },
    g1: {
      id: 'g1',
      type: 'group',
      title: 'Block',
      note: '',
      colour: '',
      custom: {},
      targetDuration: null,
      entries: [],
    },
    d1: { id: 'd1', type: 'delay', duration: 60000 },
    m1: { id: 'm1', type: 'milestone', title: 'Doors', cue: 'M', note: '', colour: '', custom: {} },
  },
};
const customFields = {
  lighting: { label: 'Lighting', type: 'text', colour: '' },
  sound: { label: 'Sound', type: 'text', colour: '' },
};

type HttpCase = { name: string; path: string; ok?: boolean };
type WsCase = { name: string; tag: string; payload?: unknown; ok?: boolean };

const http: HttpCase[] = [
  // the router itself
  { name: 'hello', path: '/api' },
  { name: 'hello with a slash', path: '/api/' },
  { name: 'unknown action', path: '/api/explode' },
  { name: 'action names are case-insensitive', path: '/api/PAUSE' },
  { name: 'version', path: '/api/version' },
  { name: 'poll', path: '/api/poll' },
  // playback
  { name: 'start', path: '/api/start' },
  { name: 'start fails', path: '/api/start', ok: false },
  { name: 'start next', path: '/api/start/next' },
  { name: 'start next fails', path: '/api/start/next', ok: false },
  { name: 'start previous', path: '/api/start/previous' },
  { name: 'start by index', path: '/api/start/index/3' },
  { name: 'start by index fails', path: '/api/start/index/3', ok: false },
  { name: 'start by index zero', path: '/api/start/index/0' },
  { name: 'start by negative index', path: '/api/start/index/-2' },
  { name: 'start by fractional index', path: '/api/start/index/1.5', ok: false },
  { name: 'start by index not a number', path: '/api/start/index/three' },
  { name: 'start by index from the query', path: '/api/start?index=2' },
  { name: 'start by index in hex', path: '/api/start/index/0x10' },
  { name: 'start by index with spaces', path: '/api/start/index/%202%20' },
  { name: 'start by id', path: '/api/start/id/abc-123' },
  { name: 'start by id fails', path: '/api/start/id/abc-123', ok: false },
  { name: 'start by cue', path: '/api/start/cue/12' },
  { name: 'start by cue fails', path: '/api/start/cue/4A', ok: false },
  { name: 'start by an encoded cue', path: '/api/start/cue/A%201' },
  { name: 'start with a stray word', path: '/api/start/now' },
  { name: 'start with a trailing slash', path: '/api/start/' },
  { name: 'start with an unknown query', path: '/api/start?foo=bar' },
  { name: 'start ignores the room token', path: '/api/start?token=secret' },
  { name: 'load next', path: '/api/load/next' },
  { name: 'load previous', path: '/api/load/previous' },
  { name: 'load previous fails', path: '/api/load/previous', ok: false },
  { name: 'load by index', path: '/api/load/index/1' },
  { name: 'load by id', path: '/api/load/id/xyz' },
  { name: 'load by cue', path: '/api/load/cue/3' },
  { name: 'load by cue fails', path: '/api/load/cue/3', ok: false },
  { name: 'load with nothing', path: '/api/load' },
  { name: 'pause', path: '/api/pause' },
  { name: 'stop', path: '/api/stop' },
  { name: 'reload', path: '/api/reload' },
  { name: 'roll', path: '/api/roll' },
  // added time
  { name: 'addtime', path: '/api/addtime/60000' },
  { name: 'addtime negative', path: '/api/addtime/-30000' },
  { name: 'addtime add', path: '/api/addtime/add/120000' },
  { name: 'addtime remove', path: '/api/addtime/remove/120000' },
  { name: 'addtime zero', path: '/api/addtime/0' },
  { name: 'addtime over an hour', path: '/api/addtime/3600001' },
  { name: 'addtime exactly an hour', path: '/api/addtime/-3600000' },
  { name: 'addtime not a number', path: '/api/addtime/soon' },
  { name: 'addtime with nothing', path: '/api/addtime' },
  { name: 'addtime from the query', path: '/api/addtime?add=5000' },
  // offset mode
  { name: 'offset mode relative', path: '/api/offsetmode/relative' },
  { name: 'offset mode absolute', path: '/api/offsetmode/absolute' },
  { name: 'offset mode unknown', path: '/api/offsetmode/sideways' },
  // the message overlay
  { name: 'message timer text', path: '/api/message/timer/text/hello' },
  { name: 'message timer text encoded', path: '/api/message/timer/text/hello%20world' },
  { name: 'message timer text reserved escape', path: '/api/message/timer/text/a%2Fb' },
  { name: 'message timer text malformed', path: '/api/message/timer/text/100%' },
  { name: 'message timer visible', path: '/api/message/timer/visible/true' },
  { name: 'message timer visible yes', path: '/api/message/timer/visible/yes' },
  { name: 'message timer blink 0', path: '/api/message/timer/blink/0' },
  { name: 'message timer blackout bad', path: '/api/message/timer/blackout/maybe' },
  { name: 'message timer secondary source', path: '/api/message/timer/secondarySource/aux2' },
  { name: 'message timer secondary source unknown', path: '/api/message/timer/secondarySource/aux9' },
  { name: 'message timer from the query', path: '/api/message/timer?text=hi+there&visible=1' },
  { name: 'message secondary', path: '/api/message/secondary/look%20up' },
  { name: 'message with nothing', path: '/api/message' },
  { name: 'message timer is not an object', path: '/api/message/timer' },
  // aux timers
  { name: 'auxtimer start', path: '/api/auxtimer/1/start' },
  { name: 'auxtimer pause', path: '/api/auxtimer/2/pause' },
  { name: 'auxtimer stop', path: '/api/auxtimer/3/stop' },
  { name: 'auxtimer bad command', path: '/api/auxtimer/1/explode' },
  { name: 'auxtimer bad index', path: '/api/auxtimer/4/start' },
  { name: 'auxtimer duration', path: '/api/auxtimer/1/duration/90000' },
  { name: 'auxtimer duration not a number', path: '/api/auxtimer/1/duration/long' },
  { name: 'auxtimer addtime', path: '/api/auxtimer/2/addtime/-5000' },
  { name: 'auxtimer direction', path: '/api/auxtimer/3/direction/count-up' },
  { name: 'auxtimer direction unknown', path: '/api/auxtimer/3/direction/sideways' },
  { name: 'auxtimer unknown patch', path: '/api/auxtimer/1/speed/2' },
  { name: 'auxtimer with nothing', path: '/api/auxtimer' },
  // screens
  { name: 'client rename', path: '/api/client?target=abc&rename=Stage' },
  { name: 'client redirect', path: '/api/client?target=abc&redirect=%2Ftimer' },
  { name: 'client identify from a string', path: '/api/client?target=abc&identify=true' },
  { name: 'client without a target', path: '/api/client?rename=Stage' },
  // editing an entry
  { name: 'change title', path: '/api/change/e1/title/New%20title' },
  { name: 'change note from the query', path: '/api/change/e1?note=hello+there' },
  { name: 'change cue', path: '/api/change/e1/cue/4' },
  { name: 'change skip', path: '/api/change/e1/skip/true' },
  { name: 'change flag bad', path: '/api/change/e1/flag/sometimes' },
  { name: 'change colour name', path: '/api/change/e1/colour/Red' },
  { name: 'change colour hex', path: '/api/change/e1/colour/%23FFAA00' },
  { name: 'change colour bad hex', path: '/api/change/e1/colour/%23FFAA0' },
  { name: 'change colour unknown name', path: '/api/change/e1/colour/blurple' },
  { name: 'change duration', path: '/api/change/e1/duration/900000' },
  { name: 'change duration too long', path: '/api/change/e1/duration/86400000' },
  { name: 'change duration negative', path: '/api/change/e1/duration/-1' },
  { name: 'change time start', path: '/api/change/e1/timeStart/37800000' },
  { name: 'change end action', path: '/api/change/e1/endAction/play-next' },
  { name: 'change end action unknown', path: '/api/change/e1/endAction/explode' },
  { name: 'change timer type', path: '/api/change/e1/timerType/clock' },
  { name: 'change time strategy', path: '/api/change/e1/timeStrategy/lock-end' },
  { name: 'change link start', path: '/api/change/e1/linkStart/1' },
  { name: 'change custom field', path: '/api/change/e1/custom:lighting/blue' },
  { name: 'change unknown custom field', path: '/api/change/e1/custom:smoke/heavy' },
  { name: 'change custom field of a delay', path: '/api/change/d1/custom:lighting/blue' },
  { name: 'change a field the entry lacks', path: '/api/change/d1/title/Late' },
  { name: 'change a field not permitted', path: '/api/change/e1/id/other' },
  { name: 'change group title', path: '/api/change/g1/title/Second%20block' },
  { name: 'change milestone cue', path: '/api/change/m1/cue/M2' },
  { name: 'change unknown entry', path: '/api/change/zz/title/Gone' },
  { name: 'change with only an id', path: '/api/change/e1' },
  { name: 'change with nothing', path: '/api/change' },
  // the teleprompter
  { name: 'teleprompter play', path: '/api/teleprompter/play' },
  { name: 'teleprompter speed', path: '/api/teleprompter/speed/120' },
  { name: 'teleprompter speed not a number', path: '/api/teleprompter/speed/fast' },
  { name: 'teleprompter goto index', path: '/api/teleprompter/goto/index/2' },
  { name: 'teleprompter goto loaded', path: '/api/teleprompter/goto/loaded' },
  { name: 'teleprompter unknown', path: '/api/teleprompter/rewind' },
];

const ws: WsCase[] = [
  { name: 'start with nothing', tag: 'start' },
  { name: 'start by index as a number', tag: 'start', payload: { index: 2 } },
  { name: 'start by id not a string', tag: 'start', payload: { id: 7 } },
  { name: 'start by cue as a number', tag: 'start', payload: { cue: 7 } },
  { name: 'start by cue not text', tag: 'start', payload: { cue: true } },
  { name: 'start index wins over id', tag: 'start', payload: { index: 1, id: 'x' } },
  { name: 'start with null', tag: 'start', payload: null },
  { name: 'start with a list', tag: 'start', payload: [] },
  { name: 'addtime as a number', tag: 'addtime', payload: 60000 },
  { name: 'addtime as a fraction', tag: 'addtime', payload: 1500.4 },
  { name: 'addtime add wins over remove', tag: 'addtime', payload: { add: 1000, remove: 2000 } },
  { name: 'addtime with null', tag: 'addtime', payload: null },
  { name: 'addtime with a list of one', tag: 'addtime', payload: [5000] },
  { name: 'message as the editor sends it', tag: 'message', payload: { timer: { text: 'Wrap up', visible: true } } },
  { name: 'message secondary source cleared', tag: 'message', payload: { timer: { secondarySource: null } } },
  { name: 'message secondary not text', tag: 'message', payload: { secondary: { a: 1 } } },
  { name: 'message timer null', tag: 'message', payload: { timer: null } },
  { name: 'message not an object', tag: 'message', payload: 'hello' },
  { name: 'auxtimer as the editor sends it', tag: 'auxtimer', payload: { '1': 'start' } },
  { name: 'auxtimer duration as a number', tag: 'auxtimer', payload: { '2': { duration: 300000 } } },
  { name: 'auxtimer index 0', tag: 'auxtimer', payload: { '0': 'start' } },
  { name: 'client identify', tag: 'client', payload: { target: 'abc', identify: true } },
  { name: 'client rename wins', tag: 'client', payload: { target: 'abc', rename: 'A', redirect: '/b' } },
  { name: 'client target not text', tag: 'client', payload: { target: 3, rename: 'A' } },
  { name: 'change with booleans', tag: 'change', payload: { e1: { skip: true, flag: false } } },
  { name: 'change with numbers', tag: 'change', payload: { e1: { timeWarning: 30000, timeDanger: 10000 } } },
  { name: 'change two custom fields', tag: 'change', payload: { e1: { 'custom:lighting': 'red', 'custom:sound': 7 } } },
  { name: 'change with an empty object', tag: 'change', payload: {} },
  { name: 'change with a number', tag: 'change', payload: { e1: 5 } },
  { name: 'change with null', tag: 'change', payload: { e1: null } },
  { name: 'change with text', tag: 'change', payload: { e1: 'title' } },
  { name: 'offset mode not text', tag: 'offsetmode', payload: 1 },
  { name: 'teleprompter speedBy', tag: 'teleprompter', payload: { speedBy: -20 } },
  { name: 'teleprompter goto cue', tag: 'teleprompter', payload: { goto: { cue: 3 } } },
  { name: 'teleprompter blackout', tag: 'teleprompter', payload: { blackout: 'toggle' } },
  { name: 'teleprompter mode', tag: 'teleprompter', payload: { mode: 'script' } },
];

function reset(ok: boolean) {
  record.calls = [];
  record.ok = ok;
  record.rundown = rundown;
  record.customFields = customFields;
}

const app = express();
app.use('/api', integrationRouter);
const server = app.listen(0);
await new Promise((resolve) => server.once('listening', resolve));
const { port } = server.address() as AddressInfo;

const httpResults = [];
for (const { name, path, ok = true } of http) {
  reset(ok);
  const response = await fetch(`http://127.0.0.1:${port}${path}`);
  // Express answers a path no route takes with its own HTML page
  const text = await response.text();
  const body = response.headers.get('content-type')?.includes('json') ? JSON.parse(text) : { html: true };
  httpResults.push({ name, path, ok, status: response.status, body, calls: record.calls });
}
server.close();

const wsResults = [];
for (const { name, tag, payload, ok = true } of ws) {
  reset(ok);
  try {
    const reply = await dispatchFromAdapter(tag, payload, 'ws');
    wsResults.push({ name, tag, payload, ok, reply, calls: record.calls });
  } catch (error) {
    wsResults.push({ name, tag, payload, ok, error: (error as Error).message, calls: record.calls });
  }
}

console.log(JSON.stringify({ rundown, customFields, http: httpResults, ws: wsResults }));
