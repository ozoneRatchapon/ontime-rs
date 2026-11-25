# 014 — Benchmarks for the integration path

Status: **done and deployed (2026-11-25), Worker `f154eadf`.** Item 5 of the RIIR 5 review.

## Why

There are no numbers for parse and dispatch yet. The global rules ask for
benchmark results, numbered in order, in an organised directory.

## Approach

- `packages/ontime-core/examples/integration_bench.rs`, timed with `Instant` like
  the existing teleprompter bench, so no criterion dependency. It measures
  `http_call`, `parse` and `dispatch` for `start/index`, `load/cue`, `change`
  and `poll`, on rundowns of 10, 100 and 1,000 events.
- Results go to `benchmarks/002_integration_baseline.md`, then the next
  number after each optimisation (001 was already taken).
- Known candidate: `load_cue` and `load_index` build the timed order (a
  `Vec<String>` of cloned ids) once or twice per call. Iterating over borrowed
  ids would avoid the allocation. Only worth changing if the numbers show it
  matters next to a Durable Object request, which costs milliseconds.

## Result

- `benchmarks/002_integration_baseline.md`: the baseline. `addtime` cost
  598 µs on 1,000 events, because every playback command, and every alarm,
  copied the whole rundown first.
- `benchmarks/003_integration_no_rundown_copy.md`: the copy is gone
  (`Project::runtime_and_current` borrows the two fields apart), and the
  selection helpers borrow ids.
  - `addtime`: 598 µs → 35 ns.
  - `start` by index: 666 → 60 µs.
  - `load` next: 687 → 78 µs.
  - The alarm while an event plays: under 30 ns at any size, where it paid
    60 µs at 100 events.
- Left alone on purpose: caching the event orders per rundown revision.
  `load` by cue takes 20 µs at 100 events, against a request of several
  milliseconds.
