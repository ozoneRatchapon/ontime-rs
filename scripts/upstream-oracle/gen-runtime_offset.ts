// The runtime offset contract (RIIR 3): how late the run is, and when the next
// flag, the end of the group and the end of the rundown are now expected, as
// upstream's server computes them (stores/runtimeState.ts). Every screen reads
// these from the runtime-data packet, so our Runtime::snapshot must agree.
//
// Roll is covered too (RIIR 4): runtime.service drives it from each tick's
// result, which `tick` below copies. Ticks land exactly on a boundary, as the
// Durable Object's alarm does; upstream ticks every 32ms instead.
//
// Each case is a rundown and a script of operator actions at given times. The
// clock is faked; node runs with TZ=UTC so a time of day is epoch mod 24h.
import { makeOntimeGroup } from '@upstream/apps/server/src/api-data/rundown/__mocks__/rundown.mocks.ts';
import * as state from '@upstream/apps/server/src/stores/runtimeState.ts';
import { OffsetMode } from 'ontime-types';

import { ev, H, MIN, processRundown, rundownOf } from './rundown.ts';

// a UTC midnight; every `at` below is milliseconds from it, and may pass 24h
const DAY = 1_699_920_000_000;
let fakeNow = DAY;
Date.now = () => fakeNow;

type Step =
  | { at: number; do: 'start'; id: string }
  | { at: number; do: 'load'; id: string }
  | { at: number; do: 'addtime'; amount: number }
  | { at: number; do: 'mode'; mode: 'absolute' | 'relative' }
  | { at: number; do: 'pause' | 'resume' | 'stop' | 'tick' | 'roll' };

const show = () =>
  rundownOf([
    ev('a', 10 * H, 30 * MIN),
    ev('b', 10 * H + 30 * MIN, 30 * MIN),
    ev('c', 11 * H, 30 * MIN, { flag: true }),
    ev('d', 11 * H + 30 * MIN, 30 * MIN),
  ]);
const gaps = () =>
  rundownOf([ev('a', 10 * H, 30 * MIN), ev('b', 11 * H, 30 * MIN, { flag: true }), ev('c', 12 * H, 30 * MIN)]);
const linked = () =>
  rundownOf([
    ev('a', 10 * H, 30 * MIN),
    ev('b', 0, 30 * MIN, { linkStart: true }),
    ev('c', 0, 30 * MIN, { linkStart: true, flag: true }),
    ev('d', 12 * H, 30 * MIN),
  ]);
const grouped = () =>
  rundownOf(
    [makeOntimeGroup({ id: 'g', entries: ['a', 'b'] }), ev('c', 11 * H + 30 * MIN, 30 * MIN)],
    [ev('a', 10 * H, 30 * MIN, { parent: 'g' }), ev('b', 10 * H + 45 * MIN, 30 * MIN, { parent: 'g' })],
  );
const overnight = () =>
  rundownOf([
    ev('a', 23 * H + 30 * MIN, 60 * MIN),
    ev('b', 0 * H + 45 * MIN, 30 * MIN, { flag: true }),
    ev('c', 2 * H, 30 * MIN),
  ]);

