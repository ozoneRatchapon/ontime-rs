#!/usr/bin/env node
// A copy of every room on this machine, free (plan 018).
//
//   node scripts/backup-rooms.mjs [--base URL] [--out DIR] [--keep DAYS]
//
// For each room on the admin list it saves what `POST /data/db/download`
// returns, which is what `POST /data/db/restore` takes back: the schedule,
// settings, report and run archive, without the password hash. Files go to
// <out>/<YYYY-MM-DD>/<room>.json, readable by this user only, and dated folders
// older than --keep days are removed.
//
// The default folder is outside the repository on purpose: these are real
// events, and the repository's origin is public.
//
// Cost: one request for the list and one per room, a few hundred at most, out
// of 100,000 a day. scripts/install-backup-agent.sh runs it every night.
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';

import { DEFAULT_BASE, resolve_token } from './lib/ontime-client.mjs';

const { values } = parseArgs({
  options: {
    base: { type: 'string', default: DEFAULT_BASE },
    out: { type: 'string', default: path.join(os.homedir(), 'OntimeBackups') },
    keep: { type: 'string', default: '30' },
  },
});
const keepDays = Number(values.keep);
const token = resolve_token();
const auth = { Authorization: `Bearer ${token}` };

const listed = await fetch(`${values.base}/admin/rooms`, { headers: auth });
if (!listed.ok) {
  console.error(`could not list rooms: ${listed.status} ${await listed.text()}`);
  process.exit(1);
}
const body = await listed.json();
const rooms = (body.rooms ?? body).map((room) => room.name).filter(Boolean);

const day = new Date().toISOString().slice(0, 10);
const folder = path.join(values.out, day);
await fs.mkdir(folder, { recursive: true, mode: 0o700 });

let saved = 0;
const failed = [];
for (const room of rooms) {
  try {
    const response = await fetch(`${values.base}/r/${encodeURIComponent(room)}/data/db/download`, {
      method: 'POST',
      headers: auth,
    });
    if (!response.ok) throw new Error(`${response.status} ${await response.text()}`);
    const snapshot = await response.text();
    JSON.parse(snapshot); // a file that would not restore is not a backup
    await fs.writeFile(path.join(folder, `${room}.json`), snapshot, { mode: 0o600 });
    saved += 1;
  } catch (error) {
    failed.push(`${room}: ${error.message}`);
  }
}

// keep the last --keep days; a folder that is not a date is not ours to remove
const cutoff = Date.now() - keepDays * 86_400_000;
for (const entry of await fs.readdir(values.out, { withFileTypes: true })) {
  if (!entry.isDirectory() || !/^\d{4}-\d{2}-\d{2}$/.test(entry.name)) continue;
  if (Date.parse(entry.name) < cutoff) await fs.rm(path.join(values.out, entry.name), { recursive: true });
}

console.log(`${new Date().toISOString()} saved ${saved} of ${rooms.length} rooms to ${folder}`);
for (const line of failed) console.log(`  failed ${line}`);
process.exit(failed.length > 0 ? 1 : 0);
