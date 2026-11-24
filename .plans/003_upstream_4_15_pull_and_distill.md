# 003 — What to take from upstream ontime v4.11 → v4.15.2

Status (2026-11-24):
- **A1–A9 deployed.** Worker version `f366b3c1`; the rollback point is `1bf1ba98`.
- **B1 + B2 deployed (client, 2026-11-24, Pages `f2fcd906`)** from branch `feature/upstream-4.15`, which is not merged or pushed.

## B — what was done (branch `feature/upstream-4.15`)

- **B1:** 90 of the 97 clean patches.
  - Seven were left out because each needs a patch we did not pull:
    - timeline secondary text;
    - share link offers all views;
    - sentry logging;
    - rundown cursor across editor layouts;
    - cuesheet delay rows;
    - quick add menu UX;
    - title list follows the live event.
  - Most depend on 979de757, "scope rundown data sources": 52 files, still to pull.
- **B2:** client side of
  - 2436ec2f: silently dropped sockets;
  - 4c656950: unreported close;
  - 6e02d3a9: watchdog backoff;
  - 43cbce05: disconnected state;
  - 58774ecd: the displayed clock compensates for network delay, using A1's `clock`;
  - 1191d467: Pong matching.
  - Plus our own fix: the local timer engine stops on every lost socket, the
    watchdog's included.
- **Aux timer naming, client side (84b28083):** the settings panel and the names in
  the studio view, which uses A2.
- **Dependencies:** a lockfile for the patches' bumps. `@tanstack/react-table`
  went from 8 to 9.
- **Two upstream test cases are skipped,** each with a note. Their code is in
  patches not pulled yet:
  - 759ea78e, countdown with skipped events. It needs a `getExpected` refactor,
    so it is left for D3.
  - An Excel-upload message test, which needs 979de757.

Tests:
- client typecheck: only the 3 known errors;
- client unit tests: 386 pass, 2 skipped;
- utils unit tests: 428 pass;
- e2e: 261 checks, plus admin 34, people 35, members 36;
- browser:
  - people and members walk-throughs;
  - `upstream.mjs`, 9 checks: ping/pong with clock, aux names in the studio
    view, the room time on the timer view;
  - reconnect after a Worker restart.

## A — what was done

| # | result | where |
|---|---|---|
| A1 | Pong carries `clock`, the room's time of day (its `timezoneOffset`) | `socket.rs`, `room.rs` |
| A2 | `settings.auxTimerNames` (3, trimmed, ≤30 chars); each aux timer state has `name` | `aux_timer.rs`, `project.rs` |
| A3 | `PUT /data/rundowns/:id {title?, entries}` replaces in place; playback continues while the loaded event is still playable. `seed-room --replace` uses it and keeps each cue's entry id | `project.rs`, `rundown_routes.rs`, `scripts/seed-room.mjs` |
| A4 | edits drop values for unknown fields; renaming a field moves its values and deleting removes them, in every rundown; unknown field → 404, type change / rename onto another field → 400 | `custom_fields.rs` |
| A5 | upstream's refusal table (409 duplicate id, 422 bad parent / nested group / type change / wrong-type applydelay or ungroup, 404 missing anchor); `parent` now places an entry in its group | `rundown.rs`, `rundown_routes.rs` |
| A6 | share URL needs `scheme://host` and a path → otherwise 422 | `api.rs` |
| A7 | refetches were already sent; added: a settings write keeps the stored `version`; any write that leaves the loaded event unplayable (deleted, skipped, cleared) stops playback | `api.rs`, `project.rs` |
| A8 | already correct (search by id in playable order); pinned with tests | `lookahead_tests.rs` |
| A9 | rundown titles: any script, ≤64 chars, at least one letter/number → otherwise 422 | `rundown.rs` |

Bugs found on the way, all fixed:
- `PATCH …/renumber` ignored the client's `{ids, prefix, start, increment}` and
  renumbered **every** cue from 1.
- A badly typed `POST /data/settings`, `/data/project` or `/data/view-settings`
  silently reset the whole document to defaults. Now a 400 that changes nothing.
- Adding an entry with `parent` and no anchor inside that group left it at the
  top level while claiming the group. Production has one such entry, in
  `raknong_4ever` (not touched).

Tests: core 283; e2e all suites plus the new `upstream.mjs` (26 checks); admin 34,
people 35, members 36.

`api.rs` was split: the rundown routes now live in `rundown_routes.rs`, which
brought `api.rs` under the 1024-line limit.

## Where we stand

- Our repo started from a **snapshot** of ontime v4.11.0. It shares no git
  history with `cpvalente/ontime`, so `git merge` is impossible. Upstream changes
  come over as patches (`git format-patch` → `git am -3`).
