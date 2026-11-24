# 008 — D1: the teleprompter, in the Rust core

Status: **done and deployed (2026-11-24).**

- Fast-forwarded into `feat/rust-cloud-backend` (70b8b383).
- **Worker:** `c9560deb`. The rollback point is `02b203be`.
- **Client:** Pages `1ca3aa2c`, Production at `ontime-team.pages.dev`.
- **Production smoke test:** `teleprompter.mjs` 35/35 and `activity.mjs`
  25/25, both with throwaway rooms that were removed afterwards.
- **Existing rooms:** a read-only check returns 200 for all of them.
- **The `master.` alias** was briefly stuck on the previous client, because
  a Production deploy doesn't move a branch alias. It is current since the
  two-branch deploy (`81aa811c` and `89924a11`).
  Its 4 decisions use the recommended answers, so the user's standing approval
  applies.

## Progress

**T0 and T1, the contract and the core:**

- Done in commits de884be0, edc4f759, 91ecd415 and 45501194.
- `ontime-core/src/teleprompter/` holds `wrap`, `settings`, `script`,
  `position` and `transport`.
- **How the fixtures are made:** `scripts/teleprompter-oracle/generate.sh
[ref]` extracts upstream with `git archive`, bundles its TypeScript with
  esbuild, and runs it over our case lists. So every expected value is
  upstream's own output, not our reading of it.
- **What they cover:**
  - wrap: 62 cases, including Thai, emoji and no-break spaces;
  - script and settings: 48 cases;
  - position: 7 scripts swept at every quarter row, 12 edits, 13 reanchors;
  - transport: 20 scenarios, 119 steps;
  - the command parser: 44 payloads.
- **Result:** everything matches exactly, floats included. The tests parse
  floats with serde_json's `float_roundtrip`.
- Re-run `generate.sh` after each upstream pull. A changed fixture shows what
  upstream changed.

**Found along the way:** upstream cuts Thai without spaces in the middle of a
word (`ยิ|นดี`), because it breaks per grapheme. The contract copies that.
Breaking at real Thai words would need a dictionary (ICU), so it is a later
improvement and must be added to both sides.

**T2, the hosts:**

- **The core facade, `teleprompter/room.rs`:** builds the script, the first
  read follows the loaded event, resolves goto targets (cue, id, index,
  loaded), keeps the reader in place after a change, settles on the alarm,
  gives the wake time, pauses on restore, and produces the runtime fields.
- **Routes, `teleprompter/routes.rs`:** `GET` and `POST
/data/teleprompter/settings` (422 on invalid input), and
  `GET /data/teleprompter/script?source=`.
- **Hooks in `api`:**
  - the script from before a change, then `after_change`;
  - the `teleprompter` socket tag;
  - following the loaded event after any playback command;
  - `on_alarm`, which is the timer tick plus the prompter settling;
  - `next_wake_in`, now the earlier of the two;
  - `runtime_payload`, shared by both hosts, which removed their duplicated
    payload code.
- **Custom fields:** renaming one moves the script source with it, and
  deleting one falls back to `note`.
- **Host fix:** both hosts now pass the query string to the core. The Worker
  passed only `req.path()`, and native has `RawQuery`.
- **Tests:**
  - core `teleprompter_room_tests` (9);
  - e2e `teleprompter.mjs` (35);
  - every other e2e suite is still green on both Workers;
  - native activity 9/9, and native serves `?source=`.

**T4, the client from upstream:**

- **14 upstream commits cherry-picked with `-x`.** `5757ebed0` was skipped:
  it only adapts upstream's Playwright spec to a router prefix.
- **What was left out:** every `apps/server` and `docs` part. Our vendored TS
  server is older, isn't deployed, and the Rust core replaces it.
  `git diff 10f27353 -- apps/server docs` is empty.
- **Three client conflicts, kept both sides:**
  - `getAppUrl` builds on `clientURL` and keeps the room.
  - The settings menu has no upstream search keywords; that is another
    feature.
  - The preset route has no `ViewLoader` wrapper.
- **Parity check:** every teleprompter file in `packages` and `apps/client` is
  byte-identical to `upstream/master`.
- **The TS side of the contract:**
  `packages/utils/src/teleprompter/teleprompterContract.test.ts` replays the
  same position and transport fixtures as the Rust tests.
