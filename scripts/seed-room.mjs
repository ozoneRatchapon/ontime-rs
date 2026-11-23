#!/usr/bin/env node
// Seeds a room's rundown from a schedule file — one script for every event, the
// event itself lives in schedules/<name>.json (see schedules/example.json).
//
//   node scripts/seed-room.mjs schedules/<name>.json --dry      preview, writes nothing
//   node scripts/seed-room.mjs schedules/<name>.json            create the room/rundown
//   node scripts/seed-room.mjs schedules/<name>.json --replace  swap a rundown of the same title
//
// The master token comes from --token, ONTIME_TOKEN, or the Keychain item
// "ontime-session-password" — so normally there is nothing to type.
//
// --replace refuses while the room's timer is running (override with --live):
// the new rundown is built and loaded first, and only then is the old one deleted.

import { readFileSync } from 'node:fs';
import { parseArgs } from 'node:util';

import { DEFAULT_BASE, H, field_key, hhmm, read_playback, resolve_token, room_api } from './lib/ontime-client.mjs';
import { build_entries } from './lib/templates.mjs';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    base: { type: 'string' },
    room: { type: 'string' },
    token: { type: 'string' },
    dry: { type: 'boolean', default: false },
    replace: { type: 'boolean', default: false },
    live: { type: 'boolean', default: false },
  },
});

const [file] = positionals;
if (!file) {
  console.error('usage: seed-room.mjs schedules/<name>.json [--dry] [--replace] [--room R] [--base URL] [--token T]');
  process.exit(2);
}

const schedule = JSON.parse(readFileSync(file, 'utf8'));
const base = values.base ?? schedule.base ?? DEFAULT_BASE;
const room = values.room ?? schedule.room;
if (!room || !/^[A-Za-z0-9_-]{1,64}$/.test(room)) {
  console.error(`"${room}" is not a valid room name — letters, digits, - and _ only`);
  process.exit(2);
}
if (!schedule.rundown) {
  console.error('the schedule needs a "rundown" title');
  process.exit(2);
}

const entries = build_entries(schedule);
const fields = schedule.fields ?? [];

// every custom value must belong to a declared field, or it would silently vanish
for (const entry of entries) {
  for (const label of Object.keys(entry.custom)) {
    if (!fields.some((field) => field.label === label)) {
      throw new Error(`cue ${entry.cue} has a value for "${label}", which is not in "fields"`);
    }
  }
}

console.log(`${schedule.rundown}: ${entries.length} cues (${schedule.template ?? 'linked'}) → room "${room}" on ${base}`);

if (values.dry) {
  for (const entry of entries) {
    console.log(`  ${entry.cue.padEnd(4)} ${hhmm(entry.start)}–${hhmm(entry.end)} ${entry.pinned ? '⏱' : '↳'} ${entry.title}`);
  }
  console.log('  ⏱ pinned to the clock   ↳ follows the previous cue');
  process.exit(0);
}

const token = resolve_token(values.token);
const api = room_api(base, room, token);

const existing = await api('GET', '/data/rundowns');
const previous = existing.rundowns.filter((rundown) => rundown.title === schedule.rundown);
if (previous.length > 0 && !values.replace) {
  console.log(`  "${schedule.rundown}" already exists in ${room} — pass --replace to swap it`);
  process.exit(0);
}
if (previous.length > 0 && !values.live) {
  const playback = await read_playback(base, room, token);
  if (playback !== 'stop') {
    console.error(`  the timer in ${room} is "${playback}" — stop it first, or pass --live to replace anyway`);
    process.exit(1);
  }
}

await api('POST', '/data/settings', {
  timezoneOffset: (schedule.timezoneHours ?? 7) * H,
  timeFormat: schedule.timeFormat ?? '24',
});
if (schedule.project) {
  await api('POST', '/data/project', schedule.project);
}

const known_fields = (await api('GET', '/data/custom-fields')) ?? {};
for (const { label, colour } of fields) {
  if (field_key(label) in known_fields) continue;
  await api('POST', '/data/custom-fields', { type: 'text', label, colour: colour ?? '' });
}

const previous_ids = new Set(previous.map((rundown) => rundown.id));
const created = await api('POST', '/data/rundowns', { title: schedule.rundown });
const rid = created.rundowns.find((rundown) => rundown.title === schedule.rundown && !previous_ids.has(rundown.id)).id;

for (const entry of entries) {
  const custom = Object.fromEntries(Object.entries(entry.custom).map(([label, value]) => [field_key(label), value]));
  await api('POST', `/data/rundowns/${rid}/entry`, {
    type: 'event',
    cue: entry.cue,
    title: entry.title,
    note: entry.note,
    colour: entry.colour,
    duration: entry.end - entry.start,
    timeStrategy: 'lock-duration',
    ...(entry.pinned ? { timeStart: entry.start } : { linkStart: true }),
    timeWarning: entry.timeWarning,
    timeDanger: entry.timeDanger,
    custom,
  });
}

await api('POST', `/data/rundowns/${rid}/load`);

// only once the new one is loaded is the old one dropped
for (const rundown of previous) {
  await api('DELETE', `/data/rundowns/${rundown.id}`);
}

// a fresh room starts with a demo rundown; drop it while it is still untouched
const listed = await api('GET', '/data/rundowns');
const demo = listed.rundowns.find((rundown) => rundown.title === 'Default' && rundown.numEntries === 3);
if (demo && demo.id !== rid) {
  await api('DELETE', `/data/rundowns/${demo.id}`);
}

// who looks after the room, for the admin page's list. A Worker older than the
// room list answers 404 here, which is not a reason to fail the seed.
if (schedule.owner !== undefined || schedule.note !== undefined) {
  const labelled = await fetch(`${base}/admin/rooms/${room}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json', Authorization: `Bearer ${token}` },
    body: JSON.stringify({ owner: schedule.owner, note: schedule.note }),
  });
  // a label missing from the file is left as it is on the admin page
  const set = ['owner', 'note'].filter((key) => schedule[key] !== undefined).map((key) => `${key}: ${schedule[key]}`);
  console.log(labelled.ok ? `  room list — ${set.join(', ')}` : `  could not label the room (${labelled.status})`);
}

const loaded = await api('GET', `/data/rundowns/${rid}`);
let mismatches = 0;
for (const [index, id] of loaded.order.entries()) {
  const entry = loaded.entries[id];
  const planned = entries[index];
  const agrees = entry.timeStart === planned.start && entry.timeEnd === planned.end;
  if (!agrees) mismatches += 1;
  console.log(`  ${entry.cue.padEnd(4)} ${hhmm(entry.timeStart)}–${hhmm(entry.timeEnd)} ${agrees ? ' ' : '!'} ${entry.title}`);
}

const client = schedule.client ?? 'https://master.ontime-team.pages.dev';
const views = ['editor', 'cuesheet', 'timer', 'backstage'];
console.log(mismatches === 0 ? '  every time matches the schedule' : `  ${mismatches} disagree with the schedule`);
console.log(`  open: ${views.map((view) => `${client}/${view}?room=${room}`).join('\n        ')}`);
process.exit(mismatches === 0 ? 0 : 1);
