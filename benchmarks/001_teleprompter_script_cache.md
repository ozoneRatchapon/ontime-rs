# 001 — Teleprompter script: build vs cache hit

- **Date:** 2026-11-24.
- **Machine:** Apple M5 Pro, release build, native.
- **Run:** `cargo run --release --example teleprompter_build_bench` in
  `packages/ontime-core`.
- **Input:** a rundown of 100 events, half English notes of about 500
  characters and half Thai notes of about 400 characters (3 bytes each in
  UTF-8). That makes 1,800 script lines at 32 characters per line.

|                                                   | per call |
| ------------------------------------------------- | -------- |
| build the script (wrap every event)               | 1.03 ms  |
| cache hit, first fingerprint (`Value::to_string`) | 124 µs   |
| cache hit, strings hashed as bytes                | 88 µs    |
| cache hit, `Mutex` instead of `RefCell`           | 89 µs    |

**What a cache hit costs:** a blake3 pass over the text the script reads. That
is the price of a key that cannot go stale even when an edit skips
`recompute`.

**What this means for the hosts:**

- The native host checks its wake time every 50 ms while the prompter plays.
  That was about 2% of a core per room and is now about 0.2%.
- The Worker builds the script once per change instead of once per runtime
  broadcast and wake check.
