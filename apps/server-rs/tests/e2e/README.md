# End-to-end suites

Behavioural tests that drive the real HTTP and websocket surface `apps/client` uses.

They exist because unit tests could not have caught the bugs that actually shipped:
a 200 response with the wrong body, rooms leaking into each other, a timer that
stopped ticking, an endpoint named in the singular. Each suite here was written in
response to a real failure.

## Running

```bash
./run.sh                        # against a local wrangler dev on :8787
./run.sh --start                # start wrangler dev, run, stop it
./run.sh https://…workers.dev   # against a deployment
```

Exits non-zero and names the failing suite. Safe to run against production: every
suite creates its own rundown or room and cleans up after itself.

`engine.mjs` loads the real `.wasm` the browser loads, so `packages/ontime-wasm` has
to be built first — `run.sh` builds it if it is missing.

## The suites

| Suite | Covers |
|---|---|
| `rest.sh` | CRUD round trips: create/edit/delete actually persist, link timing, reorder, group/ungroup, clone, settings, custom fields, url presets, rundown lifecycle |
| `shapes.mjs` | the **type of every field the client destructures**, taken from `packages/types` |
| `ws.mjs` | websocket handshake, ping/pong, and that a REST mutation broadcasts a refetch |
| `timer.mjs` | playback driven over the websocket exactly as the client drives it |
| `engine.mjs` | the browser WASM engine computes the same numbers as the server |
| `rooms.mjs` | two rooms are fully isolated — data, settings and playback |
| `auth.mjs` | per-room password over REST and websocket |

`coverage.sh` is a diagnostic sweep, not a test — it prints the status code of every
endpoint the client can call. Useful when adding endpoints; it is deliberately not
in `run.sh` because it asserts nothing.

## Adding an endpoint

Add its contract to `shapes.mjs`. Status codes are not enough: three endpoints once
returned 200 with a body the client could not use, and the status-only sweep passed
them all. `shapes.mjs` asserts field types, which is what caught them.

## Notes

- Suites are ordered cheapest-first in `run.sh`, so an obvious break surfaces fast.
- `timer.mjs` asserts on **the packet a command caused**, not the most recent packet.
  With a fast heartbeat a background packet can land inside the wait window; reading
  "the latest" made the suite flaky.
- Tolerances in `engine.mjs` distinguish two things: same-instant agreement between
  server and browser (must be exact) and the gap a client accumulates between
  packets (network jitter, not drift).
