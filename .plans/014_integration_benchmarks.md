# 014 — Benchmarks for the integration path

Status: **planned (2026-11-25).** Item 5 of the RIIR 5 review.

## Why

There are no numbers for parse and dispatch yet. The global rules ask for
benchmark results, numbered in order, in an organised directory.

## Approach

- `packages/ontime-core/benches/integration.rs` with criterion. It measures
  `http_call`, `parse` and `dispatch` for `start/index`, `load/cue`, `change`
  and `poll`, on rundowns of 10, 100 and 1,000 events.
- Results go to `benchmarks/001_integration_baseline.md`, then `002_...` after
  each optimisation.
- Known candidate: `load_cue` and `load_index` build the timed order (a
  `Vec<String>` of cloned ids) once or twice per call. Iterating over borrowed
  ids would avoid the allocation. Only worth changing if the numbers show it
  matters next to a Durable Object request, which costs milliseconds.
