// Templates turn a schedule's rows into rundown entries.
//
// An entry is { cue, title, note, colour, start, end, pinned, timeWarning,
// timeDanger, custom } with times in ms since midnight. A pinned entry carries a
// wall-clock start; an unpinned one is linked to the entry before it, so an
// overrun moves it along.

import { M, hhmm, parse_hhmm } from './ontime-client.mjs';

/** A long event can afford a longer warning than a short one. */
export function thresholds_for(duration) {
  if (duration >= 30 * M) return { timeWarning: 5 * M, timeDanger: 2 * M };
  if (duration >= 10 * M) return { timeWarning: 3 * M, timeDanger: 1 * M };
  return { timeWarning: 2 * M, timeDanger: 1 * M };
}

/** Arrays in custom values become one bullet-separated line. */
function custom_of(row) {
  const custom = {};
  for (const [label, value] of Object.entries(row.custom ?? {})) {
    if (value === undefined || value === null || value === '') continue;
    custom[label] = Array.isArray(value) ? value.join(' • ') : String(value);
  }
  return custom;
}

/**
 * One unbroken track: only the first row keeps its wall-clock start, the rest
 * follow. Every row must start where the previous one ends.
 */
function linked(schedule) {
  let cursor = null;
  return schedule.rows.map((row, index) => {
    const start = parse_hhmm(row.start);
    const end = parse_hhmm(row.end);
    if (end <= start) throw new Error(`row ${index + 1} ("${row.title}") ends before it starts`);
    if (cursor !== null && start !== cursor) {
      throw new Error(
        `row ${index + 1} ("${row.title}") starts at ${hhmm(start)}, but the previous one ends at ${hhmm(cursor)}`,
      );
    }
    cursor = end;
    return {
      cue: row.cue ?? String(index + 1).padStart(2, '0'),
      title: row.title,
      note: row.note ?? '',
      colour: row.colour ?? schedule.colour ?? '',
      start,
      end,
      pinned: index === 0,
      ...thresholds_for(end - start),
      custom: custom_of(row),
    };
  });
}

const EXAM_PARTS = [
  { label: 'นำเสนอผลงาน (Presentation)', minutes: 7, colour: '#f4a582' },
  { label: 'ถาม-ตอบ & ข้อเสนอแนะจากผู้ทรงคุณวุฒิ', minutes: 10, colour: '#92c5de' },
];

/**
 * A fixed slot per row, run as consecutive parts (presentation then Q&A by
 * default; override with schedule.parts). The first part is pinned to the
 * slot's start, so a student overrunning never moves the next student; what is
 * left of the slot stays a gap for scoring and changeover.
 */
function exam_slot(schedule) {
  const parts = schedule.parts ?? EXAM_PARTS;
  const used = parts.reduce((sum, part) => sum + part.minutes * M, 0);

  return schedule.rows.flatMap((row, index) => {
    const slot_start = parse_hhmm(row.start);
    const slot_end = parse_hhmm(row.end);
    if (used > slot_end - slot_start) {
      throw new Error(
        `row ${index + 1} ("${row.title}"): parts take ${used / M} min but the slot is ${(slot_end - slot_start) / M} min`,
      );
    }
    const cue = row.cue ?? String(index + 1);
    let cursor = slot_start;
    return parts.map((part, part_index) => {
      const duration = part.minutes * M;
      const entry = {
        cue: `${cue}.${part_index + 1}`,
        title: `${row.title} — ${part.label}`,
        note: row.note ?? '',
        colour: part.colour ?? schedule.colour ?? '',
        start: cursor,
        end: cursor + duration,
        pinned: part_index === 0,
        ...thresholds_for(duration),
        custom: custom_of(row),
      };
      cursor = entry.end;
      return entry;
    });
  });
}

export const TEMPLATES = { linked, 'exam-slot': exam_slot };

export function build_entries(schedule) {
  const template = TEMPLATES[schedule.template ?? 'linked'];
  if (!template) {
    throw new Error(`unknown template "${schedule.template}" — one of: ${Object.keys(TEMPLATES).join(', ')}`);
  }
  if (!Array.isArray(schedule.rows) || schedule.rows.length === 0) {
    throw new Error('the schedule has no rows');
  }
  return template(schedule);
}
