// The selection contract (RIIR 5): which event each way of choosing one picks,
// as upstream's runtime.service asks runtime.utils and roll.utils. Our
// selection.rs must pick the same, so `load/next`, `start/index/3` and
// `start/cue/4` reach the same event through HTTP and the websocket.
//
// For every rundown, and for every event that could be loaded (or none):
// next, previous and the next roll event; every index; and every cue, as start
// (timed order, the armed event allowed or not) and load (playable order) ask.
import {
  makeOntimeDelay,
  makeOntimeGroup,
  makeOntimeMilestone,
} from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import { findNextRollEvent } from '@upstream/apps/server/src/services/runtime-service/roll.utils.ts';
import {
  findNextPlayableId,
  findNextPlayableWithCue,
  findPreviousPlayableId,
  getEventAtIndex,
} from '@upstream/apps/server/src/services/runtime-service/runtime.utils.ts';
import { isPlayableEvent } from 'ontime-types';

import { ev, H, MIN, processRundown, rundownOf } from './rundown.ts';

const rundowns: [string, ReturnType<typeof rundownOf>][] = [
  ['plain', rundownOf([ev('a', 10 * H, 30 * MIN), ev('b', 10 * H + 30 * MIN, 30 * MIN), ev('c', 11 * H, 30 * MIN)])],
  [
    'skipped events',
    rundownOf([
      ev('a', 10 * H, 30 * MIN, { skip: true }),
      ev('b', 10 * H + 30 * MIN, 30 * MIN),
      ev('c', 11 * H, 30 * MIN, { skip: true }),
      ev('d', 11 * H + 30 * MIN, 30 * MIN),
      ev('e', 12 * H, 30 * MIN, { skip: true }),
    ]),
  ],
  [
    'groups, delays and milestones',
    rundownOf(
      [
        makeOntimeMilestone({ id: 'm', cue: 'a' }),
        makeOntimeGroup({ id: 'g', entries: ['a', 'b'] }),
        makeOntimeDelay({ id: 'x', duration: 5 * MIN }),
        ev('c', 11 * H, 30 * MIN),
      ],
      [ev('a', 10 * H, 30 * MIN, { parent: 'g' }), ev('b', 10 * H + 30 * MIN, 30 * MIN, { parent: 'g' })],
    ),
  ],
  [
    'shared cues',
    rundownOf([
      ev('a', 10 * H, 30 * MIN, { cue: '1' }),
      ev('b', 10 * H + 30 * MIN, 30 * MIN, { cue: '2' }),
      ev('c', 11 * H, 30 * MIN, { cue: '1' }),
      ev('d', 11 * H + 30 * MIN, 30 * MIN, { cue: '1', skip: true }),
      ev('e', 12 * H, 30 * MIN, { cue: '2' }),
    ]),
  ],
  ['zero-length events', rundownOf([ev('a', 10 * H, 0), ev('b', 10 * H, 30 * MIN), ev('c', 10 * H + 30 * MIN, 0)])],
  ['one event', rundownOf([ev('a', 10 * H, 30 * MIN)])],
  ['only skipped', rundownOf([ev('a', 10 * H, 30 * MIN, { skip: true }), ev('b', 11 * H, 30 * MIN, { skip: true })])],
  ['empty', rundownOf([])],
];

const cases = rundowns.map(([name, rundown]) => {
  const metadata = processRundown(rundown);
  const { timedEventOrder: timed, playableEventOrder: playable } = metadata;
  const cues = [...new Set(timed.map((id) => (rundown.entries as any)[id].cue as string)), 'none'];
  const loaded = [undefined, ...playable];

  const from = loaded.map((current) => {
    const currentIndex = current === undefined ? undefined : timed.indexOf(current);
    return {
      current: current ?? null,
      next: findNextPlayableId(playable, current) ?? null,
      previous: findPreviousPlayableId(playable, current) ?? null,
      rollNext: findNextRollEvent(rundown as any, metadata, current)?.id ?? null,
      cues: cues.map((cue) => ({
        cue,
        start: findNextPlayableWithCue(rundown as any, timed, cue, currentIndex, false)?.id ?? null,
        startArmed: findNextPlayableWithCue(rundown as any, timed, cue, currentIndex, true)?.id ?? null,
        load: findNextPlayableWithCue(rundown as any, playable, cue, currentIndex, false)?.id ?? null,
      })),
    };
  });

  // one past the end too, which finds nothing
  const indices = Array.from({ length: timed.length + 1 }, (_, index) => {
    const event = getEventAtIndex(rundown as any, timed, index);
    return { index, id: event?.id ?? null, loadable: event ? isPlayableEvent(event) : false };
  });

  return { name, rundown: structuredClone(rundown), timed, playable, from, indices };
});

process.stdout.write(JSON.stringify(cases));
