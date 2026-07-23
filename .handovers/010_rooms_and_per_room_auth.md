# Handover 010: Multiple Projects (Rooms) and Per-Room Auth

Each project now lives in its own isolated room with its own optional password.

## 1. A correction to handover 009 §3

Handover 008 and my earlier notes claimed Ontime "has no real auth, only a
client-side lock". That was wrong, and it changed the plan once discovered.

`editorKey` / `operatorKey` really are client-side view locks — the server only
stores and obfuscates them. But there is a **separate, server-enforced** auth layer
that I had missed:

```ts
// apps/server/src/app.ts
app.use(`${prefix}/data`, authenticate, appRouter);
```

`apps/server/src/middleware/authenticate.ts` gates `/data/*` and the websocket on a
token derived from the `SESSION_PASSWORD` env var, accepted via a `token` cookie or
`?token=`. When the variable is unset the middleware becomes a no-op — which is why
a LAN install feels open.

So the model below is a port of something that already exists upstream, not an
invention, and the client already understands the token flow.

## 2. Rooms

A room is one Durable Object instance, and therefore one isolated project: its own
rundowns, settings, playback and password.

### Addressing

Two mechanisms, because the client has two very different call styles:

| Transport | How the room travels | Why |
|---|---|---|
| REST | `/r/<room>/data/…` path prefix | the client composes paths by appending to `serverURL`; a query string would land in the middle of the composed path |
| WebSocket | `?room=<room>` | a single URL with no path to spare |

`lib.rs::route()` resolves the room in that order and **strips the prefix before
forwarding**, so every room's Durable Object sees identical paths and never has to
know how it was addressed. Room names are `[A-Za-z0-9_-]{1,64}`; anything else falls
back to the shared default room rather than creating junk namespaces.

The Durable Object needed no change for this — its path handling already searched
for `/data/` anywhere in the path.

`forward()` only rebuilds the request when there is actually a prefix to strip.
The websocket upgrade is passed through untouched, because a synthetically rebuilt
request is not reliably recognised as an upgrade.

### Client

`externals.ts` previously read `?room=` for the websocket only, so **every REST call
still went to the default room** — rooms looked implemented but were not. Now
`serverURL` carries `/r/<room>` when a room is present, which fixes every REST call
at once because they are all composed from it.

Open a room with `?room=<name>`:

```
https://ontime-client.example.pages.dev/editor?room=proj-alpha
```

## 3. Per-room auth

- Each room stores `password_hash: Option<String>`. `None` means open — matching
  upstream with `SESSION_PASSWORD` unset, so this could not lock anyone out of the
  existing deployment.
- The hash *is* the bearer token, exactly as upstream uses `hashPassword(password)`
  as its token. blake3, domain-separated with an `ontime-room-v1:` prefix.
- Comparison is constant-time, so a token cannot be recovered by timing responses.
- `SESSION_PASSWORD`, if set as a Cloudflare secret, is a master key. Without one a
  forgotten room password would make the room permanently unreachable.

### Claiming a room

While a room is open, anyone may set its password — that is how a new room is
claimed. Afterwards the endpoint needs the current token like everything else.

```bash
# claim
curl -X POST $WORKER/r/proj-alpha/data/session/password \
     -H 'content-type: application/json' -d '{"password":"show-secret"}'
# -> {"protected":true,"token":"<hash>"}

# reopen
curl -X POST $WORKER/r/proj-alpha/data/session/password \
     -H "Authorization: Bearer <hash>" \
     -H 'content-type: application/json' -d '{"password":null}'
```

The token is returned **once**, at claim time; it cannot be recovered from the hash.

### Presenting the token

Accepted from three places, so every client shape works:

| Form | Used by |
|---|---|
| `Authorization: Bearer <token>` | REST, via an axios interceptor |
| `?token=<token>` | the websocket, which cannot set headers |
| `token` cookie | compatibility with a browser already logged in to `apps/server` |

The client reads `?token=` once and keeps it in `sessionStorage` keyed by room, so
it does not have to stay in the address bar where screen shares and history would
leak it. The interceptor only attaches it to our own `serverURL`, never to the
third-party endpoints the app also calls.

```
https://ontime-client.example.pages.dev/editor?room=proj-alpha&token=<hash>
```

## 4. Verification

`scratchpad/rooms-test.mjs` — 24 checks, all passing on production:

- each room boots its own project
- an entry added in one room is absent from the other
- settings, timezone and project data are per room
- two websockets: starting playback in one leaves the other stopped, and they run
  different events
- the default room is untouched by either
- invalid room names fall back instead of creating namespaces

`scratchpad/auth-test.mjs` — 20 checks, all passing on production:

- a fresh room is open; claiming it returns a token
- every read *and* write is 401 without a token, and with a wrong token
- the token works as a header, a query param and a cookie
- the room cannot be re-claimed without the token
- the websocket is refused without a token and connects with one
- neighbouring rooms and the default room are unaffected
- clearing the password reopens the room

Full production suite: `cargo test` 51, REST 40, WebSocket 6, timer 20, engine 18,
rooms 24, auth 20.

## 5. An unresolved flake

`timer-test.mjs` has now failed exactly once immediately after each of three
deploys, and passed on every rerun (6 consecutive after the last one). I never
captured which assertion failed, so **the cause is still unproven** — the pattern is
consistent with the Durable Object restarting as the new version rolls out, but that
is a hypothesis.

The harness now dumps packet count and the last timer payload on any failure, so the
next occurrence will be diagnosable instead of guessed at.

## 6. Response shapes, and why status codes were not enough

Opening the cuesheet crashed with `can't access property "length", r is undefined`
at `useProjectList.ts`. `GET /data/db/all` was returning the whole project document
because the name reads like "everything" — it is actually the project **file
browser** listing, `{ files: ProjectFile[], lastLoadedProject: string }`.

The endpoint sweep had been passing all along: it only asserted status codes, and a
200 with the wrong body looks identical to a correct one. Auditing every response
the client destructures turned up two more of the same mistake, one of which had not
been reached yet:

| Endpoint | Client expects | Was returning |
|---|---|---|
| `/data/db/all` | `{files, lastLoadedProject}` | the whole `Project` |
| `/data/settings/serverport` | `PortInfo` `{port, pendingRestart}` | a bare `443` |
| `/data/session/info` | `GetInfo` `{networkInterfaces, version, serverPort, publicDir}` | two fields missing |

`/data/session/url` now answers 501: share links are built from the client's own
origin, which a Worker serving no HTML cannot know.

`scratchpad/shapes-test.mjs` now asserts the **type of every field the client
reads**, taken from `packages/types`, for all thirteen endpoints. This is the check
that would have caught all three.

## 7. Still open

- Roll mode, aux timers, message service, report, custom views
- The rundown-wide offset calculation (the shipped one is a single-event approximation)
- Nothing enforces *which* rooms exist: any name creates a room on first use. If
  rooms should be provisioned rather than self-service, that needs a registry and
  the master key gating creation.
