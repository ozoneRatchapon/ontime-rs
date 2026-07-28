#!/usr/bin/env node
// Restores a room from a backup produced by the "Download project" button in
// settings (or GET-ish POST /data/db/download). The room keeps its own password;
// playback starts stopped rather than resuming from when the snapshot was taken.
//
//   node scripts/restore-room.mjs <room> <backup.json> [--base URL] [--token T]

import { parseArgs } from 'node:util';
import { readFile } from 'node:fs/promises';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    base: { type: 'string', default: 'https://ontime-server-rs.example.workers.dev' },
    token: { type: 'string' },
  },
});

const [room, file] = positionals;
if (!room || !file) {
  console.error('usage: restore-room.mjs <room> <backup.json> [--base URL] [--token T]');
  process.exit(2);
}

const backup = JSON.parse(await readFile(file, 'utf8'));

const res = await fetch(`${values.base}/r/${room}/data/db/restore`, {
  method: 'POST',
  headers: {
    'content-type': 'application/json',
    ...(values.token ? { Authorization: `Bearer ${values.token}` } : {}),
  },
  body: JSON.stringify(backup),
});

if (!res.ok) {
  console.error(`restore failed: ${res.status} ${await res.text()}`);
  process.exit(1);
}

const rundowns = Object.keys(backup.rundowns ?? {}).length;
const runs = (backup.reportArchive ?? []).length;
console.log(`restored "${room}" — ${rundowns} rundowns, ${runs} archived runs`);
