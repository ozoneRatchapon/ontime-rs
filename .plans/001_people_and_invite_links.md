# 001 — People and invite links (phase 1 of 3)

Status: **implemented and tested locally (2026-11-23); not committed or deployed yet.**
Approved as drafted: 90-day sliding sessions; the admin keeps the master password.

The person session lives in `apps/client/src/common/api/personSession.ts`.
The name `session.ts` was already taken by the app-info API.

## Goal

1. The admin, who holds the master password, can add people and send each one
   a personal invite link through LINE.
2. Opening the link once signs that device in. There is no password and no
   email.
3. The admin assigns one or more owners to each room on `/admin`. This replaces
   the free-text "owner" field.
4. A signed-in owner opens `/my-rooms` and sees the rooms they own, with links.

Out of scope here:
- Phase 2: owners invite operators, and the Editor requires sign-in.
- Phase 3: the audit log.

In this phase a person's sign-in grants **no** room access. Room passwords and
the master password work exactly as they do today. Nothing can lock anyone out.

## Data model (`packages/ontime-core/src/identity.rs`, new, pure, unit-tested)

Everything lives in the existing `@registry` Durable Object. Its own storage key
is `identity_v1`, separate from `registry_v1`, so the room list is untouched.

```rust
struct Person   { id: Uuid /* now_v7 */, name: String, created_at: i64 }
struct Invite   { code_hash: String, person: Uuid, expires_at: i64 }
struct Session  { token_hash: String, person: Uuid, created_at: i64, last_seen: i64, device: String }
struct Identity { people: BTreeMap<Uuid, Person>, invites: Vec<Invite>, sessions: Vec<Session> }
```

- Invite codes and session tokens are 32 random bytes from `getrandom`, encoded
  base64url.
  - Only `blake3` hashes are stored, so a storage dump cannot sign anyone in.
  - Comparison uses the existing constant-time `token_matches`.
- An invite is single-use.
  - Redeeming it deletes it and creates a session.
  - It expires after 7 days. Expired invites are pruned on every write.
- Creating a new invite for a person replaces their unused one, so there is only
  ever one live link per person.
- A session lasts 90 days and is extended on use. `last_seen` is updated at most
  once per hour to avoid a storage write on every request.
- `device` is a short label taken from the User-Agent, e.g. "iPhone Safari", so
  the admin can tell sessions apart.
- Removing a person also deletes their invites and sessions, and removes them
  from every room's owner list.
- `RoomRecord` gains `owners: Vec<Uuid>` (`#[serde(default)]`).
  - The old `owner: String` stays and is shown as "label (legacy)" until it is
    cleared, so no data is lost.

Pure functions, so all of them can be tested without a Worker:
- `create_person`
- `issue_invite(person, now, random) -> (code, Invite)`
- `redeem(code, now, random, device) -> Result<(token, Person)>`
- `authenticate(token, now) -> Option<&Person>`
- `revoke_sessions(person)`
- `remove_person`
- `prune(now)`

## Endpoints (Worker)

Admin endpoints, behind the master token as today:

| method | path | body / result |
|---|---|---|
| GET | `/admin/people` | people, with session count + last seen + whether an invite is pending |
| POST | `/admin/people` | `{ name }` → person |
| POST | `/admin/people/<id>/invite` | → `{ code, expiresAt }` (the client builds the link) |
| DELETE | `/admin/people/<id>/sessions` | sign that person out everywhere |
| DELETE | `/admin/people/<id>` | remove person |
| PUT | `/admin/rooms/<name>` | now also accepts `{ owners: [id] }` |

Public endpoints:

| method | path | body / result |
|---|---|---|
| POST | `/auth/redeem` | `{ code }` → `{ token, person }` |
| GET | `/auth/me` | Bearer session → `{ person, rooms: [RoomRecord] }` (owned rooms only) |
| POST | `/auth/logout` | Bearer session → deletes that one session |

- A wrong or expired code returns 401 with a single, uniform message, so it is
  impossible to tell "never existed" from "already used".
- 256-bit codes make guessing hopeless, so no rate limit is needed in this phase.
- `/auth/*` is routed in `lib.rs` next to `/admin`. The logic goes in a new
  `apps/server-rs/src/auth.rs`, which forwards to the `@registry` stub the same
  way `admin.rs` does.

## Client

- **`/join#code=…`**
  - The code sits in the fragment, so it never appears in server or proxy logs.
  - The page redeems the code and stores the session in `localStorage` under
    `ontime.session`. It is localStorage, not sessionStorage, because "remember
    this device" is the whole point.
  - It then goes to `/my-rooms`.
  - It shows a clear message for a used or expired link: "ask the admin for a
    new link".
- **`/my-rooms`**
  - Shows the person's name and their rooms. Each room has editor, cuesheet,
    timer and backstage links, plus the live viewer counts from the registry.
  - It has a "sign out of this device" button.
- **`/admin`**, new "People" section:
  - Add a person.
  - "Create invite link" copies `https://master.ontime-team.pages.dev/join#code=…`
    and shows its expiry.
  - Sign a person out, or remove them.
  - Each room row has an owner picker (multi-select of people), replacing the
    free-text box.
- Like `/admin`, neither new page opens the room websocket. The check in
  `App.tsx` widens to `/admin`, `/join` and `/my-rooms`.

## Scripts

`seed-room.mjs` keeps sending `owner` text, which still works as the legacy label.
Assigning real owners is done on `/admin`, because people's IDs do not belong in
schedule files.

## Tests

- `packages/ontime-core/tests/identity_tests.rs` checks that:
  - an invite redeems once, the second try fails, and an expired one fails;
  - a new invite replaces the old one;
  - authentication works and an unknown token fails;
  - revoking signs out every device;
  - removing a person also takes them off room owner lists;
  - prune drops only expired records;
  - an old `registry_v1` record without `owners` still deserialises.
- `apps/server-rs/tests/e2e/identity.mjs` runs against local wrangler. Like
  `admin.mjs`, it is skipped unless `ADMIN_PASSWORD` is set. It covers:
  - add person → invite → redeem → `/auth/me` shows no rooms → assign owner → `/auth/me` shows the room;
  - a second redeem gives 401, sign-out gives 401, and removing the person gives 401;
  - the room APIs and room passwords behave exactly as before;
  - the existing `admin.mjs` checks still pass.
- Playwright headless run of `/join` → `/my-rooms` and of the `/admin` People
  section, checking for zero console errors.

## Rollout

1. Run everything locally: `cargo test`, clippy, all e2e tests, and the browser checks.
2. Deploy the Worker, then the client, with the usual `deploy-cloud.sh` command.
   Record the Worker version id beforehand as the rollback point.
3. Production smoke test:
   - health, and `/auth/me` without a token gives 401;
   - create a test person, invite, redeem, then remove that person;
   - the room list and `is-exam-mbv-bio` are unchanged;
   - **`raknong_4ever` is not touched.**
4. You add the real people and send the links yourself. I do not know who the
   owners are.

## Decisions to confirm

- Session length is 90 days, extended on use. A shorter limit is safer on shared
  computers, but means sending links more often.
- The admin keeps using the master password. The admin is not also a "person"
  in this phase.
