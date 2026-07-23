# Roadmap

A plan grounded in measured cost, not vibes. Every number here was measured, and
the reasoning for each phase is stated so a later decision can overrule it.

## Where things stand

Working on production (`ontime-server-rs.example.workers.dev` +
`ontime-client.example.pages.dev`):

- rundown CRUD with real timing (linkStart, timeStrategy, gap, dayOffset, delay)
- groups, clone, renumber, swap, reorder, batch edit
- playback: start/pause/stop/reload/load/addtime, end actions
- the timer engine shared between server and browser as WebAssembly, zero drift
- rooms: one isolated project per room, addressed by `/r/<room>` and `?room=`
- per-room password auth over REST and websocket
- project data, settings, view settings, custom fields, url presets, css override

Measured size: 2,948 lines of Rust against 20,046 lines of the TypeScript server.

### The cost table that drives everything below

| Module (`apps/server/src`) | LOC | Status |
|---|---|---|
| `api-data/rundown` | 3,314 | core done, edge cases remain |
| `api-data/automation` | 1,299 | **impossible on Workers** — OSC/UDP |
| `api-data/excel` | 1,122 | **impossible on Workers** — filesystem |
| `stores` | 1,084 | not started — the offset/roll core |
| `api-data/sheets` | 1,076 | **impossible on Workers** — OAuth session |
| `api-data/db` | 1,064 | **impossible on Workers** — filesystem |
| `services/runtime-service` | 952 | ~60% |
| `api-data/custom-views` | 333 | not started |
| `services/aux-timer-service` | 165 | not started |
| `api-data/session` | 150 | partial |
| `services/message-service` | 101 | not started |
| `api-data/report` | 85 | not started |

Two conclusions fall straight out:

1. **4,561 lines (23%) are not a backlog.** They cannot run on Workers at all. They
   should be struck from the target, not carried as debt. They only become
   reachable via Phase 3.
2. **501 lines buy back most of the visible gaps.** aux timers, message service,
   report and session are the buttons that currently do nothing when pressed.

### A correction on fork cost

Earlier notes warned that tracking a 36,701-line React client would be a heavy
ongoing cost. Measured, the divergence is:

```
apps/client: 8 files changed, 176 insertions(+), 11 deletions(-), 2 new files
```

~190 lines of surgical patches. Tracking upstream is cheap. This strengthens, rather
than weakens, the case against rewriting the UI in Dioxus.

---

## Phase 0 — make it safe to iterate — **DONE**

Nothing here adds a feature. It is what stops the next phases from being slower than
they need to be.

| Item | Where |
|---|---|
| e2e suites in the repo, with a runner | `apps/server-rs/tests/e2e/` + `run.sh` |
| upstream remote and rebase procedure | `docs/tracking-upstream.md` |
| CI | `.github/workflows/test_rust.yml` |
| timer flake made diagnosable | `tests/e2e/timer.mjs` detects socket closure |
| share links | `apps/client/src/common/utils/linkUtils.ts` |

Also done while getting CI green: clippy clean at `-D warnings` across all three
crates, and `cargo fmt` applied (a large mechanical diff — verified by re-running
all 51 unit tests and all 7 e2e suites afterwards).

1. **Track upstream.** `git remote -v` shows only this fork. Upstream fixes cannot
   currently be pulled at all. Add the upstream remote and write down the rebase
   procedure for the ~190 lines of client patches.
2. **Move the test suites into the repo.** Seven suites (`smoke`, `shapes`, `ws`,
   `timer`, `engine`, `rooms`, `auth` — 141 checks) live in a scratch directory and
   will vanish. They belong in `apps/server-rs/tests/` and run in CI alongside
   `cargo test`.
3. **CI that enforces the build order.** `deploy-cloud.sh` already encodes it and
   fails correctly; CI must use it rather than calling `pnpm build` directly.
4. **Resolve the timer-test flake.** It has failed once immediately after each of
   three deploys and passed on every rerun. The harness now dumps state on failure;
   the next occurrence should settle it. Do not let it become background noise —
   a test that fails 1-in-20 gets ignored, and then a real failure gets ignored too.
