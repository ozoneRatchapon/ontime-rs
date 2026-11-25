# 012 — OSC on the native server

Status: **planned (2026-11-25), not started.** Item 3 of the RIIR 5 review.
Only worth doing once the native server is used with show hardware on a LAN.

## Why

QLab, Companion's OSC actions and lighting consoles speak OSC over UDP. A
Worker cannot receive UDP, but `server-native` can, and the integration
dispatcher is free of any transport.

## Approach

- An OSC adapter in `server-native` (the `rosc` crate) that listens on the UDP
  port configured in settings (upstream's `osc.portIn`).
- An address `/ontime/start/index/3` with arguments becomes `(tag, payload)`
  as upstream's OscAdapter builds it. That logic goes into `integration/osc.rs`
  in the core, pinned by an oracle over upstream's `OscAdapter.ts`.
- The adapter calls `integration::handle`, then the same host path as the
  websocket, so storing, broadcasting and the audit log come for free.
- Security: listen on the LAN interface only, and only when the room enables
  OSC. Bound the packet size.

## Size

About a day, including the oracle.

## Update (2026-11-25): OSC with the cloud backend as it is

The user asked how OSC could work with the current Cloudflare deployment. A
Worker cannot receive UDP (or raw TCP), so something on the venue's network
has to receive OSC and pass it on over HTTPS. The user may try option 2.

1. **Companion as the bridge (no code).** QLab or a console sends OSC to
   Companion's own OSC remote control. That presses a button whose action is
   a Generic HTTP GET to `/r/<room>/api/...?token=...`. One button per
   command, and nothing goes back to the devices. Check the press address
   format in the docs for the Companion version in use.
2. **`ontime-bridge`, a small Rust program (preferred if OSC is used for
   real).** It runs on a machine at the venue:
   - in: it listens for OSC (`/ontime/start/cue/1`…), turns each message
     into `(tag, payload)` with a core parser pinned against upstream's
     `OscAdapter.ts`, and calls the integration API with the room token;
   - out: it keeps a websocket open and sends OSC to devices on changes,
     where upstream's OSC output does;
   - it reuses everything the integration API already does: access, rate
     limit, audit and broadcast.

Both need the venue's internet: if it drops, OSC commands do not arrive. A
native server on the LAN (the original plan above) does not have that
weakness. Latency has not been measured from a venue; expect tens of
milliseconds or more, which is fine for cues but not for frame accuracy.

About a day for option 2. Not started.
