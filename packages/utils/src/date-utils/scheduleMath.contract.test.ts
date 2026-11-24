/**
 * The schedule-math contract: the same fixture file is run by
 * packages/ontime-core/tests/schedule_math_tests.rs, so the Rust and
 * TypeScript copies cannot drift apart without a red test.
 */
import type { Day, MaybeNumber } from 'ontime-types';
import { OffsetMode } from 'ontime-types';

import fixture from '../../../ontime-core/tests/fixtures/schedule_math.json';

import { getExpectedEnd, getExpectedStart } from './getExpectedStart';

type StartCase = {
  name: string;
  event: { timeStart: number; dayOffset: Day; delay: number };
  state: {
    currentDay: number;
    totalGap: number;
    isLinkedToLoaded: boolean;
    offset: number;
    mode: 'absolute' | 'relative';
    actualStart: MaybeNumber;
    plannedStart: MaybeNumber;
  };
  expected: number;
};

type EndCase = {
  name: string;
  event: { timeStart: number; dayOffset: Day; duration: number; countToEnd: boolean };
  expectedStart: number;
  currentDay: number;
  expected: number;
};

const modes: Record<StartCase['state']['mode'], OffsetMode> = {
  absolute: OffsetMode.Absolute,
  relative: OffsetMode.Relative,
};

const startCases = fixture.expected_start as StartCase[];
const endCases = fixture.expected_end as EndCase[];

describe('schedule math contract: getExpectedStart()', () => {
  test.each(startCases.map((c) => [c.name, c] as const))('%s', (_name, c) => {
    expect(getExpectedStart(c.event, { ...c.state, mode: modes[c.state.mode] })).toBe(c.expected);
  });
});

describe('schedule math contract: getExpectedEnd()', () => {
  test.each(endCases.map((c) => [c.name, c] as const))('%s', (_name, c) => {
    expect(getExpectedEnd(c.event, c.expectedStart, c.currentDay)).toBe(c.expected);
  });
});
