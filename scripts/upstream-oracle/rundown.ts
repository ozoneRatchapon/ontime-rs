// Shared by the generators: building rundowns, and processing them exactly the
// way upstream's server does before anything reads their times.
import { makeOntimeEvent } from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import { makeRundownMetadata } from '@upstream/apps/server/src/api-data/rundown/rundown.parser.ts';
import { isOntimeEvent, isOntimeGroup, isPlayableEvent } from 'ontime-types';

export const MIN = 60_000;
export const H = 60 * MIN;

// upstream's processRundown, copied from rundown.dao.ts so the database layer stays out
export function processRundown(rundown: any) {
  const { process, getMetadata } = makeRundownMetadata({});
  for (const id of rundown.order) {
    const entry = rundown.entries[id];
    if (!entry) continue;
    const processed: any = process(entry, null);
    if (!isOntimeGroup(processed)) continue;
    let groupStartTime: number | null = null;
    let groupEndTime: number | null = null;
    let isFirstLinked = false;
    const groupEvents: string[] = [];
    processed.duration = 0;
    for (let j = 0; j < processed.entries.length; j++) {
      const nested = rundown.entries[processed.entries[j]];
      if (!nested) continue;
      groupEvents.push(nested.id);
      const processedNested: any = process(nested, processed.id);
      if (!isOntimeEvent(processedNested) || !isPlayableEvent(processedNested)) continue;
      if (groupStartTime === null) {
        groupStartTime = processedNested.timeStart;
        isFirstLinked = Boolean(processedNested.linkStart);
      }
      groupEndTime = processedNested.timeEnd;
      if (j > 0) processed.duration += processedNested.gap;
      processed.duration += processedNested.duration;
    }
    processed.timeStart = groupStartTime;
    processed.timeEnd = groupEndTime;
    processed.isFirstLinked = isFirstLinked;
    processed.entries = groupEvents;
  }
  return getMetadata();
}

export const ev = (id: string, timeStart: number, duration: number, patch: Record<string, unknown> = {}) =>
  makeOntimeEvent({
    id,
    cue: id,
    timeStart,
    duration,
    timeEnd: (timeStart + duration) % (24 * H),
    timeStrategy: 'lock-duration',
    ...patch,
  });

export const rundownOf = (top: any[], nested: any[] = []) => ({
  id: 'show',
  title: 'Show',
  order: top.map((e) => e.id),
  flatOrder: [],
  entries: Object.fromEntries([...top, ...nested].map((e) => [e.id, e])),
  revision: 1,
});