const cases: [string, ReturnType<typeof rundownOf>, Step[]][] = [
  [
    'on time',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 10 * MIN, do: 'tick' },
    ],
  ],
  ['started late', show(), [{ at: 10 * H + 5 * MIN, do: 'start', id: 'a' }]],
  ['started early', show(), [{ at: 9 * H + 55 * MIN, do: 'start', id: 'a' }]],
  [
    'running over',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 40 * MIN, do: 'tick' },
    ],
  ],
  ['gaps absorb lateness', gaps(), [{ at: 10 * H + 10 * MIN, do: 'start', id: 'a' }]],
  ['lateness beyond the gaps', gaps(), [{ at: 10 * H + 50 * MIN, do: 'start', id: 'a' }]],
  ['a linked chain carries lateness', linked(), [{ at: 10 * H + 10 * MIN, do: 'start', id: 'a' }]],
  ['the end of the group', grouped(), [{ at: 10 * H + 5 * MIN, do: 'start', id: 'a' }]],
  [
    'the last event of the group',
    grouped(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 50 * MIN, do: 'start', id: 'b' },
    ],
  ],
  [
    'added time',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 1 * MIN, do: 'addtime', amount: 5 * MIN },
    ],
  ],
  [
    'removed time',
    show(),
    [
      { at: 10 * H + 5 * MIN, do: 'start', id: 'a' },
      { at: 10 * H + 6 * MIN, do: 'addtime', amount: -3 * MIN },
    ],
  ],
  [
    'relative mode',
    show(),
    [
      { at: 10 * H + 5 * MIN, do: 'start', id: 'a' },
      { at: 10 * H + 6 * MIN, do: 'mode', mode: 'relative' },
    ],
  ],
  [
    'relative mode on a later event',
    show(),
    [
      { at: 10 * H + 5 * MIN, do: 'start', id: 'a' },
      { at: 10 * H + 6 * MIN, do: 'mode', mode: 'relative' },
      { at: 10 * H + 45 * MIN, do: 'start', id: 'b' },
    ],
  ],
  [
    'back to absolute',
    show(),
    [
      { at: 10 * H + 5 * MIN, do: 'mode', mode: 'relative' },
      { at: 10 * H + 5 * MIN, do: 'start', id: 'a' },
      { at: 10 * H + 6 * MIN, do: 'mode', mode: 'absolute' },
    ],
  ],
  ['loaded but not started', show(), [{ at: 10 * H + 5 * MIN, do: 'load', id: 'a' }]],
  [
    'a later event is loaded',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 20 * MIN, do: 'load', id: 'b' },
    ],
  ],
  [
    'the next event starts late',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 40 * MIN, do: 'start', id: 'b' },
    ],
  ],
  [
    'the flag is running',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 11 * H + 2 * MIN, do: 'start', id: 'c' },
    ],
  ],
  [
    'stopped',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 10 * MIN, do: 'stop' },
    ],
  ],
  [
    'paused',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 10 * MIN, do: 'pause' },
      { at: 10 * H + 15 * MIN, do: 'tick' },
    ],
  ],
  [
    'resumed after a pause',
    show(),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 10 * MIN, do: 'pause' },
      { at: 10 * H + 15 * MIN, do: 'resume' },
    ],
  ],
  [
    'count to end started late',
    rundownOf([ev('a', 10 * H, 30 * MIN, { countToEnd: true }), ev('b', 10 * H + 30 * MIN, 30 * MIN, { flag: true })]),
    [{ at: 10 * H + 10 * MIN, do: 'start', id: 'a' }],
  ],
  [
    'a skipped event in between',
    rundownOf([
      ev('a', 10 * H, 30 * MIN),
      ev('s', 10 * H + 30 * MIN, 30 * MIN, { skip: true, flag: true }),
      ev('b', 11 * H, 30 * MIN, { flag: true }),
    ]),
    [{ at: 10 * H + 5 * MIN, do: 'start', id: 'a' }],
  ],
  [
    'numbered past a skipped event',
    rundownOf([
      ev('a', 10 * H, 30 * MIN),
      ev('s', 10 * H + 30 * MIN, 30 * MIN, { skip: true }),
      ev('b', 11 * H, 30 * MIN),
      ev('c', 11 * H + 30 * MIN, 30 * MIN, { flag: true }),
    ]),
    [
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 11 * H + 5 * MIN, do: 'start', id: 'b' },
    ],
  ],
  [
    'relative mode survives a stop',
    show(),
    [
      { at: 10 * H, do: 'mode', mode: 'relative' },
      { at: 10 * H, do: 'start', id: 'a' },
      { at: 10 * H + 5 * MIN, do: 'stop' },
      { at: 10 * H + 10 * MIN, do: 'start', id: 'b' },
    ],
  ],
  ['across midnight', overnight(), [{ at: 23 * H + 40 * MIN, do: 'start', id: 'a' }]],
  [
    'after midnight',
    overnight(),
    [
      { at: 23 * H + 40 * MIN, do: 'start', id: 'a' },
      { at: 24 * H + 50 * MIN, do: 'start', id: 'b' },
    ],
  ],
  ['started on the next day', overnight(), [{ at: 24 * H + 50 * MIN, do: 'start', id: 'b' }]],
  // roll: the wall clock picks the event
  ['roll before the show', show(), [{ at: 9 * H + 50 * MIN, do: 'roll' }]],
  ['roll joins a running event', show(), [{ at: 10 * H + 10 * MIN, do: 'roll' }]],
  ['roll in a gap waits', gaps(), [{ at: 10 * H + 40 * MIN, do: 'roll' }]],
  [
    'roll starts the event when it is due',
    show(),
    [
      { at: 9 * H + 59 * MIN, do: 'roll' },
      { at: 10 * H, do: 'tick' },
      { at: 10 * H + 5 * MIN, do: 'tick' },
    ],
  ],
  [
    'roll moves on when an event ends',
    show(),
    [
      { at: 10 * H + 10 * MIN, do: 'roll' },
      { at: 10 * H + 30 * MIN, do: 'tick' },
      { at: 10 * H + 35 * MIN, do: 'tick' },
    ],
  ],
  [
    'roll waits through a gap after an event',
    gaps(),
    [
      { at: 10 * H + 10 * MIN, do: 'roll' },
      { at: 10 * H + 30 * MIN, do: 'tick' },
      { at: 10 * H + 40 * MIN, do: 'tick' },
    ],
  ],
  ['roll after the show', show(), [{ at: 13 * H, do: 'roll' }]],
  ['roll across midnight', overnight(), [{ at: 23 * H + 50 * MIN, do: 'roll' }]],
  [
    'roll into the next day',
    overnight(),
    [
      { at: 23 * H + 50 * MIN, do: 'roll' },
      { at: 24 * H + 30 * MIN, do: 'tick' },
      { at: 24 * H + 40 * MIN, do: 'tick' },
    ],
  ],
  [
    'roll takes over a late start',
    show(),
    [
      { at: 10 * H + 5 * MIN, do: 'start', id: 'a' },
      { at: 10 * H + 6 * MIN, do: 'roll' },
    ],
  ],
];

