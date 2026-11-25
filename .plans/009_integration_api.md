# 009 — The integration API, in the Rust core (RIIR 5)

Status: **done and deployed (2026-11-25), Worker `67341f2f`.** Asked for by the user:
"super goat", reusable in other projects, best performance and features, and
something to learn from. How to use it and how it fits together:
`.docs/003_integration_api.md`.

## Result

- Steps 1–6 done. ontime-core: 354 tests pass, clippy clean.
- Contract: 154 requests through upstream's own router and dispatcher, and
  eight rundowns through its selection functions. All agree, except two
  deliberate differences: `?token=` is the host's, and `addtime` counts
  whole milliseconds.
- e2e: `integration.mjs` passes all 48 checks, and every other suite still
  passes, on both hosts: a fresh `wrangler dev` and the native server (as CI
  runs them). The first push missed the native host; CI caught it.

## Follow-ups (not done, found on the way)

- `start` while rolling: upstream keeps the event's start time and switches
  to play. Ours restarts the event's timer from now. Not covered by any
  contract yet.
- `start` with nothing loaded: ours starts the first event, while upstream
  refuses ("Unable to start"). Kept for now. Check whether anything relies on
  it before matching upstream.
- An integration `change` edits the entry but is not written to the room's
  activity log. Upstream does not log it either.

## Goal

Anything that can send an HTTP GET or a websocket packet can drive a room:
Bitfocus Companion, a Stream Deck, a show-control script, `curl`. Upstream has
this in `apps/server/src/api-integration/`. The Worker answers only
`/api/health` today.

## The upstream shape we keep

Upstream sends HTTP, the websocket and OSC into **one dispatcher**,
`dispatchFromAdapter(tag, payload)`. A transport only turns its own input into
`(tag, payload)`:

- HTTP `GET /api/start/index/3` → `("start", {index: "3"})`
  (`integrationPayloadFromPath`); `?a=b` becomes the payload when the path has
  no more segments, or the value at the end of the path when it has.
- websocket `{"tag":"start","payload":{"index":3}}` → the same.

So there is one parser, one set of rules and one set of error messages. We do
the same in Rust, and fix the one thing our websocket does differently today:
it has its own smaller parser (`Command::parse`), so `start` by index or cue,
`start/previous`, `load` by index or cue and `addtime {add|remove}` only work
over HTTP upstream and not at all here.

## Design

Ports and adapters, as the rest of the core:

```
transport (host)        core: ontime_core::integration         state
HTTP GET /api/...  ─┐   path.rs    payload_from_path            Project
WS {tag,payload}   ─┼─► action.rs  Action::parse(tag, payload)  ─► dispatch.rs
(future: OSC/UDP)  ─┘   errors.rs  IntegrationError (upstream's messages)
                                   dispatch() -> Reply { payload, effects }
```

- `Action` is a typed enum, parsed once. Nothing past the parser looks at JSON
  shapes again. `Command` (playback) gains `StartIndex`, `StartCue`,
  `StartPrevious`, `LoadIndex`, `LoadCue`, so playback stays in `Runtime`.
- `dispatch` returns what changed as effects the host carries out, the way
  `api::handle` already does: persist, broadcast runtime, refetch, a message
  to a client socket (rename, redirect, identify).
- The websocket goes through the same `Action::parse`, so both transports
  accept exactly the same payloads.

## Security

- The HTTP integration API changes state on a GET, so it accepts only a
  **token** (Bearer header or `?token=`), never the browser's session or
  token cookie. Otherwise any web page could make a signed-in operator's
  browser press Start (CSRF with an `<img src>`). Upstream runs on a LAN and
  does not need this; a public Worker does.
- Members-only rooms: only a privileged caller drives, as on the websocket.
- A room that does not exist is not created by an integration call.
- `addtime` keeps upstream's one-hour bound; index and cue lookups are bounded
  by the rundown.

## Contract with upstream

`gen-integration.ts` bundles upstream master's own `integration.controller.ts`
with its services replaced by recorders (an esbuild plugin), and runs a list of
cases through `integrationPayloadFromPath` and `dispatchFromAdapter`. Each case
records which service call it made (`startByIndex(2)`, `addTime(-60000)`,
`messageService.patch({...})`) or the error it threw. The cargo test maps our
`Action` to the same call description and compares. A change on either side
fails the test.

## Steps

1. Move `Command` out of `runtime.rs` (1151 lines, over the 1024 rule) into
   `command.rs`.
2. Core: `integration/{path,action,dispatch,errors}.rs`, with the index, cue
   and previous commands in `Runtime`.
3. Oracle: `gen-integration.ts`, fixture, cargo contract test.
4. Host: `/r/<room>/api/...` in the Worker and the Durable Object; the
   websocket goes through `integration`.
5. e2e: `apps/server-rs/tests/e2e/integration.mjs` against `wrangler dev`.
6. Docs: `.docs/003_integration_api.md`, how to call it with `curl` and
   Companion, and how the parts fit (the learning part).

## Out of scope

- OSC: UDP, which a Worker cannot receive. The dispatcher is transport-free, so
  a small native host (`server-native`) could add it later.
- Automations (outgoing OSC/HTTP triggers): stored, not acted on, as today.
