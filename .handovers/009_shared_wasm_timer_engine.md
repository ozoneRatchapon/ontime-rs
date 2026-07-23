# Handover 009: One Timer Engine, Two Hosts

The rundown and timer logic in `ontime-core` now runs in **both** the Cloudflare
Worker and the browser, compiled from the same Rust source. The server stopped
ticking; the browser advances the timer itself.

## 1. Why

The 1 Hz alarm from handover 008 was the entire cost model — 3,600 Durable Object
requests and 3,600 row writes per hour of playback — and it also meant the client
could only ever be as smooth as the network. The client is a passive renderer: it
draws whatever the last `runtime-data` packet said.

Moving the per-frame maths into the browser fixes both, and it is only safe to do
because it is the *same code*, not a reimplementation in TypeScript that would
quietly diverge.

## 2. What changed

### `packages/ontime-core/src/runtime.rs`

Extracted `TimerInputs` — everything the per-tick computation needs, decoupled from
the rundown:

```rust
pub struct TimerInputs {
    playback, started_at, paused_at, paused_total,
    added_time, planned_duration, time_warning, time_danger, has_event,
}
```

with `elapsed()`, `duration()`, `current()`, `phase()`, `timer_state()` and
`ends_at()`. `Runtime::snapshot` was rewritten to call `timer_state()` rather than
computing inline, so there is exactly one implementation. All 49 existing tests
passed unchanged through the refactor, which is what makes it credible.

`ends_at()` is new: the epoch millisecond at which the loaded event runs out. It is
what lets the server schedule one alarm per event instead of one per second.

### `packages/ontime-wasm` (new)

`wasm-bindgen` bindings exposing `OntimeTimer` to the browser:

- `update(engine_json, client_now)` — feed the `_engine` block of a packet. Records
  `serverNow - clientNow` as **skew**, so a browser with a wrong system clock still
  counts the same event boundaries as the server.
- `tick(client_now)` — the `TimerState` for this frame, as JSON.
- `clock(client_now)` — milliseconds since local midnight.
- `is_running()` — lets the caller stop requesting frames.

Built by `packages/ontime-wasm/build.sh`. `wasm-pack` is not used — the installed
binary is the wrong architecture on this machine — so the script drives
`cargo` → `wasm-bindgen` → `wasm-opt` directly.

> The `wasm-bindgen` **crate** and **CLI** versions must match exactly or the schema
> check fails. Both are pinned at 0.2.126.

Output: **107 KB, 48 KB gzipped.**

### `apps/server-rs/src/room.rs`

`runtime-data` now carries an extra `_engine` block (not part of the client's
`RuntimeStore`, which ignores unknown keys):

```json
"_engine": { "serverNow": 1700000000000, "tzOffset": 25200000, "inputs": { … } }
```

`sync_alarm` was replaced by `next_wake`, which wakes only for a reason:

- at the exact moment the loaded event ends, and **only if** its `endAction` is not
  `none` — otherwise there is nothing to do at the boundary
- every `Project::heartbeat_ms` as a safety net

Idle still costs exactly nothing.

**Why keep any heartbeat.** A client that is not running the WASM engine — an old
cached bundle, or something driving the socket directly — would show a frozen timer
if the server went completely silent. That is an unacceptable failure mode for live
show control.

**It is tunable at runtime**, not a compile-time constant, because it trades cost
against how stale a non-WASM client may look and that balance is a judgement call:

```bash
curl -X POST $WORKER/data/settings -H 'content-type: application/json' \
     -d '{"heartbeatMs": 1000}'
```

Returned from `GET /data/settings`. Clamped to 250–60000 ms; `0` disables the safety
net entirely, leaving only the end-of-event wake — the cheapest the server can run.
The default is **1000 ms**, matching the original server's `notificationRate`, so a
client that cannot run the local engine behaves exactly as it does against
`apps/server`. Projects stored before this change pick the default up through
`#[serde(default)]` with no migration.

### `apps/client`

Three small edits to the vendored client, plus one new file:

- `src/common/utils/timerEngine.ts` (new) — loads the WASM module, feeds it each
  packet, and patches the runtime store from a `requestAnimationFrame` loop throttled
  to 50 ms. `requestAnimationFrame` pauses on a hidden tab, and the first frame after
  it wakes recomputes from the absolute start time, so nothing drifts.
- `socket.ts` — `initTimerEngine()` on connect, `applyEngineState(payload._engine)`
  on `runtime-data`, `stopTimerEngine()` on disconnect.
- `vite.config.js` — an alias for `ontime-timer-engine` pointing at the built `pkg/`,
  rather than adding a dependency to the vendored client's `package.json`.

**Everything degrades.** If the module fails to load, `applyEngineState` returns
false and the store keeps showing what the server sent, which the heartbeat refreshes.

