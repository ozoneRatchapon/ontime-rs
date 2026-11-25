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