- Upstream since v4.11.0:
  - 277 commits;
  - 37 features, 80 fixes and about 10 perf changes;
  - client, utils and types: 500 files, +20.5k / −5.6k lines;
  - `apps/server`: 236 files, +15.7k / −5.6k lines. That is TypeScript we have
    replaced with Rust, so its changes are **reference only**.
- Our own client has drifted from v4.11.0 by 52 files (+2.4k), mostly the admin,
  people and room-token work.

### Trial run

The trial ran in a throwaway worktree at `/tmp/ontime-pull-trial`.

- Of the 173 upstream patches that touch the client, utils or types, **97
  apply cleanly** and **76 conflict**.
- The 97 clean ones together change 254 files (+7.2k / −3.0k).
- They have **not** been built or tested against our Rust server yet.

## Already fine in our Rust (checked, no action)

| upstream fix | why ours is unaffected |
|---|---|
| paused countdown breaks across midnight | our runtime counts in epoch ms, not time-of-day |
| moving an entry "before" a later one lands one too far | `rundown.rs` unlinks then re-links by id, so there is no index shift |
| unauthenticated websocket gets data | our upgrade is refused before the socket opens (`room.rs`) |

## A. Server behaviour to distill into Rust

Upstream is the spec. Each item gets a core test first.

| # | upstream change | why it matters to us | size |
|---|---|---|---|
| A1 | Ping is answered with the server time of day | newer clients compensate the displayed clock with it | S |
| A2 | name aux timers | client feature needs the field stored | S |
| A3 | replace a rundown without stopping playback | exactly what `seed-room --replace --live` wants | M |
| A4 | drop unknown custom-field values on edits, and keep custom fields in step on import and patch | data hygiene | S |
| A5 | bad rundown input → 400, not 500 | correctness | S |
| A6 | reject invalid share URLs before generating | correctness | S |
| A7 | a matching refetch after every project-data write; broadcast all changed entries | other screens go stale otherwise | S–M |
| A8 | find flag and group end in **playable** order | to verify against `runtime.rs`; may already be fine | S |
| A9 | rundown title parsing fix (#2300) | importer | S |

## B. Client: pull as patches

The client stays TypeScript; a UI is not worth rewriting.

1. **B1 — the 97 clean patches.**
   - Land them on a branch.
   - Then `tsc`, a build, every e2e suite, and the browser checks against our Worker.
   - Some assume A1/A2. Those get their Rust side in the same step.
2. **B2 — websocket robustness. High value for the cloud.**
   - The patches:
     - detect silently dropped sockets;
     - recover when a close goes unreported;
     - back off watchdog reconnects;
     - match a Pong to its Ping.
   - All four conflicted, because `socket.ts` carries our room and session
     changes. Resolve them by hand.
3. **B3 — other conflicted fixes worth having:**
   - rundown list stops jumping on edits and clicks;
   - keyboard moves made faster than the server replies;
   - optimistic delete must not mutate the cache;
   - confirm before deleting the running event;
   - cuesheet render performance;
   - Safari clipboard for share links;
   - PWA insets.
4. **B4 — features to take or leave:**
   - export all rundowns to Excel;
   - sound when the timer ends (client only);
   - searchable settings;
   - secondary text in studio/timeline cards.

## C. Not applicable to a Worker backend (skip)

- Electron menu and update checks.
- OSC dispatch.
- MCP.
- "Back to Ontime Cloud dashboard".
- Persisting the node identity.
- The session log file.
- Restore points: a Durable Object persists every change already.
- Express compression and static gzip: Cloudflare does both.

## D. RIIR candidates

Move logic into `ontime-core` and share it with the browser through `ontime-wasm`.

1. **Teleprompter.**
   - Upstream modelled the script, the reading position and a *shared
     transport* in `packages/utils` and the TS server.
   - Shared transport across screens is a natural fit for the room's Durable
     Object.
   - The model fits the core plus wasm, like the timer engine.
   - It is the largest new upstream feature: 15+ conflicted patches.
2. **Gap and schedule calculations** that upstream just extracted to share
   between client and server. Our wasm can be that single shared copy.
3. **Countdown with skipped events.** It is pure logic.

## Recommended order

1. **A1–A9.** Small, test-first, Worker-only. Deploy them on their own.
2. **B1 + B2** on a branch `feat/upstream-4.15`. Full test pass, then deploy.
3. **B3, then B4,** picked per item.
4. **D1, the teleprompter, RIIR.** This one gets its own plan.

The throwaway worktree `/tmp/ontime-pull-trial` can be removed with
`git worktree remove /tmp/ontime-pull-trial`.
