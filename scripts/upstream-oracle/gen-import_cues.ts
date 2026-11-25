// The import contract (plan 015, follow-up): what upstream's own parseRundown
// keeps of a rundown handed to it whole, as a project file or an import is.
// An event without a cue is numbered by its place among the events ("1", "2"…,
// nested ones included); a cue it brings is kept, even an empty one.
// tests/import_cues_tests.rs sends the same rundown through our
// `PUT /data/rundowns/:id`, which is how a whole rundown reaches this server.
import {
  makeOntimeDelay,
  makeOntimeGroup,
  makeOntimeMilestone,
} from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import { parseRundown } from '@upstream/apps/server/src/api-data/rundown/rundown.parser.ts';

import { ev, H, MIN, rundownOf } from './rundown.ts';

/** An event with its cue left out, as a hand-made or exported file may have it. */
const bare = (id: string, timeStart: number, patch: Record<string, unknown> = {}) => {
  const event: Record<string, unknown> = { ...ev(id, timeStart, 30 * MIN, patch) };
  if (!('cue' in patch)) delete event.cue;
  return event;
};

const cases: [string, ReturnType<typeof rundownOf>][] = [
  ['no event has a cue', rundownOf([bare('a', 10 * H), bare('b', 10 * H + 30 * MIN), bare('c', 11 * H)])],
  [
    'some events have a cue',
    rundownOf([bare('a', 10 * H, { cue: 'Intro' }), bare('b', 10 * H + 30 * MIN), bare('c', 11 * H)]),
  ],
  ['an empty cue is kept', rundownOf([bare('a', 10 * H, { cue: '' }), bare('b', 10 * H + 30 * MIN)])],
  ['a numeric cue', rundownOf([bare('a', 10 * H, { cue: 5 }), bare('b', 10 * H + 30 * MIN)])],
  [
    'delays and milestones are not counted',
    rundownOf([
      bare('a', 10 * H),
      makeOntimeDelay({ id: 'd', duration: 5 * MIN }),
      makeOntimeMilestone({ id: 'm', title: 'Doors' }),
      bare('b', 10 * H + 30 * MIN),
    ]),
  ],
  [
    'events in a group are counted in order',
    rundownOf(
      [bare('a', 9 * H), makeOntimeGroup({ id: 'g', entries: ['x', 'y'] }), bare('c', 11 * H)],
      [bare('x', 10 * H, { parent: 'g' }), bare('y', 10 * H + 30 * MIN, { parent: 'g' })],
    ),
  ],
];

const out = cases.map(([name, rundown]) => {
  const parsed = parseRundown(structuredClone(rundown) as any, {});
  const cues = Object.fromEntries(
    Object.entries(parsed.entries).map(([id, entry]) => [id, 'cue' in entry ? (entry as any).cue : null]),
  );
  return { name, rundown: structuredClone(rundown), order: parsed.order, flatOrder: parsed.flatOrder, cues };
});
process.stdout.write(JSON.stringify(out));
