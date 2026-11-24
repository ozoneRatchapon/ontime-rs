// The transport contract: scenarios of steps replayed against upstream's
// teleprompterTransport.ts, recording the outcome of every step, plus the
// integration command parser over every payload form.
import { parseTeleprompterCommand } from '@upstream/apps/server/src/api-integration/integration.teleprompter.ts';
import { makeTextLines } from '@upstream/packages/utils/src/teleprompter/teleprompter.mocks.ts';
import { makeTeleprompterLayout, startOfEvent } from '@upstream/packages/utils/src/teleprompter/teleprompterPosition.ts';
import {
  applyTransportCommand,
  describeSync,
  describeTransport,
  positionAt,
  reanchorTransport,
  settle,
  syncPositionAt,
} from '@upstream/packages/utils/src/teleprompter/teleprompterTransport.ts';

const day = 86_400_000;
const T = 36_000_000; // 10:00
const minute = 60_000;

const event = (id: string, text: string) => ({
  id,
  cue: id.toUpperCase(),
  title: `Title ${id}`,
  lines: [{ kind: 'heading', text: id }, ...makeTextLines(text, 12), { kind: 'blank' }],
});
// a: rows 0 heading, 1-2 text, 3 blank | b: 4 heading, 5-6 text, 7 blank | c: 8 heading, 9 text, 10 blank
const abc = [event('a', 'one two three four'), event('b', 'five six seven eight'), event('c', 'nine ten')];
const grown = [event('a', 'zero one two three four five six seven'), abc[1], abc[2]];
const withoutB = [abc[0], abc[2]];
const scripts: Record<string, unknown[]> = { abc, grown, withoutB, empty: [], one: [abc[0]] };

const base = (patch: Record<string, unknown> = {}) => ({
  playback: 'pause',
  mode: 'event',
  speed: 25,
  anchor: startOfEvent('a'),
  at: T,
  ended: null,
  blackout: 'off',
  ...patch,
});

type Step =
  | { cmd: Record<string, unknown>; now: number }
  | { settle: number }
  | { position: number }
  | { describe: number }
  | { sync: number }
  | { syncAt: number }
  | { reanchor: string; now: number };

const cmd = (type: string, now: number, extra: Record<string, unknown> = {}): Step => ({ cmd: { type, ...extra }, now });

const scenarios: [string, string, Record<string, unknown>, Step[]][] = [
  ['speed commands', 'abc', base(), [
    cmd('speed', T, { value: 50 }), cmd('speed', T, { value: 0 }), cmd('speed', T, { value: -10 }), cmd('speed', T, { value: 0 }),
    cmd('speed', T, { value: 200 }), cmd('speed', T, { value: -0.4 }), cmd('speed', T, { value: 30 }),
    cmd('speedBy', T, { value: 10 }), cmd('speedBy', T, { value: -60 }), cmd('speedBy', T, { value: -5 }), cmd('speedBy', T, { value: -5 }),
    cmd('speedBy', T, { value: 3 }), cmd('speedBy', T, { value: 300 }),
  ]],
  ['playing in event mode stops at the end of the event', 'abc', base(), [
    cmd('play', T), { position: T + minute }, { describe: T + minute }, { sync: 7 }, { syncAt: T + minute },
    { position: T + 10 * minute }, { describe: T + 10 * minute }, { settle: T + 10 * minute }, { sync: 7 },
    cmd('play', T + 11 * minute), { position: T + 12 * minute }, { settle: T + 30 * minute }, { describe: T + 30 * minute },
  ]],
  ['script mode reads to the end of the script', 'abc', base({ mode: 'script', speed: 60 }), [
    cmd('play', T), { position: T + 5 * minute }, { describe: T + 5 * minute }, { settle: T + 20 * minute }, { describe: T + 20 * minute },
    cmd('play', T + 21 * minute), { describe: T + 21 * minute },
  ]],
  ['reading back', 'abc', base({ anchor: { eventId: 'b', charOffset: 9, lines: 0.5 }, speed: -20 }), [
    cmd('play', T), { position: T + minute / 10 }, { settle: T + 10 * minute }, cmd('mode', T + 10 * minute, { mode: 'script' }),
    cmd('play', T + 10 * minute), { settle: T + 30 * minute }, { describe: T + 30 * minute },
  ]],
  ['moves keep the play state', 'abc', base(), [
    cmd('play', T), cmd('next', T + 1000), { describe: T + 1000 }, cmd('next', T + 2000), cmd('next', T + 3000),
    cmd('previous', T + 3000), cmd('previous', T + 3000), cmd('pause', T + 4000), cmd('top', T + 5000), cmd('previous', T + 5000),
    cmd('goto', T + 6000, { eventId: 'c' }), cmd('goto', T + 6000, { eventId: 'gone' }), { describe: T + 6000 },
  ]],
  ['previous restarts the event once a line into it', 'abc', base({ anchor: { eventId: 'b', charOffset: 0, lines: 0.5 } }), [
    cmd('previous', T), cmd('scroll', T, { lines: 5.5 }), cmd('previous', T), cmd('scroll', T, { lines: 1 }), cmd('previous', T),
  ]],
  ['scrolling is clamped and clears the end', 'abc', base(), [
    cmd('scroll', T, { lines: 2.5 }), cmd('scroll', T, { lines: -100 }), cmd('scroll', T, { lines: 1000 }), cmd('play', T + 1),
    cmd('scroll', T + 2, { lines: -3 }), { position: T + minute },
  ]],
  ['an empty script cannot play', 'empty', base({ anchor: null }), [
    cmd('play', T), { describe: T }, { sync: 1 }, cmd('next', T), cmd('scroll', T, { lines: 3 }), { position: T },
  ]],
  ['mode and blackout toggles leave playback alone', 'abc', base(), [
    cmd('play', T), cmd('mode', T + 1000, { mode: 'toggle' }), cmd('blackout', T + 2000, { blackout: 'toggle' }),
    { describe: T + 2000 }, cmd('blackout', T + 3000, { blackout: 'off' }), cmd('mode', T + 4000, { mode: 'event' }), cmd('toggle', T + 5000),
    cmd('toggle', T + 6000),
  ]],
  ['playback across midnight', 'abc', base({ mode: 'script', speed: 10, at: day - 30_000 }), [
    cmd('play', day - 30_000), { position: 30_000 }, { syncAt: 30_000 }, { describe: 30_000 },
  ]],
  ['a client clock slightly behind', 'abc', base({ mode: 'script' }), [
    cmd('play', T), { position: T - 500 }, { syncAt: T - 500 }, { position: T - 59_000 }, { position: T - 61_000 },
  ]],
  ['slightly behind just after midnight', 'abc', base({ mode: 'script', at: 1000 }), [
    cmd('play', 1000), { position: day - 500 }, { syncAt: day - 500 },
  ]],
  ['long playback at the slowest speed', 'abc', base({ mode: 'script', speed: 1 }), [
    cmd('play', T), { position: T + 3 * minute }, { position: T + 9 * minute }, { settle: T + 9 * minute }, { settle: T + 20 * minute },
  ]],
  ['a repeated command is still a change', 'abc', base(), [cmd('pause', T), cmd('pause', T + 5), cmd('speed', T + 9, { value: 25 })]],
  ['an edit above during playback does not jump', 'abc', base({ anchor: startOfEvent('b'), mode: 'script' }), [
    cmd('play', T), { position: T + minute / 2 }, { reanchor: 'grown', now: T + minute / 2 }, { position: T + minute / 2 }, { position: T + minute },
  ]],
  ['a paused reader keeps their exact anchor across a rewrap', 'abc', base({ anchor: { eventId: 'a', charOffset: 8, lines: 0.25 } }), [
    { reanchor: 'abc', now: T }, { reanchor: 'grown', now: T + 1 }, { position: T + 2 },
  ]],
  ['the reading event is deleted', 'abc', base({ anchor: { eventId: 'b', charOffset: 5, lines: 0 } }), [
    { reanchor: 'withoutB', now: T }, { describe: T },
  ]],
  ['the script empties', 'abc', base(), [cmd('play', T), { reanchor: 'empty', now: T + 1000 }, { describe: T + 1000 }]],
  ['a single event plays to the script end', 'one', base(), [cmd('play', T), { settle: T + 10 * minute }, { describe: T + 10 * minute }]],
  ['an anchor outside the script', 'abc', base({ anchor: startOfEvent('gone') }), [
    { position: T }, { describe: T }, { sync: 2 }, cmd('play', T), { settle: T + minute },
  ]],
];

