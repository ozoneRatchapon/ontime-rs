/**
 * Plan 008: the teleprompter contract, from the browser's side.
 * The same fixtures are replayed by packages/ontime-core/tests/teleprompter_*_tests.rs,
 * so the Rust host and this copy cannot drift without a red test.
 * Regenerate them with scripts/teleprompter-oracle/generate.sh after an upstream pull.
 */
import type { TeleprompterScriptEvent, TeleprompterTransport } from 'ontime-types';

import positionFixture from '../../../ontime-core/tests/fixtures/teleprompter_position.json';
import transportFixture from '../../../ontime-core/tests/fixtures/teleprompter_transport.json';

import {
  anchorAtRow,
  eventAtRow,
  makeTeleprompterLayout,
  mapOffsetThroughEdit,
  reanchor,
  rowOfAnchor,
} from './teleprompterPosition';
import {
  applyTransportCommand,
  describeSync,
  describeTransport,
  positionAt,
  reanchorTransport,
  settle,
  syncPositionAt,
  type TeleprompterTransportCommand,
} from './teleprompterTransport';

type Events = TeleprompterScriptEvent[];
type Step = Record<string, unknown>;
type Outcome = Record<string, unknown>;

const position = positionFixture.cases;
const transport = transportFixture.cases;

describe('position contract', () => {
  test.each(position.sweeps.map((sweep) => [sweep.name, sweep] as const))('%s', (_name, sweep) => {
    const layout = makeTeleprompterLayout(sweep.events as unknown as Events);
    expect(layout).toEqual(sweep.layout);
    for (const row of sweep.rows) {
      const anchor = anchorAtRow(layout, row.row);
      expect(anchor).toEqual(row.anchor);
      expect(anchor ? rowOfAnchor(layout, anchor) : null).toBe(row.back);
      expect(eventAtRow(layout, row.row)?.id ?? null).toBe(row.eventAt);
    }
  });

  test.each(position.edits.map((edit) => [edit.name, edit] as const))('edit: %s', (_name, edit) => {
    expect(mapOffsetThroughEdit(edit.previous, edit.next, edit.offset)).toBe(edit.result);
  });

  test.each(position.reanchors.map((r) => [r.name, r] as const))('reanchor: %s', (_name, r) => {
    expect(reanchor(r.previous as unknown as Events, r.next as unknown as Events, r.anchor as never)).toEqual(r.result);
  });
});

describe('transport contract', () => {
  const scripts = transport.scripts as unknown as Record<string, Events>;

  test.each(transport.scenarios.map((scenario) => [scenario.name, scenario] as const))('%s', (_name, scenario) => {
    let events = scripts[scenario.script];
    let layout = makeTeleprompterLayout(events);
    let current = scenario.initial as unknown as TeleprompterTransport;
    scenario.steps.forEach((raw, index) => {
      const step = raw as Step;
      const want = scenario.results[index] as Outcome;
      if ('cmd' in step) {
        try {
          current = applyTransportCommand(current, step.cmd as TeleprompterTransportCommand, layout, step.now as number);
        } catch (error) {
          expect((error as Error).message).toBe(want.error);
        }
      } else if ('settle' in step) {
        current = settle(current, layout, step.settle as number);
      } else if ('position' in step) {
        expect(positionAt(current, layout, step.position as number)).toBe(want.position);
      } else if ('describe' in step) {
        expect(describeTransport(current, layout, step.describe as number)).toEqual(want.state);
      } else if ('sync' in step) {
        expect(describeSync(current, layout, step.sync as number)).toEqual(want.sync);
      } else if ('syncAt' in step) {
        expect(syncPositionAt(describeSync(current, layout, 1), step.syncAt as number)).toBe(want.position);
      } else {
        const next = scripts[step.reanchor as string];
        const nextLayout = makeTeleprompterLayout(next);
        current = reanchorTransport(current, { events, layout }, { events: next, layout: nextLayout }, step.now as number);
        events = next;
        layout = nextLayout;
      }
      if (want.transport) expect(current).toEqual(want.transport);
    });
  });
});