## 3. Verification

`scratchpad/engine-test.mjs` loads the real `.wasm` the browser loads, drives real
playback against the Worker, and compares the browser's numbers to the server's.

Against the deployed Worker:

```
== local engine agrees with the server at the same instant ==
  ok   duration / addedTime / playback / phase match
  ok   current within 5ms (drift 0ms)
== still agrees after several seconds ==
  ok   identical to the server (drift 0ms)
  ok   6s without a packet, still within 500ms (gap 34ms)
== local engine advances between server packets ==
  ok   counts down 1000ms in 1s (got 1000)
== server packet rate while running ==
  ok   ~10 packets in 10000ms at a 1000ms heartbeat
       => ~3600 server wake-ups per hour (at the 1000ms default)
== nothing is sent while stopped ==
  ok   no packets while stopped
```

**Drift is 0 ms**, on localhost and across the internet.

Two comparisons are made deliberately. Seeding a fresh engine from the packet under
test isolates the invariant — same inputs, same instant, same answer — while the
long-lived engine measures what a real client experiences between packets. An
earlier version of this test compared the long-lived engine against a packet
timestamped by the *server*, which reported 95 ms of "drift" that was really one-way
network latency; the engine was never wrong.

Full suite against production, all passing:

| Suite | Result |
|---|---|
| `cargo test -p ontime-core` | 49 |
| REST smoke | 40 checks |
| WebSocket | 6 checks |
| Timer over websocket | 20 checks |
| Browser engine vs server | 18 checks |

`apps/client` builds and emits `ontime_timer_bg-*.wasm` as an asset.

## 4. Cost, revised

Measured, by setting `heartbeatMs` and counting packets:

| `heartbeatMs` | DO requests + rows / hour | Playback headroom / day |
|---|---|---|
| 1000 (default) | 3,600 | ~27 h |
| 5000 | 720 | ~138 h |
| 0 (end-of-event only) | ~1 per event | effectively unbounded |

Idle is 0 at every setting.

What the WASM engine changes regardless of the heartbeat:

| | Before (008) | Now |
|---|---|---|
| Timer smoothness | 1 Hz | frame rate |
| Survives a network gap | no | yes |
| Timer maths | server only | one engine, both hosts |

The default trades cost for safety: at 1000 ms the running cost is the same as
before, but the timer is now smooth and gap-tolerant, and the cost can be cut 5x
whenever you decide every client runs the engine.

## 5. Build and deploy

Use `./deploy-cloud.sh` rather than running the steps by hand — the ordering has two
traps that both produce a *silently* broken deploy:

1. `packages/ontime-wasm` must be built before `apps/client`. The client resolves
   `ontime-timer-engine` to its `pkg/` output through a vite alias, and `pkg/` is a
   gitignored build artifact.
2. `apps/client` must be built with `VITE_WORKER_URL`. Without it the client falls
   back to `window.location.origin`, which on Pages is a static host with no
   backend — every API call 404s and the editor renders blank (handover 006).

```bash
./deploy-cloud.sh                # worker + client
./deploy-cloud.sh --worker       # worker only
./deploy-cloud.sh --client       # client only
./deploy-cloud.sh --dry-run      # build and check, deploy nothing
```

The script refuses to deploy unless the built bundle references the configured
worker URL, contains no localhost URL, and emitted a `.wasm` asset. Both failure
paths were tested by deliberately triggering them; the script exits 1.

CI needs the same ordering — running `pnpm build` alone will not produce a working
client.

## 6. Route fixes found by opening the app in a browser

The first real browser session surfaced two endpoints returning 404 in a React Query
retry loop — noisy, and each retry is a billable request:

| Path | Cause | Fix |
|---|---|---|
| `/data/automations` | the router was written singular; the original mounts `/automations` | renamed to plural |
| `/user/translations/translations.json` | the original serves `/user` from a static directory; ours had no handler | returns `{}` — "no custom translations", which the client merges over `langEn` |

A systematic diff of `apps/server/src/api-data/index.ts` against `room.rs` confirmed
these were the only two mismatched names out of fourteen mounts. Both are now in the
endpoint sweep (`scratchpad/coverage.sh`), which reports no remaining 404 or 500 on
any path the client requests.

## 7. What this unlocks

`ontime-core` is now proven to run on three targets from one source: native (the
49 tests), `wasm32` in a Worker, and `wasm32` in a browser. A native server binary
(axum) reusing the same core would run on a VM without touching the engine — which
means the Workers-vs-VM decision from handover 008 no longer has to be permanent.

Still not ported: roll mode, aux timers, message service, report, custom views, and
the rundown-wide offset calculation. See handover 008 §5 for what Workers cannot do
at all.