- **Tests:**
  - utils 609 (upstream's teleprompter tests plus the contract);
  - client 510;
  - typecheck shows only the 3 known errors;
  - the build works.

**T5, the browser:**

- `apps/server-rs/tests/browser/teleprompter.mjs` passes 8/8 against a local
  Worker and a built client.
- A controller and a remote screen scroll together and read the same line.
  Pausing stops both, a late screen lands on the same line, an edit reaches
  the screens, and the console has no errors.
- **A bug the browser found, now fixed and covered by a core test:**
  upstream's first build re-anchors from an empty script, which puts a reader
  with no place at the top. `mark_built` didn't do this, so in a room with no
  event loaded the first play paused at once.

**Differences from upstream, all deliberate:**

- **No 304 or ETag on the script:** the script carries its revision, which is
  what screens compare.
- **A body that isn't an object (`[]`)** reads as an empty patch (200, no
  change), not 422. Every route's hosts treat such bodies as `{}`.
- **Socket commands get no `{tag:'teleprompter'}` reply.** Screens read
  `runtime-data`, and the reply matters only to OSC, which the Worker doesn't
  have.

**For later:**

- **Script cache:** done on 2026-11-24.
  - It is keyed on a blake3 fingerprint of exactly what the script reads, so
    it stays correct even for a write that skips `recompute`.
  - It is held on `Project` but never serialized, so a restore starts fresh.
  - A build takes 1.03 ms and a cache hit 89 µs, for 100 events. See
    `benchmarks/001_teleprompter_script_cache.md`.
- **Teleprompter commands in the audit log:** done on 2026-11-24, with their
  own area and actions. Quick repeated taps merge into one line.
- **`api.rs` split by responsibility:** done on 2026-11-24 into `api/`. The
  largest file is 304 lines, and the public paths are unchanged.

## T2 design notes (written before building it)

- **Settings and transport:** settings go in `Project.teleprompter`, the same
  key upstream uses, so project files stay compatible. The transport goes in
  `Project.teleprompter_transport`. Both have serde defaults. A restore pauses
  the transport.
- **The previous script for reanchoring:** the host builds the script before
  and after any change that can affect it (rundown, custom fields, settings,
  load, restore), then calls `reanchor_transport`.
  - This needs no stored copy of the old script.
  - It is skipped while the transport has never been used (anchor is `None`).
- **Script cache:** in memory, keyed by (loaded rundown id, rundown
  revision, settings, custom fields), so `runtime-data` can carry
  `teleprompter` and `teleprompterSync` without rewrapping.
- **Clock:** `now` is the room's time of day,
  `(epoch + timezone_offset) mod day`. This is the same clock that Pong
  carries and the client's `serverClock` follows.
- **Waking:** `transport::ms_until_settled` feeds `next_wake_in`, so the alarm
  fires once, at the boundary.
- **Shape:** a small core facade (`teleprompter/room.rs`) does goto target
  resolution (cue, id, index, loaded), following the loaded event, the
  runtime patch and the wake time. Each host only calls it.

## What upstream built

Source: `upstream/master`, read 2026-11-24.

**A shared prompter.** Screens in a room scroll one script together:

- the `controller` role drives it;
- `remote` screens follow;
- a `local` screen runs on its own.

**The script comes from the rundown.** Each event contributes its note, its
title or a text custom field. The text is wrapped into lines of
`charsPerLine` characters, under a heading of title and/or cue.

**The server is authoritative.** It holds a transport:

- `playback`, `mode` (`event` or `script`), `speed` in lines per minute
  (negative reads back), `anchor`, `at`, `ended` and `blackout`;
- the anchor is a place in the text (event id, character offset, fractional
  lines), not a row, so edits and rewraps don't make the reader jump.

**What clients receive.** Clients get `teleprompterSync { revision, playback,
speed, row, until, at }`. Each follower computes
`row + speed × elapsed / 60000`, clamped at `until`, against the server
clock. There is no per-tick position broadcast: sync changes only on commands
and at boundaries.

**Where upstream keeps things:**

- Settings `{ script, charsPerLine, heading }` are stored in the project.
- The transport is in memory only.
- The pure logic is in `packages/utils/src/teleprompter/`:
  - transport;
  - position and layout;
  - settings;
- and in the server's service folder: `wrap` and `utils`.

**Size.** It is the largest upstream feature: about 20 client files, a
service, routes, a websocket command, and 15+ patches that conflict with us.

## Approach: the same split as the timer engine

Pure logic goes into `ontime-core`. The Durable Object holds the shared
transport. The client view comes from upstream almost unchanged.

### T0. A contract first (same pattern as D2 step 1)

Port upstream's test tables to a fixture,
`packages/ontime-core/tests/fixtures/teleprompter.json`, and run it from both
sides:

- `cargo test`;
- vitest against upstream's TS functions, once they are pulled in T4.

**What the fixture covers:**

- **wrap:** graphemes, break points, NBSP, punctuation and number gluing, long
  words, indent, tab and CRLF, offsets;
- **position:** layout rows, the anchor and row round trip, edits and
  rewraps, deleted events;
- **transport:** the command tables, settle and bounds, midnight wrap, clock
  lag, speed clamp and step, `describeSync`;