5. **Fix share links.** `handleLinks` builds URLs from `serverURL`, which is the
   Worker. It serves no HTML, so every "open in new tab" is a 404. Pre-existing, not
   caused by rooms. It should use the client origin.

**Exit:** a clean checkout can build, test and deploy with one command, and upstream
can be merged.

## Phase 1 — operator completeness — **DONE**

The client already rendered these controls; they now do something.

- **aux timers** — `packages/ontime-core/src/aux_timer.rs`, 18 tests
- **message overlay** — `packages/ontime-core/src/message.rs`
- **report** — same module, plus `GET/DELETE /data/report`
- **client list** — `apps/server-rs/src/room.rs`, stored on the socket attachment

Verified by `tests/e2e/operator.mjs`, 44 checks passing on production.

Three things worth recording:

- **Aux timers keep the tick alive by themselves.** `next_wake` only considered the
  playback runtime, so a stopwatch started while playback was stopped would have
  frozen. Caught by asserting playback stays `stop` while aux1 counts.
- **The connect packet and the broadcast packet were composed separately**, so the
  aux timers appeared on change but were missing on connect. They now share one
  `runtime_payload()`; the duplication was the bug.
- **A closing socket is still in `get_websockets()`** while `websocket_close` runs.
  The first version broadcast a client list that still included the leaving client.
  It is now excluded by id.

The client list lives in the socket's attachment rather than a field, because
hibernation evicts the object between messages — a field would not survive.

## Phase 2 — timing fidelity — **DONE**

`packages/ontime-core/src/offset.rs` ports `getRuntimeOffset` and
`getExpectedStart` exactly. The figure previously shipped was a single-event
approximation and could not be relied on.

`absolute` now accumulates: how late the loaded event started against its planned
slot, plus overrun, plus time spent paused, plus operator-added time. `relative`
measures the same against the plan as shifted by when the run actually began,
which is what you want when a show starts late by design. `countToEnd` counts only
the overrun.

Two things that had to be got right:

- **The day the run began is fixed at start.** Every later event is measured against
  it, so it cannot be recomputed as the rundown advances — which meant threading the
  timezone into `Runtime::apply`, since which day it is is a wall-clock question.
- **Paused time is measured in epoch, not time-of-day.** The original computes
  `clock - pausedAt` and carries a comment noting it breaks across midnight; ours
  does not have that problem.

The server also now keeps ticking while paused. It previously went quiet, which
froze the offset on screen — but a paused show is still losing time and that figure
is exactly what the operator is watching.

`expectedGroupEnd` and `expectedFlagStart` remain null: they need the
accumulated-gap walk over the rundown, which is not ported. Null rather than a
number that would look authoritative.

23 unit tests on the arithmetic, plus `tests/e2e/offset.mjs` on the wiring.

### Roll mode

`load_roll` ports the original's search: walk the playable events in order,
counting midnight crossings, and return the first that has not already finished.
Zero-duration events are skipped — rolling into one would leave playback stuck on
it forever. An overnight event later in the rundown is checked too, because at
00:10 the event that should be on air started yesterday.

Three behaviours worth naming:

- **Rolling mid-event joins it part way** rather than restarting it: the start time
  is back-dated by how far in we are.
- **Roll waits through a gap.** When an event ends, the next one is re-resolved
  against the clock rather than simply stepped, so dead air stays dead air instead
  of pulling the next event forward.
- **The server wakes at the exact moment** an armed event is due, not on the next
  heartbeat — in roll mode nothing else is going to start it.

Making this work exposed that `Playback::Roll` was invisible to the timer maths:
`elapsed`, `ends_at` and `is_running` all only recognised `Play`, so a rolling event
showed its full duration and the server would have stopped ticking. Caught by
asserting the joined event reads 40 minutes remaining rather than a fresh hour.

18 unit tests, plus `tests/e2e/roll.mjs` driving a real rundown against the clock.

## Phase 3 — the native target — **DONE**

`ontime-core` is already proven on three targets from one source: native (51 tests),
`wasm32` in a Worker, `wasm32` in a browser. A native server binary — axum reusing
the same core — costs an HTTP/websocket shell, not an engine.

Why this is the highest-leverage item despite not being a feature:

- It **dissolves the Workers-vs-VM decision.** Both, from one codebase, chosen per
  deployment rather than permanently.
