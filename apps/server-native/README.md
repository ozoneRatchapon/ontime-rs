# ontime-server-native

The Ontime backend as a native binary, sharing `ontime-core` with the Cloudflare
Worker: the same rundown model, the same timer, the same routing, the same wake
policy. This crate supplies only what a Worker cannot — a filesystem to keep the
project in, and a process that can hold its own sockets.

## Running

```bash
cargo run --release -p ontime-server-native
# PORT=4001  ONTIME_DATA_DIR=./data  SESSION_PASSWORD=…
```

Point the client at it the same way as at the Worker:

```bash
cd apps/client && VITE_WORKER_URL=http://localhost:4001 pnpm dev
```

## Verifying

It answers the same suites as the Worker, because it is the same routing:

```bash
apps/server-rs/tests/e2e/run.sh http://127.0.0.1:4001
```

## What lives where

| | |
|---|---|
| `ontime-core::api` | routing, auth, the wake policy, socket commands |
| this crate | HTTP and websockets (axum), one JSON file per room, the client list |

The client list is the one thing a host owns itself, because the connections are
its own: the Worker keeps it on the socket attachment, this keeps it in a map.

Anything else implemented here rather than in the core is a bug waiting to
happen — the run report went missing from this server for exactly that reason,
until it moved into `apply_socket_command`.

## Why this exists

- The 4,561 lines of upstream that Workers structurally cannot host — automations
  and OSC, Excel import/export, project files, Google Sheets — are only reachable
  from a process like this one.
- It removes the choice between hosts: the same code runs on Cloudflare and on a
  VM, decided per deployment rather than once and for ever.

Neither of those is built yet; this is the foundation they need.
