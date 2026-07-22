# Issue 001: Rundown Creation & Client Schema Parity

## Problem
The React UI client calls `GET /data/rundowns/current` expecting a full normalized `Rundown` object (`{ id, title, order, flatOrder, entries: Record<string, OntimeEntry>, revision }`). Previously, returning `RundownState` caused `isValidRundown` client validation to fail with `Network Error`.

## Solution
1. Connected `ontime-core` shared crate into `apps/server-rs/Cargo.toml`.
2. Updated `/data/rundowns/current` in `room.rs` to construct and return the normalized `Rundown` structure with `order`, `flatOrder`, and `entries` map.
3. Updated `/data/rundowns` GET/POST responses to return `ProjectRundownsList` (`{ loaded, rundowns: [...] }`).
4. Re-deployed `ontime-server-rs` to Cloudflare Workers (`14659e64-5a01-4641-af66-d00b50cdacbc`).
