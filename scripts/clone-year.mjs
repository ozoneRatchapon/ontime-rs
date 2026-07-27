#!/usr/bin/env node
// Clones every rundown of one year into a new year, ready to adjust.
//
// The 4ever room keeps one rundown per day per year. Next year you want last
// year's days as the starting point — this duplicates each rundown whose title
// carries <from> and renames the copy to <to>, in one command. Duplication mints
// fresh ids for every entry, so the copies share nothing with the originals and
// last year's archived run stays intact. Re-running is safe: a target that
// already exists is skipped.
//
//   node scripts/clone-year.mjs <room> <from> <to> [--base URL] [--token T] [--dry]
//
// e.g. node scripts/clone-year.mjs demo_room 2569 2570

import { parseArgs } from 'node:util';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    base: { type: 'string', default: 'https://ontime-server-rs.example.workers.dev' },
    token: { type: 'string' },
    dry: { type: 'boolean', default: false },
  },
});

const [room, from, to] = positionals;
if (!room || !from || !to) {
  console.error('usage: clone-year.mjs <room> <from-year> <to-year> [--base URL] [--token T] [--dry]');
  process.exit(2);
}

const api = (method, path, body) =>
  fetch(`${values.base}/r/${room}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(values.token ? { Authorization: `Bearer ${values.token}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  }).then(async (r) => {
    if (r.status >= 400) throw new Error(`${method} ${path} -> ${r.status} ${await r.text()}`);
    return r.status === 204 ? null : r.json().catch(() => null);
  });

const list = await api('GET', '/data/rundowns');
const sources = list.rundowns
  .filter((r) => r.title.includes(from))
  .sort((x, y) => x.title.localeCompare(y.title, 'th'));

if (sources.length === 0) {
  console.error(`no rundowns whose title contains "${from}" in room "${room}"`);
  process.exit(1);
}

console.log(`${room}: cloning ${sources.length} rundown(s) ${from} → ${to}`);

const existing = new Set(list.rundowns.map((r) => r.title));
let currentIds = list.rundowns.map((r) => r.id);

for (const src of sources) {
  const newTitle = src.title.split(from).join(to);
  if (existing.has(newTitle)) {
    console.log(`  skip   "${newTitle}" (already exists)`);
    continue;
  }
  if (values.dry) {
    console.log(`  would  "${src.title}"  →  "${newTitle}"`);
    continue;
  }
  // duplicate, then find the rundown that appeared and give it the new name
  const before = new Set(currentIds);
  const after = (await api('POST', `/data/rundowns/${src.id}/duplicate`)).rundowns;
  const copy = after.find((r) => !before.has(r.id));
  if (!copy) throw new Error(`could not find the duplicate of "${src.title}"`);
  await api('PATCH', `/data/rundowns/${copy.id}`, { title: newTitle });
  existing.add(newTitle);
  currentIds = after.map((r) => r.id);
  console.log(`  ✓      "${src.title}"  →  "${newTitle}"`);
}

if (!values.dry) {
  const total = (await api('GET', '/data/rundowns')).rundowns.length;
  console.log(`done. room now has ${total} rundowns — open Manage Rundowns to adjust the new year.`);
}
