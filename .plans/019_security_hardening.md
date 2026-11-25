# 019 — Security hardening (review of 2026-11-25)

Status: **items 1, 3, 5 and 4 done (not yet deployed).** Order agreed: 1 → 3 → 5 → 4 → 7.
Item 2 waits for the owner's go-ahead, because existing rooms need a migration.

## 1. Wrong guesses wait — done

A room token is the password's blake3 digest, so likely passwords can be hashed
offline and tried online at three doors: `/admin/login`, a Bearer token at
`/admin/*`, and a token at any room (REST, websocket, integration API). Before
this, none of them limited anything.

- Policy: `ontime_core::guard` (11 tests in `tests/guard_tests.rs`). Per client
  (`CF-Connecting-IP`; IPv6 keyed by its /64): 10 free failures, then a wait
  of 1 s that doubles per failure up to 15 min (about 100 guesses a day).
  Forgotten after an hour without failures. A success does not reset the
  count, so a person on the same network cannot hand a guesser a fresh
  allowance. While locked, **every** request carrying a token is refused with
  `429` + `Retry-After`, the right one too, or the lock would hide nothing.
  At most 4,096 clients tracked; the quietest go first.
- A room (`room.rs::fetch`) checks before handling and counts after: a `401`
  or `403` whose token is neither the room's nor the master's
  (`api::is_known_token`). A stale token in an open room is let in, so it is
  never counted; guessing the master token at an open room's admin-only
  routes is refused, so it is.
- The admin routes run in the stateless Worker, so they ask the registry
  instance (`POST /registry/guard`, `guard_host.rs`). One extra registry call
  per admin request; admin traffic is small.
- `tests/e2e/guard.mjs` (needs ADMIN_PASSWORD, local only, as it fakes client
  addresses): all three doors, the right answer refused while locked,
  recovery after the wait, clients counted apart, right answers never
  counted, an open room's stale token not counted. The full `run.sh` still
  passes, as do the five admin suites.

Not covered, by choice:

- **Many addresses** (a botnet) each get their own allowance. A count per
  room across all clients would let anyone lock a room's real users out
  with junk tokens, which is worse.
- **The native server** (`apps/server-native`) is a LAN install and does not
  use the guard yet. The policy is in the core, so it can be added when the
  native server faces the internet.
- **Eviction** hands out a fresh allowance, but a guesser keeps the instance
  alive with its own requests.

## 2. argon2 for passwords, a random token apart from the hash — waiting

Needs the owner's go-ahead: existing rooms (raknong among them) must keep
their passwords and share links working through a migration.

## 3. Security headers on Pages — done

`apps/client/public/_headers`, copied into the build by Vite:

- Every page: `X-Content-Type-Options: nosniff`, `Referrer-Policy:
  no-referrer` (share links carry `?token=`), a `Permissions-Policy` turning
  off camera, microphone, location, payment and USB.
- Nothing may frame a page (`X-Frame-Options: DENY`, CSP `frame-ancestors
  'none'`, plus `object-src 'none'; base-uri 'self'`), except the read-only
  views people embed on purpose: `/timer`, `/countdown`, `/backstage`,
  `/studio`, `/timeline`, `/info`, `/teleprompter`. An allow-list, because an
  unknown path redirects inside the app and could otherwise frame the editor.
- No `script-src`: `index.html` has an inline script for old browsers and the
  editor injects the operator's custom CSS. Nothing renders a person's HTML.
- Checked on a local `wrangler pages dev build`: each path gets the expected
  headers, and the wasm, scripts, service worker and manifest are served with
  the right types, so `nosniff` blocks none of them.
- Known gap: a framed viewer can navigate inside the app to a control page.
  Browsers partition a framed page's storage, so it holds no room token or
  session there, and an open room can be edited without framing at all.

## 5. `cargo deny` in CI — done

- `deny.toml` at the root, `.github/workflows/audit_rust.yml` runs it for each
  of the four crates (they are not one workspace): on push and pull request
  when a manifest or lockfile changes, and every Monday, because a new
  advisory arrives without any change here.
- Checks: vulnerabilities and yanked crates fail; licences must be on the
  allow-list (all GPL-3.0-compatible); crates only from crates.io; no
  wildcard versions. Our four crates now say `license = "GPL-3.0-or-later"`
  (as `LICENSE.md`) and `publish = false`.
- What the first run found, and what was done:
  - **`url` 2.5.8 in the Worker** (RUSTSEC-2026-0592, a stack overflow in
    `Url::origin()` on nested `blob:` URLs): updated to 2.5.9.
  - **`tokio` 1.53.1** (RUSTSEC-2026-0386, unsound `AsyncFd` constructors,
    not used by us): updated to 1.54.1 in all three lockfiles.
  - **`chrono` is unmaintained** (RUSTSEC-2026-0543, no fix exists):
    ignored with a reason. See item 9.

## 4. Weekly upstream check — done

`.github/workflows/upstream_watch.yml`, Mondays and by hand: fetches upstream
master, runs `scripts/upstream-oracle/generate.sh` over it and then the
ontime-core tests. The tests decide, not a diff: `rundown_edits.json` comes
out different on every run (upstream makes random ids when it clones or adds
an event), and the tests still pass on it. Checked locally on 2026-11-25:
the fixtures were made from upstream `0ca4d79b2`, which is still upstream
master, and a fresh generation passes every test.

The repository has issues turned off, so the job does not open one: a
failed scheduled run is emailed to whoever last edited the workflow, and the
run summary names both upstream commits, with a compare link. Nothing is
committed back. The fixtures are made-up cases, so this is safe on the
public repository.

## 7. Mutation testing (`cargo-mutants`)

## 9. Replace chrono — deferred

chrono is unmaintained. ontime-core, server-native and server-rs use it in
four files, but `worker` 0.4 depends on it too, so dropping it means moving
to `jiff` and to a newer `worker` together. Nothing is broken today; remove
the ignore in `deny.toml` when this is done.