const replay = ([name, scriptName, initial, steps]: (typeof scenarios)[number]) => {
  let events = scripts[scriptName];
  let layout = makeTeleprompterLayout(events as never);
  let transport = initial as never;
  const results = steps.map((step) => {
    if ('cmd' in step) {
      try {
        transport = applyTransportCommand(transport, step.cmd as never, layout, step.now);
        return { transport };
      } catch (error) {
        return { error: (error as Error).message, transport };
      }
    }
    if ('settle' in step) return { transport: (transport = settle(transport, layout, step.settle)) };
    if ('position' in step) return { position: positionAt(transport, layout, step.position) };
    if ('describe' in step) return { state: describeTransport(transport, layout, step.describe) };
    if ('sync' in step) return { sync: describeSync(transport, layout, step.sync) };
    if ('syncAt' in step) return { position: syncPositionAt(describeSync(transport, layout, 1), step.syncAt) };
    const next = scripts[step.reanchor];
    const nextLayout = makeTeleprompterLayout(next as never);
    transport = reanchorTransport(transport, { events: events as never, layout }, { events: next as never, layout: nextLayout }, step.now);
    events = next;
    layout = nextLayout;
    return { transport };
  });
  return { name, script: scriptName, initial, steps, results };
};

const payloads: unknown[] = [
  'play', 'pause', 'toggle', 'next', 'previous', 'top', 'stop', '', null, 5, [],
  { speed: 40 }, { speed: '-20' }, { speed: '' }, { speed: ' ' }, { speed: 'fast' }, { speed: Infinity }, { speed: null },
  { speedBy: 5 }, { speedBy: '-5' }, { scroll: 2.5 }, { scroll: '-3' },
  { mode: 'event' }, { mode: 'script' }, { mode: 'toggle' }, { mode: 'other' },
  { blackout: 'on' }, { blackout: 'off' }, { blackout: 'toggle' }, { blackout: true },
  { goto: 'loaded' }, { goto: { cue: '2.1' } }, { goto: { cue: 12 } }, { goto: { id: 'abc' } }, { goto: { id: 5 } },
  { goto: { index: 3 } }, { goto: { index: '2' } }, { goto: { index: 0 } }, { goto: { index: 1.5 } }, { goto: { index: 'x' } },
  { goto: {} }, { goto: 'first' }, { other: 1 }, { speed: 10, mode: 'script' },
];
const parse = (payload: unknown) => {
  try {
    return { payload, command: parseTeleprompterCommand(payload) };
  } catch (error) {
    return { payload, error: (error as Error).message };
  }
};

process.stdout.write(
  JSON.stringify({ scripts, scenarios: scenarios.map(replay), parses: payloads.map(parse) }, (_key, value) =>
    value === Infinity ? 'Infinity' : value,
  ),
);
