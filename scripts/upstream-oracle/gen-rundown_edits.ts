// The rundown edit contract (RIIR 7, plan 015): what upstream's own rundown
// service makes of the edits an operator does every day. Its storage and
// notifications are replaced by stubs (rundown-stubs/, laid over the upstream
// tree by generate.sh); the service, the DAO and the processing are real.
//
// Each case is a starting rundown and a script of edits. After every edit the
// processed rundown is recorded, or the error the edit was refused with.
// tests/rundown_edits_tests.rs sends the same edits through our REST routes.
import {
  makeOntimeDelay,
  makeOntimeGroup,
  makeOntimeMilestone,
} from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import { getCurrentRundown, rundownCache } from '@upstream/apps/server/src/api-data/rundown/rundown.dao.ts';
import * as service from '@upstream/apps/server/src/api-data/rundown/rundown.service.ts';

import { ev, H, MIN, rundownOf } from './rundown.ts';

type Edit =
  | { do: 'add'; entry: Record<string, unknown> }
  | { do: 'edit'; patch: Record<string, unknown> }
  | { do: 'batch'; ids: string[]; data: Record<string, unknown> }
  | { do: 'delete'; ids: string[] }
  | { do: 'reorder'; entryId: string; destinationId: string; order: 'before' | 'after' | 'insert' }
  | { do: 'swap'; from: string; to: string }
  | { do: 'applydelay'; id: string }
  | { do: 'clone'; id: string; options?: Record<string, unknown> }
  | { do: 'group'; ids: string[] }
  | { do: 'ungroup'; id: string };

const three = () =>
  rundownOf([ev('a', 10 * H, 30 * MIN), ev('b', 10 * H + 30 * MIN, 30 * MIN), ev('c', 11 * H, 30 * MIN)]);
const linked = () =>
  rundownOf([
    ev('a', 10 * H, 30 * MIN),
    ev('b', 10 * H + 30 * MIN, 30 * MIN, { linkStart: true }),
    ev('c', 11 * H, 30 * MIN, { linkStart: true }),
  ]);
const grouped = () =>
  rundownOf(
    [makeOntimeGroup({ id: 'g', title: 'Block', entries: ['a', 'b'] }), ev('c', 11 * H, 30 * MIN)],
    [ev('a', 10 * H, 30 * MIN, { parent: 'g' }), ev('b', 10 * H + 30 * MIN, 30 * MIN, { parent: 'g' })],
  );
const withDelay = () =>
  rundownOf([
    ev('a', 10 * H, 30 * MIN),
    makeOntimeDelay({ id: 'd', duration: 10 * MIN }),
    ev('b', 10 * H + 30 * MIN, 30 * MIN),
    ev('c', 11 * H, 30 * MIN, { linkStart: true }),
  ]);

