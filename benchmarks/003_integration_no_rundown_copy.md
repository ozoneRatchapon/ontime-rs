# 003 — Integration calls: no copy of the rundown

- **Date:** 2026-11-25.
- **Machine:** Apple M5 Pro, release build, native.
- **Run:** `cargo run --release --example integration_bench` in
  `packages/ontime-core`. Same input as 002.
- **Changes since 002:**
  1. `Project::runtime_and_current` borrows the runtime and the rundown as
     two separate fields, so a command and the alarm read the rundown in
     place instead of copying it.
  2. `selection.rs` returns ids borrowed from the rundown (`Vec<&str>`).
     Only the id a command picks is copied.

| call                                           | 10 events | 100 events | 1,000 events | vs 002, 1,000 events |
| ---------------------------------------------- | --------- | ---------- | ------------ | -------------------- |
| dispatch `start` by index (the last event)     | 0.63 µs   | 4.55 µs    | 59.5 µs      | 11× faster           |
| dispatch `load` by cue (the last event)        | 1.81 µs   | 19.7 µs    | 232 µs       | 3.7× faster          |
| dispatch `load` next                           | 0.75 µs   | 7.25 µs    | 78.1 µs      | 8.8× faster          |
| dispatch `addtime`                             | 34 ns     | 29 ns      | 35 ns        | 17,000× faster       |
| dispatch `change` title (re-times the rundown) | 4.42 µs   | 45.5 µs    | 911 µs       | unchanged            |
| dispatch `poll` (the whole runtime payload)    | 7.10 µs   | 31.4 µs    | 316 µs       | 1.1× faster          |
| `on_alarm` while an event plays                | 25 ns     | 27 ns      | 28 ns        | see below            |
| copying the rundown (what 002 paid each time)  | 5.19 µs   | 60.1 µs    | 599 µs       | —                    |

**The alarm is the one that matters.** It runs on every wake while an event
plays: at its end, and on each heartbeat for screens that are not running
the wasm engine. Before, each wake paid the copy in the last row, 60 µs at
100 events. Now a wake costs under 30 ns, whatever the size of the rundown.
On the Worker, that is CPU time billed per room.

Change 2 is worth only 10–15% (`poll` 357 → 316 µs). What is left grows with
the rundown because entries are stored as JSON values: walking the order
looks up each entry and reads its `type` and `skip` fields. The next step
would be to cache the orders per rundown revision. At 100 events the slowest
command, `load` by cue, takes 20 µs against a Durable Object request of
several milliseconds, so it is not worth doing yet.

`change` is unchanged. It re-times the whole rundown (`recompute`), as an
edit in the editor does.
