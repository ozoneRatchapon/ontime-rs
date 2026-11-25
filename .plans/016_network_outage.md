# 016 — When the venue's network drops (free-tier work, item 1)

Status: **measured (2026-11-25); waiting for the user's choices.**

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
