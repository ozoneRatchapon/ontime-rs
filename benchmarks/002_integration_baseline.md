# 002 — Integration calls: baseline

- **Date:** 2026-11-25.
- **Machine:** Apple M5 Pro, release build, native.
- **Run:** `cargo run --release --example integration_bench` in
  `packages/ontime-core`, at commit e3c86ca3c (before 003).
- **Input:** a rundown of 10, 100 or 1,000 five-minute events. Each figure is
  the median of 9 batches, per call.

| call                                                 | 10 events | 100 events | 1,000 events |
| ---------------------------------------------------- | --------- | ---------- | ------------ |
| `http_call` + `parse`, `/start/index/3`              | 306 ns    | 191 ns     | 182 ns       |
| `http_call` + `parse`, `/change/<id>?title=My+Title` | 189 ns    | 189 ns     | 188 ns       |
| dispatch `start` by index (the last event)           | 5.79 µs   | 64.6 µs    | 666 µs       |
| dispatch `load` by cue (the last event)              | 7.19 µs   | 82.4 µs    | 849 µs       |
| dispatch `load` next                                 | 6.01 µs   | 67.7 µs    | 687 µs       |
| dispatch `addtime`                                   | 5.38 µs   | 59.5 µs    | 598 µs       |
| dispatch `change` title (re-times the rundown)       | 4.23 µs   | 45.3 µs    | 995 µs       |
| dispatch `poll` (the whole runtime payload)          | 7.34 µs   | 36.1 µs    | 357 µs       |

**What stood out:** `addtime` only adds a number, yet it cost 598 µs on a
large rundown. Every playback command copied the whole rundown first
(`project.current().cloned()`), and so did every alarm (`on_alarm`), which
runs while an event plays. See 003.

Parsing is flat at about 0.2 µs. It does not depend on the rundown.
