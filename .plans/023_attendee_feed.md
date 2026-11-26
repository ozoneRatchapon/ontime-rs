# 023 — What a room shows its audience, without a token

Status: **done, deployed 2026-11-26 (Worker c93cdf3e, Pages ed0f4ec3 / dfb1fb62); production smoke passing.** Step 1 of linking ontime-rs with
BeThere (`~/event-checkin`), as the user chose.

## Why

A room's token lets its holder change the room. The only way to show a locked
room's timeline to attendees was a share link carrying that token, so anyone
with an attendee link could edit the show. Upstream has the same design, made
for a LAN. On the internet, and for BeThere's event page, attendees need a way
in that can only read, and only what is meant for them.

## What it does

- **Off until the room turns it on**: Settings → Network → **Attendee page**
  (Worker only). A switch, plus the custom fields attendees may see (such as a
  presenter). Changing it needs what any edit needs, and is logged
  (`public-feed`).
- `GET /r/<room>/public`: `{ agenda, live }` as JSON, open to any site (CORS
  `*`), for BeThere and others.
- `GET /public/ws?room=<room>`: the same, live. The room sends only what
  changed: the agenda when the show is edited, `live` when the operator acts
  or an event ends. A tick alone sends nothing; the page counts by itself.
- `GET /r/<room>/public/agenda/`: a page for attendees' phones. It shows the
  agenda with planned times, what is on now with a countdown and progress bar,
  what is next, and a chip saying "running N min late / ahead / on time" (the
  room's own offset, positive = behind; hours past 60 min). It also shows when
  the show is expected to end. Thai or English, light or dark (BeThere's
  colours and fonts), `embed=1` for a frame. It is served sandboxed like a
  custom view.
- All three answer 404 while the feed is off. Turning it off closes every
  audience socket ("This room closed its public page"), and the page says so.

## What is in, and what never is (`ontime_core::public_feed`)

- In: the project title, the loaded rundown's title, and its events (cue,
  title, planned times, colour, group), milestones and group titles, in order.
  Custom fields only when named, shown by label. `live`: playback, now and
  next ids, the timer, the offset, planned start and end, and the expected end.
- Never: notes, skipped events, delays, unnamed custom fields, other
  rundowns, messages, the report, the teleprompter, tokens, the activity log.

## The audience's sockets

- They are tagged `public`. They only receive; whatever they send is ignored
  before the room is even read.
- They are not clients: not in the client list, not sent operator traffic,
  and not counted for the 1 s heartbeat. A crowd therefore does not wake the
  room, and costs one request each to connect.
- At most 1,000 per room (503 after that).
- Every other socket loop in the room now goes through `operator_sockets()`.

## How it is pinned

- `tests/public_feed_tests.rs` (7):
  - off by default; only an editor turns it on;
  - bad changes refused with nothing changed;
  - the agenda carries no note, skipped event, delay, unnamed field, other
    rundown or token;
  - fields shown by label; `live` now/next; a tick is not a change;
  - an import keeps the room's feed.
- `tests/e2e/public_feed.mjs` (31, in `run.sh`), from outside:
  - off/404 before it is turned on; a stranger cannot turn it on;
  - readable without a token, CORS `*`;
  - no secret anywhere in the answer, and the room itself still 401;
  - the audience socket's commands change nothing, and it is not a client;
  - playback and an edit reach it; a tick alone sends nothing;
  - turning it off closes it with a reason, and both changes are logged.
- `tests/browser/public_agenda.mjs` (in the browser `run.sh`):
  - the editor's switch turns it on and shows the link;
  - at 390 px with no token: agenda, times, presenter, title and Thai; no
    note; now, the countdown counting by itself, live, next, and the late chip;
  - closed when the room turns it off; no page errors and nothing refused by
    the policy.
  - Screenshot checked by eye.

## Not done, on purpose

Expected start times per upcoming session. The exact figure needs upstream's
look-ahead over gaps that absorb lateness. A guess (planned + offset) would
mislead, so the page shows planned times, the lateness chip and the room's
exact expected end.

## Next: BeThere (draft, nothing changed in that repo)

BeThere's `.plans/019` keeps ontime-rs at arm's length (GPL vs its wasm), and
reading this feed over HTTP/WebSocket stays at arm's length.

1. **Now/Next card on `/e/{slug}`.** An organizer pastes the room's feed URL
   into the event form. The page opens the socket and shows a card with what
   is on now, the time left and what is next, in BeThere's own components. Or,
   first and smallest, an `<iframe>` of `/public/agenda/?embed=1`.
2. Phase 1's agenda text could be filled from `GET /public` instead of a CSV.
3. Later, after BeThere decides `.plans/041` and `019` §6.1: a rotating
   check-in QR on an OBS page, and per-session attendance from the room's
   actual start and end times.