- **settings parsing:** clamping, rejections and source renames.

**Done when:** the Rust core passes every case, and the TS side does too
once T4 lands.

### T1. Core modules in `ontime-core/src/teleprompter/`

Each module is a few hundred lines; `mod.rs` is an index only.

- **`wrap.rs`**
  - It needs grapheme widths (`unicode-segmentation`) and the Unicode
    categories Pe, Pf, Ps, Pi and N (a small table or
    `unicode-general-category`).
  - **Offsets must count UTF-16 code units,** as upstream's do, or the wire
    format changes.
- **`script.rs`:** `build_script(rundown, custom_fields, settings)`, which
  walks `flat_order` and skips `skip` events and events whose text wraps to
  no lines. The output is headings, then text lines, then a blank line.
- **`position.rs`:** layout, `row_of_anchor`, `anchor_at_row`, `reanchor`,
  `map_offset_through_edit`.
- **`transport.rs`:** `apply_command`, `settle`, `position_at`,
  `playback_bound`, `describe`, `describe_sync`, the speed clamp and step,
  and the midnight-aware elapsed time.
- **`settings.rs`:** parsing and validation (422 cases) and
  `rename_script_source`.
- **The command parser** for the `{tag:'teleprompter'}` payload: strings, goto,
  speed, speedBy, scroll, mode, blackout.

### T2. The host, in the room's Durable Object

- **Settings:** `Project.teleprompter`, with a serde default.
  - It is cleared or migrated when a custom field is renamed or deleted, using
    the same hook custom fields already have.
- **Routes:**
  - `GET` and `POST /data/teleprompter/settings` (422 on invalid input);
  - `GET /data/teleprompter/script?source=` with an ETag, answering 304 when
    nothing changed.
- **Websocket:** tag `teleprompter`. The reply is `TeleprompterState`, and
  `runtime-data` carries both `teleprompter` and `teleprompterSync`.
  - It obeys members-only like other playback commands, and is recorded in
    the audit log as a playback action.
- **Following:** in event mode the transport follows the loaded event, so
  loading an event moves the prompter there.
- **No per-second tick.** The room's alarm also wakes at the moment playback
  reaches its bound (`ms_to_bound`), settles, then broadcasts once. This joins
  `next_wake_in`, so an idle room still costs nothing.

### T3. The wasm, later

Followers need only `travel()` (a one-line formula), so the browser keeps
upstream's TS for now. T0 keeps the two copies honest, as in D2.

Moving the `local` role's transport into wasm is optional. Revisit it with
D2 step 2.

### T4. The client, from upstream

- Cherry-pick upstream's teleprompter client patches and the
  `packages/utils/src/teleprompter` and `packages/types` parts, then resolve
  the 15+ conflicts. Follow `docs/tracking-upstream.md`.
- Point its API calls at our routes. The paths are the same.
- Wire the T0 fixture into vitest.

### T5. Tests and rollout

- **e2e `teleprompter.mjs`:** settings round trip and 422, the script with
  its ETag and 304, a controller command reaching a follower socket, an end
  boundary broadcast with no tick spam, and an edit during playback.
- **Browser check:** one controller and one remote screen scroll together,
  and the local role works on its own.
- **Rollout:** record the Worker version, deploy the Worker then the client,
  and smoke test with a throwaway room.

## Decisions (recommended answers, used unless the user says otherwise)

1. **The transport is persisted with the project.** Upstream keeps it in
   memory, but a Durable Object is evicted between messages and would forget
   the reading position.
2. **The script revision is a content hash**, the first 48 bits of blake3,
   which is safe as a JS number, not an incrementing counter. It needs no
   stored state, and the same lines always give the same revision. Clients
   only compare it for equality.
3. **A boundary is settled by the room's alarm,** not a once-a-second tick,
   in line with how the timer already avoids ticking.
4. **The browser keeps upstream's TS math,** guarded by the T0 fixture. Wasm
   comes later, if ever.

## Out of scope for now

- The integration HTTP API `/api/teleprompter/...` and OSC. The Worker has no
  OSC, and the HTTP integration API is not ported yet.
- Moving the `local` role's math into wasm (T3).

## Risks

- **Unicode.** Grapheme and category tables must match the browser's
  `Intl.Segmenter`. Any difference shows up as a fixture failure, not on
  stage.
- **Upstream churn.** The teleprompter was still changing in November 2026.
  Pull it as late as practical, and let the fixture show what moved.
- **Patch conflicts in T4.** This is the biggest effort. Do it on
  `feature/teleprompter`, rebased onto the default branch.

## Size

- T0 and T1: about 3 days.
- T2: about 2 days.
- T4: about 2 to 3 days.
- T5: about 1 day.
