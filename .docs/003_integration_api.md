# 003 — The integration API: driving a room from outside

Plan: `.plans/009_integration_api.md`. Code: `packages/ontime-core/src/integration/`
(core) and `apps/server-rs/src/room_integration.rs` (Worker host).

## Using it

Every action is an HTTP `GET`, so anything that can open a URL can drive a room:
Bitfocus Companion's generic HTTP module, a Stream Deck, a show-control system,
`curl` or a browser bookmark.

```
https://<worker>/r/<room>/api/<action>/<path...>?<query>
```

A protected room needs its token: `Authorization: Bearer <token>`, or
`?token=<token>` when the sender cannot set headers. A browser cookie does not
count (see Security). Successful calls answer `202 {"payload": ...}`. A refused
call answers `400 {"message": "..."}` with upstream's wording.

| What                        | Call                                                                   |
| --------------------------- | ---------------------------------------------------------------------- |
| Is it there?                | `/api` → `{"message":"You have reached Ontime API server"}`            |
| Start / pause / stop        | `/api/start`, `/api/pause`, `/api/stop`, `/api/reload`, `/api/roll`    |
| Start the 3rd event         | `/api/start/index/3` (counts from 1, skipped events included)          |
| Start by cue or id          | `/api/start/cue/4A`, `/api/start/id/<id>`                              |
| Next / previous             | `/api/start/next`, `/api/load/previous` (`load` arms without playing)  |
| Add or remove time          | `/api/addtime/add/60000`, `/api/addtime/remove/30000` (ms, ≤ 1 hour)   |
| Message overlay             | `/api/message/timer/text/Wrap%20up`, `/api/message/timer/visible/true` |
| Aux timers 1–3              | `/api/auxtimer/1/duration/90000`, `/api/auxtimer/1/start`              |
| Edit an entry               | `/api/change/<id>/title/New`, `/api/change/<id>/custom:lighting/blue`  |
| Rename or redirect a screen | `/api/client?target=<clientId>&rename=Stage`, `...&redirect=%2Ftimer`  |
| Offset mode                 | `/api/offsetmode/relative`                                             |
| Teleprompter                | `/api/teleprompter/play`, `/api/teleprompter/speed/120`                |
| Everything at once          | `/api/poll` (the same `runtime-data` the screens get)                  |

```bash
room=my-show; token=...   # the room's token, from claiming its password
curl -H "Authorization: Bearer $token" "https://<worker>/r/$room/api/start/index/1"
curl "https://<worker>/r/$room/api/addtime/add/60000?token=$token"
```

A websocket client can send the same actions as `{"tag": "start", "payload":
{"index": 3}}` and gets `{"tag": "start", "payload": "success"}` back.

### Things that behave as upstream, which can surprise

- Path segments are used as they arrive, still percent-encoded, as Express
  hands them over. Only the overlay texts are decoded: `message/.../Wrap%20up`
  shows "Wrap up", but `change/<id>/title/My%20Title` stores `My%20Title`. To
  send a text with spaces to `change`, put it in the query, which is decoded:
  `/api/change/<id>?title=My+Title`.
- A `#` cannot travel in a path. `/api/change/<id>/colour/%23FFAA00` arrives as
  `%23FFAA00` and is refused as a colour name. Use a CSS colour name, or the
  websocket.
- `identify` takes a boolean, which a query string cannot carry. Use the
  websocket for it.
- Bitfocus Companion's own Ontime module connects to a LAN server's websocket by
  host and port. It has no way to name a room or send a token, so with this
  cloud backend use Companion's generic HTTP module and the URLs above.

## How the parts fit

```
HTTP  GET /r/<room>/api/start/index/3
        │ Worker: /r/<room> picks the room's Durable Object (lib.rs)
        ▼
      Durable Object (room_integration.rs)
        │ token only, room exists, members-only → may drive?
        │ http_call(path, query) → ("start", {"index": "3"})
WS    {"tag":"start","payload":{"index":3}} ──┐
        ▼                                     ▼
      ontime_core::integration::parse(tag, payload) → Action   (one parser)
        ▼
      dispatch(project, action, now) → Reply { payload, change }
        ▼
      host carries out `change`:
        Runtime → store, audit, send runtime-data, set the alarm
        Rundown → store, refetch, runtime-data
        Client  → rename / redirect / identify one socket
```

