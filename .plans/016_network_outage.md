# 016 — When the venue's network drops (free-tier work, item 1)

Status: **done locally (2026-11-25), as the user chose.**

`apps/server-rs/tests/browser/offline.mjs` drives a real timer screen and a
real editor in Chromium against the Worker, and takes each one offline
(`OUTAGE_S`, default 30 s). Measured locally on 2026-11-25.

## What happens

| Outage       | Timer screen                                                                                                       | A press in the editor while offline                                         |
| ------------ | ------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| 10 s         | keeps counting; the connection survives and catches up                                                             | arrives late, once the network is back                                      |
| 30 s         | counts for ~12 s, then **freezes** when the 10 s silence watchdog gives up                                         | **lost**; the button stays enabled, though a "connection lost" notice shows |
| network back | back in step within 0.2–1 s (the `online` event reconnects at once), showing the pause the operator made meanwhile | —                                                                           |

The freeze is deliberate (dd51ea6b1, 2026-11-24): once the socket is lost,
the screen stops a timer it can no longer trust, since the operator may have
paused or moved on meanwhile. The cost is a stage clock that stops in front
of a presenter during a long dropout.

A press is lost because `sendSocket` (upstream's code) sends only on an open
socket. Upstream runs on a LAN, where this rarely matters.

## Choices for the user

1. During a long outage, should the timer screen **hold still** (as now) or
   **keep counting** from the last state it knew, with an offline marker?
2. While the editor is offline, should the playback buttons be **disabled**,
   so a press is not silently lost? Queueing presses was ruled out: one that
   arrives late, after the operator has given up on it, is worse than none.

## The user's choices (2026-11-25), as built

1. **Keep counting, with a mark.** `scheduleReconnect` no longer stops the
   local engine (this reverses dd51ea6b1), so a screen counts on from the last
   state it was sent. Every view (`ViewLoader`) shows a small pulsing dot in
   the corner while the connection is lost (`OfflineMarker`). The editor keeps
   its fuller "Reconnecting / Disconnected" notice.
2. **Disable the buttons.** `getPlaybackControlState` takes `isOffline`, and
   `PlaybackButtons` and `TrackingPlaybackBar` pass the connection status, so
   every playback control is disabled while the connection is lost.

`offline.mjs` now checks both, with a 30 s outage: the screen counted 30 s in
30 s and showed the mark; the start button was disabled and came back with
the connection; the screen was back in step 0.2 s after the network returned.
It runs in `tests/browser/run.sh`, and all five browser checks pass.

Still true, and not changeable from the client: for the first ~10 s of an
outage the socket still looks open (the browser buffers), so a press then is
sent late when the network returns. The buttons disable once the watchdog
notices the silence and the reconnect attempts fail.