const cases: [string, ReturnType<typeof rundownOf>, Edit[]][] = [
  // adding
  [
    'add an event at the end',
    three(),
    [{ do: 'add', entry: { id: 'n', type: 'event', title: 'New', duration: 15 * MIN, timeStart: 12 * H } }],
  ],
  [
    'add an event after another',
    three(),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, after: 'a' } }],
  ],
  [
    'add an event before another',
    three(),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, before: 'b' } }],
  ],
  [
    'add an event at the top',
    three(),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, before: true } }],
  ],
  ['add a delay', three(), [{ do: 'add', entry: { id: 'n', type: 'delay', duration: 5 * MIN, after: 'a' } }]],
  ['add a milestone', three(), [{ do: 'add', entry: { id: 'n', type: 'milestone', title: 'Doors', after: 'b' } }]],
  ['add a group', three(), [{ do: 'add', entry: { id: 'n', type: 'group', title: 'Block' } }]],
  [
    'add an event into a group',
    grouped(),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, parent: 'g' } }],
  ],
  [
    'add an event after one in a group',
    grouped(),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, after: 'a' } }],
  ],
  [
    'add a linked event',
    three(),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, after: 'c', linkStart: true } }],
  ],
  // the cue a new event gets, from the event before it
  ...(
    [
      ['a numbered cue', '2', 'b'],
      ['a decimal cue', '1.09', 'b'],
      ['a prefixed cue', 'A-7', 'b'],
      ['a spaced cue', 'Scene 12', 'b'],
      ['a cue with zeros', '007', 'b'],
      ['a cue that ends in a letter', '4A', 'b'],
      ['an empty cue', '', 'b'],
      ['a delay, after an event', '3', 'd'],
    ] as const
  ).map(([what, cue, after]): [string, ReturnType<typeof rundownOf>, Edit[]] => [
    `add after ${what}`,
    rundownOf([
      ev('a', 10 * H, 30 * MIN, { cue: '1' }),
      ev('b', 10 * H + 30 * MIN, 30 * MIN, { cue }),
      makeOntimeDelay({ id: 'd', duration: 5 * MIN }),
      ev('c', 11 * H, 30 * MIN, { cue: '9' }),
    ]),
    [{ do: 'add', entry: { id: 'n', type: 'event', duration: 10 * MIN, after } }],
  ]),
  ['add an event with its own cue', three(), [{ do: 'add', entry: { id: 'n', type: 'event', cue: 'X1', after: 'a' } }]],
  ['add an id that exists', three(), [{ do: 'add', entry: { id: 'a', type: 'event', duration: 10 * MIN } }]],
  ['add a group into a group', grouped(), [{ do: 'add', entry: { id: 'n', type: 'group', parent: 'g' } }]],
  // editing times
  ['edit a duration (lock-duration)', three(), [{ do: 'edit', patch: { id: 'a', duration: 45 * MIN } }]],
  ['edit an end (lock-duration)', three(), [{ do: 'edit', patch: { id: 'a', timeEnd: 10 * H + 40 * MIN } }]],
  ['edit a start (lock-duration)', three(), [{ do: 'edit', patch: { id: 'b', timeStart: 10 * H + 45 * MIN } }]],
  [
    'edit a start (lock-end)',
    three(),
    [
      { do: 'edit', patch: { id: 'b', timeStrategy: 'lock-end' } },
      { do: 'edit', patch: { id: 'b', timeStart: 10 * H + 45 * MIN } },
    ],
  ],
  [
    'edit a duration (lock-end)',
    three(),
    [
      { do: 'edit', patch: { id: 'b', timeStrategy: 'lock-end' } },
      { do: 'edit', patch: { id: 'b', duration: 10 * MIN } },
    ],
  ],
  ['edit the first of a linked chain', linked(), [{ do: 'edit', patch: { id: 'a', duration: 40 * MIN } }]],
  ['unlink an event', linked(), [{ do: 'edit', patch: { id: 'b', linkStart: false, timeStart: 11 * H } }]],
  ['link an event', three(), [{ do: 'edit', patch: { id: 'c', linkStart: true } }]],
  [
    'edit past midnight',
    three(),
    [{ do: 'edit', patch: { id: 'c', timeStart: 23 * H + 30 * MIN, duration: 60 * MIN } }],
  ],
  ['edit a title and a flag', three(), [{ do: 'edit', patch: { id: 'b', title: 'Keynote', flag: true, skip: true } }]],
  ['edit a group title', grouped(), [{ do: 'edit', patch: { id: 'g', title: 'Morning', targetDuration: 60 * MIN } }]],
  ['edit an entry that does not exist', three(), [{ do: 'edit', patch: { id: 'zz', title: 'Gone' } }]],
  ['batch edit', three(), [{ do: 'batch', ids: ['a', 'c'], data: { colour: '#FF0000', countToEnd: true } }]],
  // removing
  ['delete an event', three(), [{ do: 'delete', ids: ['b'] }]],
  ['delete a group', grouped(), [{ do: 'delete', ids: ['g'] }]],
  ['delete the last event of a group', grouped(), [{ do: 'delete', ids: ['a', 'b'] }]],
  ['delete a linked event', linked(), [{ do: 'delete', ids: ['b'] }]],
  // moving
  ['move an event after another', three(), [{ do: 'reorder', entryId: 'a', destinationId: 'c', order: 'after' }]],
  ['move an event before another', three(), [{ do: 'reorder', entryId: 'c', destinationId: 'a', order: 'before' }]],
  ['move an event into a group', grouped(), [{ do: 'reorder', entryId: 'c', destinationId: 'g', order: 'insert' }]],
  ['move an event out of a group', grouped(), [{ do: 'reorder', entryId: 'a', destinationId: 'c', order: 'after' }]],
  ['move a linked event', linked(), [{ do: 'reorder', entryId: 'b', destinationId: 'c', order: 'after' }]],
  ['swap two events', three(), [{ do: 'swap', from: 'a', to: 'c' }]],
  ['swap linked events', linked(), [{ do: 'swap', from: 'a', to: 'b' }]],
  // delays, clones, groups
  ['apply a delay', withDelay(), [{ do: 'applydelay', id: 'd' }]],
  ['clone an event', three(), [{ do: 'clone', id: 'b', options: { after: 'b' } }]],
  ['clone a group', grouped(), [{ do: 'clone', id: 'g', options: { after: 'c' } }]],
  [
    'clone a milestone',
    rundownOf([ev('a', 10 * H, 30 * MIN), makeOntimeMilestone({ id: 'm', title: 'Doors' })]),
    [{ do: 'clone', id: 'm', options: { after: 'm' } }],
  ],
  ['group events', three(), [{ do: 'group', ids: ['a', 'b'] }]],
  ['group events not side by side', three(), [{ do: 'group', ids: ['a', 'c'] }]],
  ['ungroup', grouped(), [{ do: 'ungroup', id: 'g' }]],
  ['ungroup something that is not a group', three(), [{ do: 'ungroup', id: 'a' }]],
  // a day's work in one go
  [
    'a run of edits',
    three(),
    [
      { do: 'add', entry: { id: 'n', type: 'event', title: 'Break', duration: 15 * MIN, after: 'b', linkStart: true } },
      { do: 'edit', patch: { id: 'a', duration: 35 * MIN } },
      { do: 'group', ids: ['b', 'n'] },
      { do: 'reorder', entryId: 'c', destinationId: 'a', order: 'before' },
      { do: 'delete', ids: ['a'] },
    ],
  ],
];