The rules this follows, and why:

- **One parser for every transport.** Upstream sends HTTP, the websocket and OSC
  through `dispatchFromAdapter`. We do the same, so the HTTP API and the
  websocket cannot drift apart. Before this, our websocket had its own smaller
  parser and could not start an event by index or cue.
- **Typed from the edge.** `parse` turns JSON into an `Action` enum once.
  `Runtime` takes a `Command`, the overlay a `MessagePatch`, an aux timer an
  `AuxCommand`. Nothing after the parser looks at JSON again, so a payload shape
  that is wrong fails in one place with one message.
- **The core decides, the host acts.** `dispatch` changes only the project and
  returns a `Change`. Storage, sockets and alarms are the host's. This is how
  the same core runs in the Worker and in `server-native`, and how a future OSC
  host (UDP, which a Worker cannot receive) would plug in.
- **JavaScript semantics where requests depend on them.** Upstream reads strings
  from URLs with `Number()`, `String()` and `decodeURI()`. `integration/js.rs`
  follows the ECMAScript rules, so `0x10`, `" 2 "` and `inf` mean here what they
  mean there. Rust's own `"inf".parse::<f64>()` is infinity, where JavaScript
  gives `NaN`.

## How it is tested

Three layers, each catching something the others cannot:

1. **Contracts against upstream's own code**, in `scripts/upstream-oracle/`.
   - `gen-integration.ts` runs upstream's real Express router and dispatcher,
     with its services replaced by recorders (`integration-stubs/`). It records
     the service call each of 154 requests turns into, or the error it is
     refused with.
   - `gen-selection.ts` runs upstream's `runtime.utils` and `roll.utils` over
     eight rundowns: which event next, previous, an index and a cue pick.
   - `integration_contract_tests.rs` and `selection_tests.rs` compare ours.
     Differences we chose are listed in `DELIBERATE` with the reason. A test
     fails if upstream later agrees with us, so the list cannot go stale.
2. **Unit tests** in `packages/ontime-core/tests/`, for the runtime behaviour
   behind the calls.
3. **End to end**: `apps/server-rs/tests/e2e/integration.mjs` against
   `wrangler dev`. A real room, real sockets, rename and redirect, and token
   versus cookie.

Regenerate the fixtures after pulling upstream:
`scripts/upstream-oracle/generate.sh`, then `cargo test -p ontime-core`.

## Security

- **Token, never cookie.** These GETs change state. If the browser's cookie
  counted, any web page an operator opened could press Start for them with
  `<img src="https://<worker>/r/<room>/api/start">` (cross-site request
  forgery). The Durable Object reads only the `Authorization` header and
  `?token=`, and decides access before it looks up any session.
- **`?token=` is a fallback.** A URL can end up in logs and browser history, so
  prefer the header where the sender supports it.
- **Members-only rooms** are driven only by a member, the room password or the
  master password. Anyone let in may still read `version` and `poll`.
- **No room is created by a call.** An unknown room answers 404.
- **Bounds**: `addtime` is at most an hour, times are within a day, and index
  and cue lookups are bounded by the rundown.

## What this fixed on the way

- The websocket accepts everything upstream's does: start or load by index or
  cue, `start/previous`, and `addtime {add|remove}`.
- `load/next` after the last event wraps to the first, and `load/previous` at
  the first loads it again, as upstream. Before, both did nothing.
- A skipped event cannot be loaded by id, as upstream.
- The editor's Network panel can rename, redirect and identify screens. Before,
  the Worker ignored the `client` tag, so those buttons did nothing.
- `runtime.rs` was split (`command.rs`, `selection.rs`, `roll.rs`) to stay
  under 1024 lines.
