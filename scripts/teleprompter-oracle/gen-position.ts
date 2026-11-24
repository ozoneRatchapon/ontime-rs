// The position contract: layout, anchor <-> row, events at rows, offsets
// through edits and re-anchoring, run over scripts made the way upstream's
// tests make them plus scripts wrapped from Thai and emoji text.
import { wrapText } from '@upstream/apps/server/src/services/teleprompter-service/teleprompter.wrap.ts';
import { makeTextLines } from '@upstream/packages/utils/src/teleprompter/teleprompter.mocks.ts';
import {
  anchorAtRow,
  eventAtRow,
  makeTeleprompterLayout,
  mapOffsetThroughEdit,
  reanchor,
  rowOfAnchor,
  startOfEvent,
} from '@upstream/packages/utils/src/teleprompter/teleprompterPosition.ts';

type Line =
  | { kind: 'heading'; text: string }
  | { kind: 'text'; text: string; start: number; indent?: number }
  | { kind: 'blank' };
type Event = { id: string; cue: string; title: string; lines: Line[] };

const plain = (id: string, text: string, width = 12, heading = true): Event => ({
  id,
  cue: id,
  title: id,
  lines: [
    ...(heading ? [{ kind: 'heading', text: id } as Line] : []),
    ...(makeTextLines(text, width) as Line[]),
    { kind: 'blank' },
  ],
});
const wrapped = (id: string, text: string, width: number): Event => ({
  id,
  cue: id,
  title: id,
  lines: [
    { kind: 'heading', text: id },
    ...wrapText(text, width).map(
      ({ text, start, indent }) =>
        (indent > 0 ? { kind: 'text', text, start, indent } : { kind: 'text', text, start }) as Line,
    ),
    { kind: 'blank' },
  ],
});
const noHeadings = (event: Event): Event => ({
  ...event,
  lines: event.lines.filter((line) => line.kind !== 'heading'),
});

const a = plain('a', 'one two');
const b = plain('b', 'three four');
const c = plain('c', 'five six');
const scripts: [string, Event[]][] = [
  ['two events, one empty', [plain('a', 'one two three four'), plain('b', '')]],
  ['two events', [plain('a', 'one two three four'), plain('b', 'five six')]],
  ['three short events', [a, b, c]],
  ['no headings', [noHeadings(a), noHeadings(b)]],
  ['only empty events', [plain('x', ''), plain('y', '')]],
  [
    'thai and emoji',
    [wrapped('t', 'สวัสดีครับ ยินดีต้อนรับทุกท่าน 👋🏽 ขอบคุณครับ', 8), wrapped('e', '👨‍👩‍👧‍👦 family\n  indented line here', 9)],
  ],
  ['empty script', []],
];

const sweep = (events: Event[]) => {
  const layout = makeTeleprompterLayout(events as never);
  const rows = [];
  for (let row = -0.5; row <= layout.rowCount + 0.5; row += 0.25) {
    const anchor = anchorAtRow(layout, row);
    rows.push({
      row,
      anchor,
      back: anchor ? rowOfAnchor(layout, anchor) : null,
      eventAt: eventAtRow(layout, row)?.id ?? null,
    });
  }
  return { layout, rows };
};

const extraAnchors: [string, Event[], unknown][] = [
  ['the start of an event', scripts[1][1], startOfEvent('b')],
  ['an event outside the script', scripts[1][1], startOfEvent('gone')],
  ['deep in the heading', scripts[1][1], { eventId: 'b', charOffset: 0, lines: -3 }],
  ['an offset between lines', scripts[1][1], { eventId: 'a', charOffset: 10, lines: 0.5 }],
  ['an offset past the text', scripts[1][1], { eventId: 'a', charOffset: 999, lines: 0 }],
  ['an empty event', scripts[0][1], { eventId: 'b', charOffset: 0, lines: 0.5 }],
];

const edits: [string, string, string, number][] = [
  ['after', 'one two three', 'one two three four', 4],
  ['starting at', 'one two three', 'one zero two three', 4],
  ['before', 'one two three', 'zero one two three', 8],
  ['shorter before', 'one two three', 'two three', 8],
  ['around', 'one two three', 'one TWO three', 5],
  ['unchanged', 'one two three', 'one two three', 8],
  ['emoji before', 'hi there', '👋🏽 hi there', 3],
  ['thai inside', 'สวัสดีครับ ทุกท่าน', 'สวัสดีค่ะ ทุกท่าน', 12],
  ['everything replaced', 'abc', 'xyz', 2],
  ['to empty', 'abc def', '', 4],
  ['from empty', '', 'abc', 0],
  ['repeated letters', 'aaaa', 'aaaaaa', 2],
];

const text = 'one two three four five six';
const reanchors: [string, Event[], Event[], unknown][] = [
  [
    'an event above grows',
    [plain('a', 'one'), plain('b', 'five six seven eight')],
    [plain('a', 'one two three four five six'), plain('b', 'five six seven eight')],
    { eventId: 'b', charOffset: 5, lines: 0 },
  ],
  [
    'an edit before the reader',
    [plain('a', text)],
    [plain('a', `zero ${text}`)],
    { eventId: 'a', charOffset: 19, lines: 0 },
  ],
  [
    'rewrapping is not an edit',
    [plain('a', text)],
    [{ ...plain('a', text), lines: makeTextLines(text, 20) as Line[] }],
    { eventId: 'a', charOffset: 19, lines: 0 },
  ],
  ['the event moves', [a, b, c], [b, a, c], { eventId: 'b', charOffset: 6, lines: 0.5 }],
  ['the event is deleted', [a, b, c], [a, c], { eventId: 'b', charOffset: 6, lines: 0.5 }],
  ['the last event is deleted', [a, b], [a], { eventId: 'b', charOffset: 6, lines: 0.5 }],
  ['the headings go', [a, b], [noHeadings(a), noHeadings(b)], { eventId: 'b', charOffset: 0, lines: -1 }],
  [
    'nothing is left',
    [a, b, c],
    [plain('x', 'seven'), plain('y', 'eight')],
    { eventId: 'b', charOffset: 6, lines: 0.5 },
  ],
  ['an empty script', [a, b, c], [], { eventId: 'b', charOffset: 6, lines: 0.5 }],
  ['no anchor yet', [a, b], [a, b], null],
  ['a new event the reader was not in before', [a], [a, b], { eventId: 'b', charOffset: 0, lines: 0 }],
  [
    'the first and the deleted are gone, the one before survives',
    [a, b, c],
    [a],
    { eventId: 'c', charOffset: 0, lines: 0 },
  ],
  [
    'thai edit in the reader event',
    [wrapped('t', 'สวัสดีครับ ยินดีต้อนรับทุกท่าน', 8)],
    [wrapped('t', 'สวัสดีค่ะ ยินดีต้อนรับทุกท่าน', 8)],
    { eventId: 't', charOffset: 11, lines: 0.25 },
  ],
];

process.stdout.write(
  JSON.stringify({
    sweeps: scripts.map(([name, events]) => ({ name, events, ...sweep(events) })),
    anchors: extraAnchors.map(([name, events, anchor]) => ({
      name,
      events,
      anchor,
      row: rowOfAnchor(makeTeleprompterLayout(events as never), anchor as never),
    })),
    edits: edits.map(([name, previous, next, offset]) => ({
      name,
      previous,
      next,
      offset,
      result: mapOffsetThroughEdit(previous, next, offset),
    })),
    reanchors: reanchors.map(([name, previous, next, anchor]) => ({
      name,
      previous,
      next,
      anchor,
      result: reanchor(previous as never, next as never, anchor as never),
    })),
  }),
);