- It is the **only** route to the 4,561 lines Workers structurally cannot host:
  automations/OSC, Excel import/export, project files, Google Sheets.
- It runs on an Oracle Always Free VM at the same 0 baht.

This is what "all Rust" should mean in practice: one engine, every host.

**Exit met.** `apps/server-native` answers the same ten suites as the Worker, and
CI runs them against both.

### What actually moved

`ontime-core::api` now owns the HTTP routing, the auth rule, the wake policy and
the websocket command handling. The Worker went from **1,345 lines to 487** and is
an adapter: translate the request, call the core, carry out the effects.

`Effect` is the seam. The core returns *what must happen* — persist, tell clients,
reschedule — and each host decides *how*, because a Durable Object alarm and a
tokio interval have nothing in common.

### Three drifts the work exposed

Each was a behaviour that lived in the Worker rather than the core, and each meant
the second host silently lacked it. Each was caught by the suites, not by reading:

| Drift | Consequence |
|---|---|
| the websocket upgrade bypassed the auth check | `/ws` was briefly unauthenticated |
| the tick cadence ignored `heartbeat_ms` | the native host broadcast twice a second regardless of configuration |
| `record_report` sat in the Worker | the run report was never recorded natively |

All three were fixed by moving the behaviour into the core. `next_wake_in` and
`apply_socket_command` exist because of them.

The client list is the one thing each host still owns, because the connections are
its own: the Worker keeps it on the socket attachment, the native server in a map.

### What this unlocks

The 4,561 lines Workers structurally cannot host — automations and OSC, Excel
import/export, project files, Google Sheets — are now reachable from a process
that has a filesystem and can open sockets. None of them are built; this is the
foundation they needed.

And the Workers-versus-VM choice is no longer a choice made once: the same code
runs on both, decided per deployment.

## Phase 4 — deliberately not doing

- **Rewriting the UI in Dioxus.** 36,701 lines of React against ~190 lines of local
  patches. The UI is where performance matters least and work is greatest, and
  upstream keeps moving. Revisit only if a concrete need appears that React cannot
  meet — language consistency is not such a need.
- **Excel / Sheets / project files on Workers.** They only make sense on Phase 3's
  native target. Building shims for them on Workers is wasted work.

---

## Sequencing

Phase 0 first — it is small and everything after it is slower without it.

Then **Phase 1 before Phase 2**: Phase 1 is a quarter of the cost and removes the
"I pressed the button and nothing happened" class of problem, which is what makes
software feel broken. Phase 2 is deeper but affects a number most users read rather
than act on.

Phase 3 can start in parallel with either — it touches no existing behaviour.

## Where this ended

Phases 0 through 3 are done. Measured against the stopping rule below: a show can
be run from this without the desktop app, provided it needs no OSC, no file
import and no Sheets — and those are now buildable rather than impossible.

Remaining, in the order they would be worth doing:

1. **Custom views** — still an empty stub.
2. **The features the native target unlocks** — automations/OSC, Excel, project
   files, Sheets. Each is a real piece of work; none is blocked any more.
3. **Room provisioning** — any name still creates a room on first use.
4. **`SESSION_PASSWORD` is not set** on the deployment. Without it a forgotten
   room password makes that room unreachable. One `wrangler secret put`.

## How to know when to stop

The target is not "all 20,046 lines". It is: **can this run a real show without the
operator needing the desktop app?** Concretely — Phases 0-2 complete, and either
Phase 3 done or the show genuinely needs no OSC, no file import and no Sheets.

Everything past that point is worth doing only if a specific show needs it.

## Open risks

- **Any room name creates a room.** If rooms should be provisioned rather than
  self-service, that needs a registry and the master key gating creation.
- **`SESSION_PASSWORD` is not set.** Without it a forgotten room password makes that
  room permanently unreachable. Setting it costs one `wrangler secret put`.
- **One project per Durable Object value, 2 MB ceiling.** Not close yet; splitting
  per rundown is the fix when it matters.
- **Response shapes.** Three endpoints returned 200 with the wrong body because the
  sweep only checked status codes. `shapes-test.mjs` now asserts field types for
  thirteen endpoints — every new endpoint needs an entry there.