const run = (rundown: ReturnType<typeof rundownOf>, steps: Step[]) => {
  state.clearState();
  state.setOffsetMode(OffsetMode.Absolute);
  const meta: any = processRundown(structuredClone(rundown));
  const processed = { ...rundown, entries: meta.entries, flatOrder: meta.flatEntryOrder };
  state.updateRundownData({ numEvents: meta.timedEventOrder.length, ...meta });
  const event = (id: string) => processed.entries[id];
  // runtime.service checkTimerUpdate: roll moves itself on from a tick's result
  const tick = () => {
    const result: any = state.update();
    if (state.getState().timer.playback !== 'roll') return;
    if (result.hasSecondaryTimerFinished) state.roll(processed, meta);
    else if (result.hasTimerFinished) state.rollNext(processed, meta);
  };

  for (const step of steps) {
    fakeNow = DAY + step.at;
    tick();
    switch (step.do) {
      case 'load':
        state.load(event(step.id), processed, meta);
        break;
      case 'start':
        // runtime.service startById: load unless it is already loaded, then start
        if (state.getState().eventNow?.id !== step.id) state.load(event(step.id), processed, meta);
        state.start();
        break;
      case 'addtime':
        state.addTime(step.amount);
        break;
      case 'mode':
        state.setOffsetMode(step.mode === 'relative' ? OffsetMode.Relative : OffsetMode.Absolute);
        break;
      case 'pause':
        state.pause();
        break;
      case 'resume':
        state.start();
        break;
      case 'stop':
        state.stop();
        break;
      case 'roll':
        state.roll(processed, meta);
        break;
      case 'tick':
        break;
    }
    tick();
  }

  const { offset, rundown: run, eventFlag, groupNow, eventNow, timer } = state.getState();
  return {
    timer: {
      playback: timer.playback,
      startedAt: timer.startedAt,
      secondaryTimer: timer.secondaryTimer,
      duration: timer.duration,
      current: timer.current,
    },
    eventNow: eventNow?.id ?? null,
    offset: { ...offset },
    rundown: {
      selectedEventIndex: run.selectedEventIndex,
      numEvents: run.numEvents,
      plannedStart: run.plannedStart,
      plannedEnd: run.plannedEnd,
      actualStart: run.actualStart,
      currentDay: run.currentDay,
    },
    eventFlag: eventFlag?.id ?? null,
    groupNow: groupNow?.id ?? null,
  };
};

const out = cases.map(([name, rundown, steps]) => ({
  name,
  epoch: DAY,
  rundown: structuredClone(rundown),
  steps,
  expected: run(rundown, steps),
}));
process.stdout.write(JSON.stringify(out));
