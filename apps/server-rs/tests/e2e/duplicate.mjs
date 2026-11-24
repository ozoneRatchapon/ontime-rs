// Duplicating a rundown must produce a fully independent copy: the same schedule
// under brand-new ids. The room-wide report is keyed by entry id, so a copy that
// kept the originals' ids would share their report slots — running the copy would
// overwrite the original's recorded times. That is the regression this guards.
const base = process.argv[2] ?? 'http://127.0.0.1:8787';

let failed = false;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  if (!ok) failed = true;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${label}${ok ? '' : ` -> got ${JSON.stringify(actual)} want ${JSON.stringify(expected)}`}`,
  );
};

// a fresh random room each run, so repeats never collide on a deployment
const room = 'dup-' + Math.random().toString(36).slice(2, 10);
const api = (method, path, body) =>
  fetch(`${base}/r/${room}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => (r.status === 204 ? null : r.json().catch(() => null)));

console.log('== a rundown with a group of two events ==');
const rid = (await api('GET', '/data/rundowns')).loaded;
await api('DELETE', `/data/rundowns/${rid}/all`);
const a = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'A', duration: 60000 });
const b = await api('POST', `/data/rundowns/${rid}/entry`, { title: 'B', duration: 60000 });
await api('POST', `/data/rundowns/${rid}/group`, { ids: [a.id, b.id] });
const sourceIds = new Set(Object.keys((await api('GET', `/data/rundowns/${rid}`)).entries));
check('source has a group + 2 events', sourceIds.size, 3);

console.log('== duplicate it ==');
const list = await api('POST', `/data/rundowns/${rid}/duplicate`);
const copyMeta = list.rundowns.find((r) => r.title.endsWith('(copy)'));
check('a "(copy)" rundown appears', !!copyMeta, true);
const copy = await api('GET', `/data/rundowns/${copyMeta.id}`);
const copyIds = Object.keys(copy.entries);

console.log('== the copy is a fully independent duplicate ==');
check('same number of entries', copyIds.length, sourceIds.size);
check('shares no entry id with the source', copyIds.filter((id) => sourceIds.has(id)).length, 0);
const group = Object.values(copy.entries).find((e) => e.type === 'group');
check('the copy carries its group', !!group, true);
check(
  'group children exist in the copy',
  group.entries.every((id) => !!copy.entries[id]),
  true,
);
check(
  'children point back at the copy group',
  group.entries.every((id) => copy.entries[id].parent === group.id),
  true,
);
check(
  'schedule preserved in order',
  copy.flatOrder.map((id) => copy.entries[id].title),
  ['', 'A', 'B'],
);

console.log('== editing the copy leaves the original untouched ==');
await api('PUT', `/data/rundowns/${copyMeta.id}/entry`, { id: copy.flatOrder[1], title: 'CHANGED' });
const sourceTitles = Object.values((await api('GET', `/data/rundowns/${rid}`)).entries)
  .map((e) => e.title)
  .sort();
check('the source still reads A, B (+ its group)', sourceTitles, ['', 'A', 'B']);

// tidy up so a re-run against a deployment leaves nothing behind
await api('DELETE', `/data/rundowns/${copyMeta.id}`);

console.log(failed ? '\nFAILED' : '\nALL CHECKS PASSED');
process.exit(failed ? 1 : 0);
