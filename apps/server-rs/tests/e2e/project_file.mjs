// Project files to and from the Ontime app (plan 020): a file the app saved
// takes the place of a room's show, and the room downloads as a file the app
// opens. The file is upstream's demo project, as the contract fixture has it.
//
//   ADMIN_PASSWORD=admin-e2e node project_file.mjs http://127.0.0.1:8787
import { readFileSync } from 'node:fs';

const base = process.argv[2] ?? 'http://127.0.0.1:8787';
const password = process.env.ADMIN_PASSWORD;
if (!password) {
  console.log("  skipped: set ADMIN_PASSWORD to the worker's SESSION_PASSWORD to run it");
  process.exit(0);
}

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};

const fixture = JSON.parse(
  readFileSync(new URL('../../../../packages/ontime-core/tests/fixtures/project_file.json', import.meta.url), 'utf8'),
);
const demo = fixture.cases.find((c) => c.name === 'the demo project');

const login = await fetch(`${base}/admin/login`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ password }),
});
const master = (await login.json()).token;
const room = `file-${Math.random().toString(36).slice(2, 8)}`;
const call = (method, path, body, token = master) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => ({ status: r.status, body: await r.json().catch(() => null) }));

try {
  console.log('== a file from the app ==');
  await call('POST', '/data/rundowns', { title: 'Before' });
  const imported = await call('POST', '/data/db/import', { file: demo.file });
  check('import answers', imported.status, 200);
  check('nothing in the demo is left out', imported.body?.warnings, []);
  const project = await call('GET', '/data/project');
  check('the project details are the file’s', project.body?.title, 'Ontime Demo Project');
  const list = (await call('GET', '/data/rundowns')).body;
  check(
    'its rundowns, and only those',
    list.rundowns.map((r) => r.id).sort(),
    Object.keys(demo.data.rundowns).sort(),
  );
  check('the first one in the file is loaded', list.loaded, demo.data.firstRundown);

  console.log('== the room as a file for the app ==');
  const exported = await call('POST', '/data/db/export');
  check('export answers', exported.status, 200);
  // upstream's demo has no id; the room makes one, and keeps it from then on
  check('the file has a project id', typeof exported.body?.projectId === 'string' && exported.body.projectId.length > 0, true);
  check(
    'only what a project file carries',
    Object.keys(exported.body ?? {}).sort(),
    ['automation', 'customFields', 'project', 'projectId', 'rundowns', 'settings', 'teleprompter', 'urlPresets', 'viewSettings'],
  );
  const again = await call('POST', '/data/db/import', { file: JSON.stringify(exported.body) });
  check('it imports back', again.status, 200);
  check('and exports the same', (await call('POST', '/data/db/export')).body, exported.body);

  console.log('== what is not a project file ==');
  const v3 = await call('POST', '/data/db/import', { file: JSON.stringify({ settings: { version: '3.14.0' } }) });
  check('a v3 file is refused', v3.status, 400);
  check('with a way forward', /Open it in the Ontime app/.test(v3.body?.message ?? ''), true);
  check('the show is untouched', (await call('GET', '/data/project')).body?.title, 'Ontime Demo Project');
  check('nor is broken JSON taken', (await call('POST', '/data/db/import', { file: '{' })).status, 400);

  console.log('== it can be undone, and it is logged ==');
  const history = (await call('GET', '/data/snapshots')).body?.snapshots ?? [];
  check('each import copied the show first', history.length >= 2, true);
  const log = (await call('GET', '/data/activity')).body;
  check(
    'logged as an import',
    log.lines.filter((line) => line.action === 'project-import').length,
    2,
  );
} finally {
  await fetch(`${base}/admin/rooms/${room}`, { method: 'DELETE', headers: { Authorization: `Bearer ${master}` } });
}

console.log(failed ? '\nSOME CHECKS FAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
