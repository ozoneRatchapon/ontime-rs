# 005 — D2: one copy of the schedule math, in Rust

Status: **step 1 done (2026-11-24); step 2 waiting for approval.**

## Step 1 result

- **The fixture:** `packages/ontime-core/tests/fixtures/schedule_math.json`.
  - 30 `expected_start` cases: every upstream case, plus a clamped negative
    delay, a positive delay, 23:50 pushed past midnight, a next-day event
    whose overnight gap absorbs lateness, and relative mode with nulls.
  - 6 `expected_end` cases, including count-to-end on the running day.
- **The runners:**
  - `packages/ontime-core/tests/schedule_math_tests.rs`;
  - `packages/utils/src/date-utils/scheduleMath.contract.test.ts`.
- **Rust gained `offset::expected_end`**, ported from `getExpectedEnd`. It is
  used only by the contract for now.
- **Result:** both pass, and no differences were found.
  - Changing one expected value turns both suites red, so the contract does
    catch drift.
- **Not covered yet:** the linked-to-loaded and gap look-ahead.
  - The TS side has no standalone `getLinksToLoaded` in our tree.
  - The Rust side keeps it inline in `runtime.rs`.
  - Covering it needs a small extraction on both sides. That belongs in step 2.
- **Decision 1 answered:** the copies agree today, so unifying them is about
  stopping future drift, not fixing a bug. Step 2 is still worth doing, but it
  isn't urgent.

## The problem

The same calculations exist twice, and nothing checks that the copies agree:

| calculation | Rust (server, `ontime-core`) | TypeScript (browser) |
|---|---|---|
| expected start of an event | `offset::expected_start` | `getExpectedStart` (`packages/utils`) |
| expected end | not yet | `getExpectedEnd`, ported on 2026-11-24 for D3 |
| gap / linked-to-loaded chain | `runtime.rs` look-ahead (`accumulated_gap`, `is_linked_to_loaded`) | `getLinksToLoaded` and `rundownMetadata.ts` (upstream d9d0237c) |
| day offset | `offset::find_day_offset` | inside the same TS helpers |

- The server uses its copy for runtime-data: offset, the flag and the group end.
- The browser uses its copy for the countdown, timeline and overview screens.
- When the two disagree, two screens in the same room show different times.
- Nothing would notice.

## Approach: two steps, each worth having on its own

### Step 1: a shared contract, cheap and done first

- A fixture file, `packages/ontime-core/tests/fixtures/schedule_math.json`.
- Each case holds the inputs (event fields, current day, offset, mode, actual
  and planned start, total gap, linked) and the expected outputs.
- **Both sides run the same file:**
  - `cargo test` in `ontime-core`, through a new `schedule_math_tests.rs`;
  - `vitest` in `packages/utils`, through a new `scheduleMath.contract.test.ts`.
- **Where the cases come from:**
  - every case in upstream's `getExpectedStart.test.ts`,
    `getLinksToLoaded.test.ts` and the countdown tests;
  - edge cases: across midnight, a negative delay, relative mode, an event the
    next day, count-to-end.
- **Done when:** both suites pass on the same fixtures. Any difference found is
  fixed toward upstream's behaviour, test first.
- After this, the two copies cannot drift without a red test.

### Step 2: the browser uses the Rust copy

- **Add `ontime-wasm` exports:**
  - `expected_start`, `expected_end` and `links_to_loaded`, taking and
    returning plain numbers or JSON;
  - add `expected_end` to `offset.rs` first.
- **Load the wasm before the first render.**
  - Today `initTimerEngine()` is fire-and-forget, so a synchronous call during
    render may find no wasm yet.
  - Change `index.tsx` to `await initTimerEngine()` before `root.render`. The
    wasm is 107 KB and already downloaded on every page, so only the order
    changes.
- **`packages/utils` keeps the same function names.** They call the wasm when
  it is loaded and the TS body otherwise, for unit tests and old browsers. The
  contract from step 1 keeps both honest.
- **Done when:**
  - the countdown, timeline and overview read their times from the wasm;
  - the contract passes;
  - the browser checks (`views.mjs`, `upstream.mjs`) pass;
  - the first-render time is measured before and after. Acceptable cost: under
    50 ms on a mid-range laptop.

## What stays TypeScript

Anything about display: formatting, colours, labels. Only the math moves.

## Risks

- **Upstream changes its TS math** (it did twice this autumn). Each change then
  needs a Rust change too.
  - Mitigation: the contract fails, and we see it when we pull.
  - This is the price of one copy. It is smaller than two silent copies.
- **A wasm load failure** (old browser, blocked by a policy): the TS fallback
  covers it.

## Size

- Step 1: about a day.
- Step 2: about two days, with the first-render measurement.

## Decisions to confirm

1. Step 1 first, and step 2 only if step 1 finds the copies worth unifying.
2. Await the wasm before the first render. The alternative is to keep the TS
   path until it has loaded, which is more complicated.
