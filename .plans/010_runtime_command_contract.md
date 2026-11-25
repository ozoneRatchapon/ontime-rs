# 010 — Pin every playback command's effect against upstream (RIIR 6)

Status: **done and deployed (2026-11-25), Worker `f4c09ec0`.** Item 1 of the five from the RIIR 5 review.
The others are plans 011–014.

## Why

Plan 009 pins what a request turns into: which service call, with which
arguments. It does not pin what that call then does to the timer. Two
differences are already known:

- **start while rolling.** Upstream keeps the event's `startedAt` and switches
  to play. Ours restarts the event from now, so the timer jumps.
- **reload.** Upstream loads the event again and leaves it armed. Ours starts
  it again if it was playing.

Mixed sequences, such as next while paused or load next while rolling, have
never been compared at all.

## Approach

The RIIR 3 oracle (`gen-runtime_offset.ts`) already drives upstream's real
`runtimeState` through scripts of operator actions, and compares the timer,
offset and rundown sections. Extend it:

1. Make its actions go through the same checks `runtime.service` applies, as
   the integration API reaches them:
   - `validatePlayback` for start, pause and stop;
   - `handleLoadNext` and `handleLoadPrevious`, which use findNextRollEvent
     under roll and skip unplayable events;
   - `reload`, which is `loadEvent(eventNow)`;
   - `startNext` and `startPrevious`, which load and then start.
2. Add the actions `next`, `previous`, `startnext`, `startprevious` and
   `reload` to the generator and the test.
3. Add cases:
   - start while rolling, pending and running;
   - reload while playing and while paused;
   - next and previous while paused;
   - next while rolling;
   - next from the last event;
   - startnext from playing;
   - pause while rolling;
   - stop and start again.
4. Fix every difference in `Runtime`, or list it in `DELIBERATE` with the
   reason.

## Done when

- Every new case matches, or is listed as deliberate with a reason.
- ontime-core tests, clippy and both e2e hosts are green.
- Deployed, with the production smoke run passing.

## Result

- 12 new cases are driven through upstream's real `runtimeState`, with
  `runtime.service`'s checks applied. 10 matched at once.
- Both known differences are fixed in `Runtime`:
  - start while rolling keeps `startedAt` and switches to play;
  - reload loads the event again, armed, and starts nothing.
- The unit test that pinned the old reload, written with the engine in
  b7a29b3c3 with no reason given, now pins upstream's reload.
- Still deliberate: `start` with nothing loaded starts the first event, where
  upstream refuses. Unit and e2e tests rely on it, and the editor cannot reach
  it, because upstream's controls disable Start while stopped.
- ontime-core: 355 tests pass. e2e: every suite passes on wrangler and on the
  native server.