/** The fields both servers keep, of every entry, in the processed rundown. */
const KEEP = [
  'type',
  'parent',
  'entries',
  'title',
  'cue',
  'skip',
  'flag',
  'colour',
  'countToEnd',
  'linkStart',
  'timeStrategy',
  'timeStart',
  'timeEnd',
  'duration',
  'delay',
  'gap',
  'dayOffset',
  'targetDuration',
];
const snapshot = () => {
  const rundown = getCurrentRundown();
  const entries = Object.fromEntries(
    Object.entries(rundown.entries).map(([id, entry]) => [
      id,
      Object.fromEntries(KEEP.filter((key) => key in entry).map((key) => [key, (entry as any)[key]])),
    ]),
  );
  return { order: rundown.order, flatOrder: rundown.flatOrder, entries };
};

const apply = async (edit: Edit) => {
  const id = getCurrentRundown().id;
  switch (edit.do) {
    case 'add':
      return service.addEntry(id, edit.entry as any);
    case 'edit':
      return service.editEntry(id, edit.patch as any);
    case 'batch':
      return service.batchEditEntries(id, edit.ids, edit.data as any);
    case 'delete':
      return service.deleteEntries(id, edit.ids);
    case 'reorder':
      return service.reorderEntry(id, edit.entryId, edit.destinationId, edit.order);
    case 'swap':
      return service.swapEvents(id, edit.from, edit.to);
    case 'applydelay':
      return service.applyDelay(id, edit.id);
    case 'clone':
      return service.cloneEntry(id, edit.id, (edit.options ?? {}) as any);
    case 'group':
      return service.groupEntries(id, edit.ids);
    case 'ungroup':
      return service.ungroupEntries(id, edit.id);
  }
};

const out = [];
for (const [name, rundown, edits] of cases) {
  rundownCache.init(structuredClone(rundown) as any, {});
  const steps = [];
  for (const edit of edits) {
    try {
      await apply(edit);
      steps.push({ edit, after: snapshot() });
    } catch (error) {
      steps.push({ edit, error: (error as Error).message, after: snapshot() });
    }
  }
  out.push({ name, rundown: structuredClone(rundown), steps });
}
process.stdout.write(JSON.stringify(out));
