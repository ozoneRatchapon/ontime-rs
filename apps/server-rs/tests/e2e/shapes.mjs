// Asserts the *shape* of every response the client destructures.
//
// A 200 is not enough: /data/db/all returned 200 with the wrong body and crashed
// the cuesheet on `files.length`. Each entry below names the fields the client
// actually reads, taken from packages/types.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, ok, detail = '') => {
  if (!ok) failed = true;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok || !detail ? '' : ` -> ${detail}`}`);
};

const get = (path) => fetch(`${base}${path}`).then(async (r) => ({ status: r.status, body: await r.json().catch(() => null) }));

/** Every field the client reads, with the type it expects. */
const contracts = [
  ['/data/db/all', { files: 'array', lastLoadedProject: 'string' }],
  ['/data/settings/serverport', { port: 'number', pendingRestart: 'boolean' }],
  ['/data/session/info', { networkInterfaces: 'array', version: 'string', serverPort: 'number', publicDir: 'string' }],
  ['/data/custom-views', { views: 'array' }],
  ['/data/settings', { version: 'string', timeFormat: 'string', language: 'string' }],
  ['/data/project', { title: 'string', description: 'string', url: 'string', info: 'string', custom: 'array' }],
  ['/data/view-settings', { overrideStyles: 'boolean', normalColor: 'string', warningColor: 'string', dangerColor: 'string' }],
  ['/data/rundowns', { loaded: 'string', rundowns: 'array' }],
  ['/data/rundowns/current', { id: 'string', order: 'array', flatOrder: 'array', entries: 'object', revision: 'number' }],
  ['/data/automations', { enabledAutomations: 'boolean', triggers: 'array', automations: 'object' }],
  ['/data/url-presets', null], // a bare array
  ['/data/report', null],
  ['/data/report/archive', null], // append-only run history — a bare array // a bare record
  ['/user/translations/translations.json', null],
];

const typeOf = (value) => (Array.isArray(value) ? 'array' : value === null ? 'null' : typeof value);

for (const [path, contract] of contracts) {
  const { status, body } = await get(path);
  if (status !== 200) {
    check(`${path} responds 200`, false, `status ${status}`);
    continue;
  }
  if (!contract) {
    check(`${path} returns json`, body !== null, `got ${JSON.stringify(body)}`);
    continue;
  }
  const wrong = Object.entries(contract)
    .filter(([field, expected]) => typeOf(body?.[field]) !== expected)
    .map(([field, expected]) => `${field}: expected ${expected}, got ${typeOf(body?.[field])}`);

  check(`${path} matches its type`, wrong.length === 0, wrong.join('; '));
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
