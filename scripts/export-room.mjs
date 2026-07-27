#!/usr/bin/env node
// Exports everything a room holds — schedule, the live report, and the permanent
// run archive — to a JSON snapshot and a readable planned-vs-actual table.
//
//   node scripts/export-room.mjs <room> [--base URL] [--out DIR] [--token T]
//
// Use it to pull a season's history for analysis, or to keep an off-server copy.
import { parseArgs } from 'node:util';
import fs from 'node:fs/promises';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    base: { type: 'string', default: 'https://ontime-server-rs.example.workers.dev' },
    out: { type: 'string', default: 'archives' },
    token: { type: 'string' },
  },
});
const room = positionals[0];
if (!room) {
  console.error('usage: export-room.mjs <room> [--base URL] [--out DIR] [--token T]');
  process.exit(2);
}

const H = 3600000, M = 60000;
const hhmm = ms => ms == null ? '—' : `${Math.floor(ms / H) % 24}`.padStart(2, '0') + ':' + `${Math.floor((ms % H) / M)}`.padStart(2, '0');
const api = p => fetch(`${values.base}/r/${room}${p}`, {
  headers: values.token ? { Authorization: `Bearer ${values.token}` } : {},
}).then(r => {
  if (!r.ok) throw new Error(`${p} -> ${r.status}`);
  return r.json();
});

const list = await api('/data/rundowns');
const report = await api('/data/report');
const archive = await api('/data/report/archive');
const settings = await api('/data/settings');
const project = await api('/data/project');

const rundowns = {};
for (const r of list.rundowns) rundowns[r.title] = await api(`/data/rundowns/${r.id}`);

const snapshot = { room, exportedFrom: values.base, project, settings, rundowns, report, reportArchive: archive };
await fs.mkdir(values.out, { recursive: true });
await fs.writeFile(`${values.out}/${room}.json`, JSON.stringify(snapshot, null, 2));

// readable: each archived run as its own planned-vs-actual table
let md = `# ${project.title || room}\n\nห้อง \`${room}\` · export ${new Date().toISOString().slice(0, 10)}\n\n`;
md += `rundowns: ${list.rundowns.length} · live report: ${Object.keys(report).length} · archived runs: ${archive.length}\n\n`;
for (const run of archive) {
  md += `## ${run.rundownTitle} — เล่นจริง (archived)\n\n| กิจกรรม | แผน | จริง | ต่าง |\n|---|---|---|---|\n`;
  for (const e of run.events) {
    let diff = '';
    if (e.startedAt != null && e.endedAt != null && e.plannedDuration > 0) {
      const d = Math.round(((e.endedAt - e.startedAt) - e.plannedDuration) / M);
      diff = d === 0 ? '±0' : (d > 0 ? `+${d}m` : `${d}m`);
    }
    md += `| ${(e.title || '?').replace(/\|/g, '/')} | ${Math.round(e.plannedDuration / M)}m | ${hhmm(e.startedAt)}–${hhmm(e.endedAt)} | ${diff} |\n`;
  }
  md += `\n`;
}
if (archive.length === 0) md += `_(ยังไม่มี archived run — ประวัติการเล่นจริงจะถูกเก็บเมื่อสลับ rundown หรือกดบันทึกเอง)_\n`;
await fs.writeFile(`${values.out}/${room}.md`, md);

console.log(`${values.out}/${room}.json  (${JSON.stringify(snapshot).length} bytes)`);
console.log(`${values.out}/${room}.md    (${archive.length} archived runs)`);
