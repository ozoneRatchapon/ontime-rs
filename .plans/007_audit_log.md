# 007 — Audit log: who changed what, in which room (sign-in phase 3 of 3)

Status: **built and tested locally (2026-11-24); not deployed yet.** Builds on 001 and 002.

## As built

The 4 decisions were approved as recommended: field names only, 2,000 lines
or 90 days per room and 365 days for the registry, owners and admin read,
names kept as they were.

**Where the code is:**
- `ontime-core`:
  - `audit.rs`: the line, actor and action types, plus chunked storage,
    pruning, merging and paging;
  - `audit_describe.rs`: what a room request or socket command records, and
    who the actor is;
  - `registry_audit.rs`: people events.
- `server-rs`:
  - `audit_store.rs`: chunk load and page, and the batched write;
  - `room.rs` and `registry_host.rs` use it;
  - `/admin/activity` is in `admin.rs`.
- Client:
  - `common/api/activity.ts`, `common/utils/activityText.ts`;
  - `views/admin/ActivityPanel.tsx`, used by `/my-rooms`, the admin room rows
    and the admin "People activity" section.

**Changes from the plan:**
- **Storage is chunked, not one key.**
  - `worker` 0.4.2 has no SQL API.
  - With a single 300 KB key, every change would have rewritten 300 KB.
  - Instead, lines live in chunks of 100 under `audit_v1:<n>`, and a change
    rewrites only the newest chunk (at most about 15 KB).
  - The room's index of chunks is a field of the project document, which is
    written anyway. The registry keeps its index under `audit_index_v1`.
  - Each change goes out in a single `put_multiple`, so a change and its line
    are written together.
  - The 2,000-line limit is enforced as 20 chunks of 100.
- **One room endpoint, for owners and the admin:**
  `GET /r/<room>/data/activity?before=&limit=`.
  - It replaces `/auth/rooms/<room>/activity` and
    `/admin/rooms/<room>/activity`.
  - The room already knows both callers, so the registry doesn't need to be
    involved.
  - Registry events are at `GET /admin/activity`.
- **"Show older" is a button,** not loading more on scroll.
- **Members-only on or off is recorded room-side** (where it happens), not
  in the registry.
- **A command that changes nothing is not recorded,** for example a second
  pause.
  - Repeats that do change something within 2 s merge, such as add-time
    tapped twice.
- **A restore keeps the room's own index,** and a downloaded backup leaves the
  log out.

**Not done:** `server-native` (the LAN host) does not record activity yet.

## Rollout still to do

1. Record the Worker version.
2. Deploy the Worker, then the client.
3. Smoke test with a throwaway person and room.
4. Remove the throwaway person and room.

## Goal

- An owner opens their room on `/my-rooms` → "Activity" and sees what happened
  and who did it, for example:
  - "Somchai (operator) started cue 2.1 — 09:30";
  - "the room password edited 'Keynote' — 09:12".
- The admin sees the same for every room, plus the people events: invites,
  sign-ins, removals.

## Who counts as the actor

| how the request was allowed | recorded as |
|---|---|
| a person's session | the person: id, plus their name at that moment |
| the room password | "room password" |
| the master password | "admin" |
| nothing, in an open room | "anyone with the link" |

- **No IP addresses, no user agents.**
  - The sessions already hold a device label, which is enough to tell "Somchai
    on iPad" apart.
  - Storing more would be personal data with no use here.
- **For this, the room has to learn who the person is, not only their role:**
  - `auth/check` returns `{ person, role, name }`; today it returns
    `{ person, role }`;
  - the room's 60 s cache keeps the id and the name next to the role.

## What is recorded

**Room-side** (in the room's Durable Object), one line per change:

| kind | examples | detail kept |
|---|---|---|
| playback | start, pause, stop, load, roll, add time | cue and title of the event |
| rundown | add, edit, delete, reorder, replace, renumber | cue and title; for an edit, the field **names** that changed (not values) |
| rundown list | create, rename, delete, load | title |
| room | settings, project data, custom fields, password set or cleared, members-only on or off | which section |
| report | archive, clear | — |

- **Not recorded:**
  - aux timer ticks and the message overlay text;
  - clients connecting and disconnecting (already visible live on `/admin`);
  - reads.
- **Field values are not copied** into the log. Notes can hold names, which is
  why the log keeps only which fields changed.
- **Repeated identical playback commands** within 2 s from the same actor merge
  into one line, so double-taps don't flood it.

**Registry-side**, for the admin only:
- a person created or removed;
- an invite made or redeemed;
- a session signed out;
- owners or operators changed;
- members-only switched.

## Storage and limits

- **A ring buffer per room in Durable Object storage**, key `audit_v1`.
  - It keeps the last **2,000 lines or 90 days**, whichever is smaller.
  - Each line is about 150 bytes, so up to about 300 KB per room.
- **The registry keeps its own ring:** 2,000 lines or 365 days.
- **Writing:** a line is appended in the same persist as the change it
  describes, so a change and its log line cannot disagree.

## Who may read

| endpoint | who |
|---|---|
| `GET /auth/rooms/<room>/activity?before=<ts>&limit=100` | owners of that room |
| `GET /admin/rooms/<room>/activity` | admin |
| `GET /admin/activity` (registry events) | admin |

Operators and viewers can't read it.

## UI

- **`/my-rooms` → a room → "Activity":** a list of lines, newest first, with
  more loaded as you scroll.
- **`/admin` → a room row → "Activity",** plus an "Activity" tab for the
  people events.

If plan 006 (Dioxus) is accepted first, this UI is built there instead of in
React.

## Tests

- **Core:**
  - the actor rule, for each way in;
  - each recorded kind produces exactly one line;
  - an edit records field names and never their values;
  - the ring drops the oldest line by count, and by age;
  - repeated playback lines merge;
  - an owner can read and an operator can't.
- **e2e, `activity.mjs`:**
  - an owner, an operator and the room password each make a change;
  - the owner sees all three, correctly attributed;
  - the operator gets 403;
  - a removed person still appears in old lines, under the name kept at the
    time.
- **Browser:** the Activity list on `/my-rooms` and `/admin`.

## Rollout

Same as 001 and 002:
1. Record the Worker version.
2. Deploy the Worker, then the client.
3. Smoke test with a throwaway person and room.

Old rooms start with an empty log. Nothing is backfilled.

## Decisions to confirm

1. Edits record field names only, never values.
2. Limits: 2,000 lines or 90 days per room; 365 days for the registry.
3. Owners and admin can read; operators can't.
4. Names are stored as they were at the time, so a later rename doesn't change
   old lines.
