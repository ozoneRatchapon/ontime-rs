# 002 — Operators and members-only rooms (phase 2 of 3)

Status: **done — committed in `2fda18ef`, deployed 2026-11-23 (Worker `1bf1ba98`).**
All three decisions were approved as drafted.

Notes from building it:

- A refused socket command is answered with a `log` packet, which the editor
  already shows.
- An owner removing an operator they invited, who is then in no room at all,
  removes that person entirely instead of only signing them out. Builds on 001, which is live in
  production. Its Worker version `53bef09b` is the rollback point.

## Goal

1. An owner invites operators to their own room from `/my-rooms`, without the
   admin. They can also remove them.
2. A signed-in owner or operator can open their room's editor **without knowing
   the room password**.
3. An owner can switch a room to **"editing needs sign-in"**. After that, only
   these can control the room (edit the rundown, start or stop cues):
   - its members;
   - the room password, while it is still set;
   - the master password.

   Viewer screens (timer, backstage, cuesheet read-only) stay open as they are
   today.

Nothing changes for any room until its owner flips the switch. It is off by
default.

Today, 3 of the 4 production rooms have no password at all (`default_room`,
`is-exam-mbv-bio`, `solana-thailand`): anyone with the link can edit them. The
switch is what closes that gap.

## Roles

| who                     | can                                                     |
| ----------------------- | ------------------------------------------------------- |
| admin (master password) | everything, as today                                    |
| owner                   | control the room, manage its operators, flip the switch |
| operator                | control the room                                        |
| viewer (no sign-in)     | watch: connect, read, set their own screen name/path    |

`RoomRecord` gains `operators: Vec<Uuid>` (`#[serde(default)]`).

`Person` gains `invited_by: Option<Uuid>`, which is `None` when the admin made them.

## Owner endpoints (session token, scoped to rooms the caller owns)

| method | path                                       |                                                         |
| ------ | ------------------------------------------ | ------------------------------------------------------- |
| GET    | `/auth/rooms/<room>/team`                  | owners + operators: name, devices, pending invite       |
| POST   | `/auth/rooms/<room>/operators`             | `{ name }` → new person, added as operator, invite link |
| POST   | `/auth/rooms/<room>/operators/<id>/invite` | a new link for an operator **this owner invited**       |
| DELETE | `/auth/rooms/<room>/operators/<id>`        | remove them from this room only                         |

**Security rule:** an owner may make links only for people they invited
themselves. Otherwise owner A could make a new link for someone who owns other
rooms, open it, and sign in as that person. The admin can still do anything.

Removing an operator who is no longer in any room, and was invited by this
owner, also signs them out of every device.

`/auth/me` returns rooms with a `role` (`owner` | `operator`).

## How a room knows who is asking

- **Transport:**
  - The client sends the person session separately from the room token, so
    both can be present at once.
  - REST uses the header `X-Ontime-Session`.
  - The websocket uses the query string `?session=`, because a websocket cannot
    set headers. The room token already travels the same way.
- **Lookup:**
  - The room's Durable Object asks the registry: `auth/check { token, room }` →
    `{ person, role }` or 401.
  - The answer is cached in the room's memory for 60 s, keyed by the token hash.
  - So a removed person can keep control for up to 60 s. Refusals are cached
    too, so a bad token cannot hammer the registry.
- **Rule** (pure, in `ontime-core::api`, unit-tested):
  - `is_authorised` keeps today's behaviour, and a member counts as authorised
    even on a password room.
  - A new `may_control(project, req, member)` applies only when
    `project.members_only` is set. Control then needs a member, the room
    password, or the master token.
  - What counts as **control**:
    - every REST write (non-GET) under `/data/` and `/api/`;
    - runtime commands over the websocket (start, pause, stop, load, roll, aux
      timers, message overlay).

    What stays **viewing**: GET requests, opening the socket, and the
    client-identity messages (`client-set-path`, rename, ping). The exact list
    of websocket message types is taken from `room.rs` and covered by a test.

- **The switch:**
  - `project.members_only: bool`, stored in the room next to `password_hash`.
  - Set by `POST /data/members-only { on }`, which only an owner or the master
    token may call.
  - The room reports it to the registry, which shows it on `/admin`.

## Client

- **`externals.ts`:** if `ontime.session` exists, attach `X-Ontime-Session`
  (axios) and `?session=` (socket). The room token logic stays as it is.
- **`/my-rooms`:**
  - The role is shown per room.
  - Owners get a "Team" panel per room: add an operator (makes and copies a
    link), make a new link, remove.
  - Owners also get the "Editing needs sign-in" switch, with a confirm that
    explains the effect.
- **Editor in a members-only room without sign-in:** a clear notice: "This room
  needs sign-in to edit — ask the owner for an invite link". The rundown still
  shows. Control buttons fail with that message rather than silently.
- **`/admin`:** shows each room's operators and a 🔒 icon when it is
  members-only.

## Tests

- **Core:**
  - The rule matrix: open, password, members-only × anonymous, room token,
    master, owner, operator, and someone else's session. Each against view and
    control.
  - The owner scoping rules, including "cannot re-link a person someone else
    invited".
  - Old records load with empty `operators`.
- **e2e `members.mjs`:**
  - owner invites an operator;
  - the operator controls the room;
  - with the switch on, an anonymous viewer can watch but cannot start a cue,
    over both REST and the socket;
  - removing the operator takes away control within the cache window. The test
    waits it out.
- **The existing 15 suites still pass,** which proves that with the switch off
  nothing changed.
- **Browser:** the owner team panel, an operator signing in and opening the
  editor of a password-protected room without the password, and the
  members-only notice.

## Rollout

Same as 001:

1. Record the Worker version.
2. Deploy the Worker, then the client.
3. Smoke-test production with a throwaway person.

**No room is switched to members-only by me.** That is the owner's choice per
room. `raknong_4ever` is not touched.

## Decisions to confirm

1. The switch is off by default and per room.
2. An owner can re-link only people they invited.
3. Up to 60 s before removal takes effect, in exchange for not asking the
   registry on every request.
