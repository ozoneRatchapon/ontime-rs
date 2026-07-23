# Handover 007: Rust Backend Wired to the Durable Object

## 1. What was broken

The Worker looked alive but nothing it returned was real.

1. **REST never reached the Durable Object.** `apps/server-rs/src/lib.rs` answered every
   `/data/*` route itself with JSON literals. The CRUD implementation in `room.rs` existed
   but no route ever called it — dead code. Observable on the deployed Worker:

   ```
   POST /data/rundowns/default_rundown/entry {"title":"TEST"}
   → 200 {"title":"TEST","id":"019f8d00-…"}     looks like it worked
   GET  /data/rundowns/default_rundown
   → entries: entry_1, entry_2, entry_3          the new entry is gone
   ```

   Every edit returned 200 and was then discarded. React Query refetched and the UI snapped
   back to the same three placeholder events.

2. **The Durable Object never read its own storage.** `new()` assigned
   `RundownState::default()`; there were `storage().put(…)` calls but no `get`. Any state that
   did get written was lost the moment the DO was evicted.

3. **Invalid enum values.** The literals used `"timerType": "countDown"` and
   `"timeStrategy": "relative"`. The client's enums are `count-down` and `lock-duration`,
   so the editor could not parse an entry even when one arrived.

4. **Wrong data shapes.** `ProjectData` was sent as `{title, description, publicUrl}` — the
   client expects `{title, description, url, info, logo, custom}`. `Settings` was missing
   `language`. Events carried `subtitle`/`presenter`, which no longer exist on `OntimeEvent`,
   and `note: null` where the type says `string`.

5. **Flat event list, no timing.** State was a `Vec<EventItem>` with `delay`, `gap`,
   `dayOffset` and `timeStart` hardcoded to `0`, so every displayed time was wrong. Groups
   (`parent` / `order` / `flatOrder`) were not modelled at all.

## 2. What changed

### `packages/ontime-core/src/rundown.rs` (new)

The rundown document. Entries are stored as raw `serde_json::Value` rather than a typed
struct — the client owns the `OntimeEntry` shape and keeps evolving it, so storing payloads
verbatim keeps unknown fields intact across a round trip. The server only normalises the
fields it is responsible for.

Operations: `add_entry`, `edit_entry`, `batch_edit`, `delete_entries`, `delete_all`,
`reorder`, `swap`, `clone_entry`, `renumber`, `group_entries`, `ungroup`, `apply_delay`.

`recompute()` runs after every mutation and rebuilds:

- `flatOrder`, by expanding group children into the top-level order
- `timeStart` / `timeEnd` / `duration`, honouring `linkStart` and
  `timeStrategy` (`lock-end` derives duration from the end time, wrapping past midnight)
- `gap` — idle time before an unlinked event
- `dayOffset` — incremented when an event starts before the previous one ended
- `delay` — accumulated from `delay` entries earlier in the order
- group `timeStart` / `timeEnd` / `duration` / `isFirstLinked`

`RUNTIME_FIELDS` (`delay`, `dayOffset`, `gap`, `revision`) are stripped from incoming
patches so a client cannot overwrite a calculated value.

### `packages/ontime-core/src/project.rs` (new)

`Project` — the counterpart of the client's `DatabaseModel`: the rundown map, `loaded`,
`ProjectData`, `Settings`, `ViewSettings`, url presets, custom fields, automation, CSS
override. Defaults copied from `apps/server/src/models/dataModel.ts` so the shapes match
field for field.

### `apps/server-rs/src/room.rs` (rewritten)

The Durable Object is now the whole backend.

- `hydrate()` loads the project from storage once per instance. `DurableObject::new` is
  synchronous so this cannot live in the constructor — this is the fix for the eviction
  data loss.
- Persistence uses a **JSON string**, not a struct. The document nests maps and free-form
  `serde_json::Value`, which do not round trip cleanly through `serde_wasm_bindgen`'s JS
  representation.
- Every mutating handler ends in `commit()`, which persists and then broadcasts a `refetch`
  packet to connected sockets.
- WebSocket: replies to `ping` with `pong`, sends `client-init` on connect. The previous
  handler acted on `start`/`pause`/`load` tags — the client sends playback commands over
  the socket, but nothing in the project document responds to them, so those are now a
  no-op rather than mutating a phantom state.

### `apps/server-rs/src/lib.rs` (rewritten)

A thin proxy. `/ws`, `/data/*` and `/user/*` all forward to the room's Durable Object; the
only locally-answered routes are `/` and `/api/health`.

`with_cors` had to be rewritten: a response returned from a DO stub has **immutable
headers**, so `headers_mut().set(…)` threw `TypeError: Can't modify immutable headers` and
turned every proxied request into a 500. It now copies headers into a fresh `Headers` and
rebuilds the response, skipping websocket upgrades (status 101).

`apps/server-rs/src/types.rs` and `src/mod.rs` were deleted — fully superseded.

## 3. Verification

- `cargo test -p ontime-core` — 23 passing (19 new in `tests/rundown_tests.rs` covering
  defaults, partial patches, runtime-field protection, link timing, `lock-end`, gaps,
  delays, reorder, group/ungroup, clone, renumber, and a serialisation round trip).
- REST smoke test against `wrangler dev --local`: 40 checks, all passing — create/edit/
  delete round trip through storage, link timing, reorder, group/ungroup, clone, settings,
  project data, view settings, custom fields, url presets, rundown lifecycle.
- WebSocket test: `client-init` on connect, `ping`→`pong`, and a REST mutation broadcasting
  a `refetch` carrying `target`, `revision` and `rundownId`.
- **Persistence across restart**: killed and restarted `wrangler dev`; the rundown and the
  edited project title were still there. This was the failure mode before.
- Endpoint sweep: all 15 paths the client fetches on load return 200.

## 4. Known limits

Not ported — these answer `501` with a message rather than a bare 404, so the client shows
something meaningful:

| Module | Why |
|---|---|
| `/data/db/*` | project files need a filesystem |
| `/data/excel/*` | import/export needs a filesystem |
| `/data/sheets/*` | Google Sheets sync needs OAuth + a long-lived session |
| `POST /data/automation/*` | automations drive OSC/UDP, which Workers cannot open |
| `POST /data/rundowns/import` | depends on the spreadsheet parser |

Also still missing:

- **No runtime/playback.** There is no timer engine, so `runtime-data` is never broadcast.
  *(Resolved in handover 008 — playback, countdown and end actions now work. Roll mode,
  aux timers and the message service are still missing.)*
- **No report, no custom views.** Both return empty stubs.
- **One shared room.** Every visitor without `?room=` lands on `default_room`, so all users
  share one project. There is no auth.
- **2 MB storage ceiling.** The whole project is one Durable Object value; on the SQLite
  backend a key plus its value cannot exceed 2 MB. Large projects will need splitting
  per rundown. (Superseded note: an earlier draft said 128 KiB, which is the limit of
  the key-value backend this no longer uses — see handover 008.)
- **Timing is an approximation.** `recompute` implements linkStart, timeStrategy, gap,
  dayOffset and delay, but not the full behaviour of the original rundown cache
  (`countToEnd`, nested-group edge cases).

## 5. Running it

```bash
# backend
cd apps/server-rs && worker-build --release && npx wrangler dev --port 8787 --local

# client against it
cd apps/client && VITE_WORKER_URL=http://127.0.0.1:8787 pnpm dev --port 5173
```

Deployed at `https://ontime-server-rs.example.workers.dev` — see handover 008 for
the free-plan Durable Object migration that this deploy required.
