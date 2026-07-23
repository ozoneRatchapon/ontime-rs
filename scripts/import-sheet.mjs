#!/usr/bin/env node
// Imports a day's schedule from the Google Sheet into a rundown.
//
// The sheet's shape maps onto Ontime directly:
//   a row with a time range (07.30-08.30) is a block
//   the rows after it with only a duration are the activities inside that block
// so a block becomes a group and its activities become linked events, which is
// what makes the whole block move when one activity overruns.
//
//   node scripts/import-sheet.mjs <room> <gid> "<rundown title>" [--base URL] [--dry]

import { parseArgs } from 'node:util';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    base: { type: 'string', default: 'https://ontime-server-rs.example.workers.dev' },
    sheet: { type: 'string', default: 'YOUR_SHEET_ID' },
    dry: { type: 'boolean', default: false },
    token: { type: 'string' },
  },
});

const [room, gid, title] = positionals;
if (!room || !gid || !title) {
  console.error('usage: import-sheet.mjs <room> <gid> "<rundown title>" [--base URL] [--dry]');
  process.exit(2);
}

const H = 3600000;
const M = 60000;

// --- read the sheet --------------------------------------------------------

const csvUrl = `https://docs.google.com/spreadsheets/d/${values.sheet}/export?format=csv&gid=${gid}`;
const csv = await fetch(csvUrl).then((r) => {
  if (!r.ok) throw new Error(`could not read the sheet: ${r.status}`);
  return r.text();
});

/** A minimal CSV reader: fields may be quoted and may contain newlines. */
function parseCsv(text) {
  const rows = [];
  let row = [];
  let field = '';
  let quoted = false;

  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') { field += '"'; i++; }
      else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"') quoted = true;
    else if (c === ',') { row.push(field); field = ''; }
    else if (c === '\n') { row.push(field); rows.push(row); row = []; field = ''; }
    else if (c !== '\r') field += c;
  }
  if (field || row.length) { row.push(field); rows.push(row); }
  return rows;
}

const rows = parseCsv(csv);

// the header is the row naming the columns; anything above it is a sheet title
const headerIndex = rows.findIndex((r) => r[0]?.trim() === 'เวลา');
if (headerIndex === -1) throw new Error('no "เวลา" header row — is this a schedule tab?');

const header = rows[headerIndex].map((h) => h.trim());
const column = (name) => header.indexOf(name);
const columns = {
  time: column('เวลา'),
  minutes: column('นาที'),
  activity: column('กิจกรรม'),
  mic: column('Micหลัก') !== -1 ? column('Micหลัก') : column('Mic'),
  mic2: column('Micรอง'),
  demon: column('Demon'),
  place: column('สถานที่'),
  note: column('หมายเหตุ'),
};

/** "07.30-08.30" or "10.30-12.00 น." to milliseconds since midnight. */
function parseTimeRange(text) {
  const match = text.match(/(\d{1,2})[.:](\d{2})\s*-\s*(\d{1,2})[.:](\d{2})/);
  if (!match) return null;
  const [, h1, m1, h2, m2] = match.map(Number);
  return { start: h1 * H + m1 * M, end: h2 * H + m2 * M };
}

// --- turn the rows into blocks --------------------------------------------

const blocks = [];
for (const row of rows.slice(headerIndex + 1)) {
  const cell = (i) => (i >= 0 ? (row[i] ?? '').trim() : '');
  const activity = cell(columns.activity);
  const minutes = Number(cell(columns.minutes)) || 0;
  const range = parseTimeRange(cell(columns.time));

  if (!activity && !range) continue;

  const item = {
    title: activity || 'ไม่ระบุ',
    duration: minutes * M,
    mic: [cell(columns.mic), cell(columns.mic2)].filter(Boolean).join(' / '),
    demon: cell(columns.demon),
    place: cell(columns.place),
    // a note in the time column ("เลย 5 นาที") is a remark, not a time
    note: [cell(columns.note), range ? '' : cell(columns.time)].filter(Boolean).join(' — '),
  };

  if (range) {
    blocks.push({ ...item, start: range.start, end: range.end, children: [] });
  } else if (blocks.length) {
    blocks[blocks.length - 1].children.push(item);
  }
}

console.log(`${title}: ${blocks.length} blocks, ${blocks.reduce((n, b) => n + b.children.length, 0)} activities`);

if (values.dry) {
  for (const b of blocks) {
    const hhmm = (ms) => `${String(Math.floor(ms / H)).padStart(2, '0')}:${String(Math.floor((ms % H) / M)).padStart(2, '0')}`;
    console.log(`  ${hhmm(b.start)}–${hhmm(b.end)}  ${b.title}${b.children.length ? ` (${b.children.length})` : ''}`);
    for (const c of b.children) console.log(`      ${String(c.duration / M).padStart(3)}m  ${c.title}`);
  }
  process.exit(0);
}

// --- write it into the room -----------------------------------------------

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

// the sheet's extra columns become cuesheet columns. Labels are ASCII because
// the original validates them as alphanumeric-with-spaces.
for (const [label, colour] of [['Mic', '#3b82f6'], ['Demon', '#a855f7'], ['Place', '#22c55e'], ['Note', '#64748b']]) {
  await api('POST', '/data/custom-fields', { type: 'text', colour, label }).catch(() => {});
}

const existing = await api('GET', '/data/rundowns');
if (existing.rundowns.some((r) => r.title === title)) {
  console.log(`  "${title}" already exists — delete it first to re-import`);
  process.exit(0);
}

const created = await api('POST', '/data/rundowns', { title });
const rid = created.rundowns.filter((r) => r.title === title).pop().id;

const custom = (item) => {
  const fields = {};
  if (item.mic) fields.Mic = item.mic;
  if (item.demon) fields.Demon = item.demon;
  if (item.place) fields.Place = item.place;
  if (item.note) fields.Note = item.note;
  return fields;
};

for (const block of blocks) {
  if (block.children.length === 0) {
    await api('POST', `/data/rundowns/${rid}/entry`, {
      cue: '',
      title: block.title,
      timeStart: block.start,
      duration: block.end - block.start,
      timeEnd: block.end,
      timeWarning: 5 * M,
      timeDanger: 2 * M,
      custom: custom(block),
    });
    continue;
  }

  // the activities carry the timing; the block becomes a group around them so
  // the whole block shifts together when one of them overruns
  const childIds = [];
  let first = true;
  for (const child of block.children) {
    const entry = await api('POST', `/data/rundowns/${rid}/entry`, {
      cue: '',
      title: child.title,
      duration: child.duration,
      ...(first ? { timeStart: block.start } : { linkStart: true }),
      timeWarning: 2 * M,
      timeDanger: 1 * M,
      custom: custom(child),
    });
    childIds.push(entry.id);
    first = false;
  }
  await api('POST', `/data/rundowns/${rid}/group`, { ids: childIds });
  // name the group after the block it came from
  const current = await api('GET', `/data/rundowns/${rid}`);
  const groupId = current.order.find((id) => current.entries[id].type === 'group' && !current.entries[id].title);
  if (groupId) {
    await api('PUT', `/data/rundowns/${rid}/entry`, { id: groupId, title: block.title });
  }
}

const loaded = await api('GET', `/data/rundowns/${rid}`);
console.log(`  imported ${Object.keys(loaded.entries).length} entries into "${title}"`);
