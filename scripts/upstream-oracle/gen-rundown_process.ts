// The rundown processing contract: times, gaps, delays and days, as upstream's
// server computes them (rundown.parser.ts makeRundownMetadata, driven the way
// rundown.dao.ts processRundown drives it). Our Rundown::recompute must agree.
import {
  makeOntimeDelay,
  makeOntimeEvent,
  makeOntimeGroup,
  makeOntimeMilestone,
} from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import { makeRundownMetadata } from '@upstream/apps/server/src/api-data/rundown/rundown.parser.ts';
import { isOntimeEvent, isOntimeGroup, isPlayableEvent } from 'ontime-types';

const MIN = 60_000;
const H = 60 * MIN;

// upstream's processRundown, copied from rundown.dao.ts so the database layer stays out
function processRundown(rundown: any) {
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

const ev = (id: string, timeStart: number, duration: number, patch: Record<string, unknown> = {}) =>
  makeOntimeEvent({ id, cue: id, timeStart, duration, timeEnd: (timeStart + duration) % (24 * H), timeStrategy: 'lock-duration', ...patch });
const rundownOf = (top: any[], nested: any[] = []) => ({
  id: 'show',
  title: 'Show',
  order: top.map((e) => e.id),
  flatOrder: [],
  entries: Object.fromEntries([...top, ...nested].map((e) => [e.id, e])),
  revision: 1,
});

// Every case keeps timeStart, duration and timeEnd consistent: upstream derives
// them when an edit is validated, not here (see rundown_times.rs).
const cases: [string, ReturnType<typeof rundownOf>][] = [
  ['back to back', rundownOf([ev('a', 9 * H, 30 * MIN), ev('b', 9 * H + 30 * MIN, 30 * MIN)])],
  ['a gap between events', rundownOf([ev('a', 9 * H, 30 * MIN), ev('b', 10 * H, 30 * MIN)])],
  ['an overlap', rundownOf([ev('a', 9 * H, 60 * MIN), ev('b', 9 * H + 30 * MIN, 60 * MIN)])],
  ['a linked chain', rundownOf([ev('a', 9 * H, 30 * MIN), ev('b', 0, 20 * MIN, { linkStart: true }), ev('c', 0, 10 * MIN, { linkStart: true })])],
  ['a link with nothing before it is dropped', rundownOf([ev('a', 9 * H, 30 * MIN, { linkStart: true })])],
  ['a link skips a skipped event', rundownOf([ev('a', 9 * H, 30 * MIN), ev('s', 12 * H, 60 * MIN, { skip: true }), ev('b', 0, 20 * MIN, { linkStart: true })])],
  ['a skipped event takes no gap or day', rundownOf([ev('a', 22 * H, 60 * MIN), ev('s', 1 * H, 60 * MIN, { skip: true }), ev('b', 23 * H + 30 * MIN, 10 * MIN)])],
  ['crossing midnight', rundownOf([ev('a', 23 * H, 30 * MIN), ev('b', 23 * H + 45 * MIN, 30 * MIN), ev('c', 0 * H + 30 * MIN, 30 * MIN)])],
  ['an event past midnight on its own', rundownOf([ev('a', 23 * H + 30 * MIN, 60 * MIN), ev('b', 1 * H, 30 * MIN)])],
  ['a delay accumulates', rundownOf([ev('a', 9 * H, 30 * MIN), makeOntimeDelay({ id: 'd', duration: 10 * MIN }), ev('b', 9 * H + 30 * MIN, 30 * MIN), ev('c', 10 * H, 30 * MIN)])],
  ['a gap absorbs a delay', rundownOf([ev('a', 9 * H, 30 * MIN), makeOntimeDelay({ id: 'd', duration: 10 * MIN }), ev('b', 9 * H + 30 * MIN, 30 * MIN), ev('c', 10 * H + 15 * MIN, 30 * MIN), ev('e', 11 * H, 30 * MIN)])],
  ['a negative delay', rundownOf([ev('a', 9 * H, 30 * MIN), makeOntimeDelay({ id: 'd', duration: -5 * MIN }), ev('b', 10 * H, 30 * MIN)])],
  ['a milestone takes no time', rundownOf([ev('a', 9 * H, 30 * MIN), makeOntimeMilestone({ id: 'm', cue: 'M' }), ev('b', 9 * H + 30 * MIN, 30 * MIN)])],
  ['a group spans its events', rundownOf([makeOntimeGroup({ id: 'g', entries: ['a', 'b'] })], [ev('a', 9 * H, 30 * MIN, { parent: 'g' }), ev('b', 10 * H, 30 * MIN, { parent: 'g' })])],
  ['a group whose first event is linked', rundownOf([ev('x', 8 * H, 30 * MIN), makeOntimeGroup({ id: 'g', entries: ['a', 'b'] })], [ev('a', 0, 30 * MIN, { parent: 'g', linkStart: true }), ev('b', 9 * H, 30 * MIN, { parent: 'g' })])],
  ['an empty group', rundownOf([ev('a', 9 * H, 30 * MIN), makeOntimeGroup({ id: 'g', entries: [] }), ev('b', 10 * H, 30 * MIN)])],
  ['count to end', rundownOf([ev('a', 9 * H, 30 * MIN, { countToEnd: true }), ev('b', 9 * H + 30 * MIN, 30 * MIN)])],
  ['a link after a delay', rundownOf([ev('a', 9 * H, 30 * MIN), makeOntimeDelay({ id: 'd', duration: 10 * MIN }), ev('b', 0, 20 * MIN, { linkStart: true })])],
];

const fields = ['timeStart', 'timeEnd', 'duration', 'gap', 'delay', 'dayOffset', 'linkStart'];
const out = cases.map(([name, rundown]) => {
  const input = structuredClone(rundown);
  const meta: any = processRundown(rundown);
  const entries = Object.fromEntries(
    Object.entries(meta.entries).map(([id, e]: [string, any]) => [
      id,
      Object.fromEntries(['type', ...fields].filter((k) => k in e).map((k) => [k, e[k]])),
    ]),
  );
  return { name, rundown: input, entries, totalDelay: meta.totalDelay, totalDuration: meta.totalDuration, totalDays: meta.totalDays };
});
process.stdout.write(JSON.stringify(out));
