# Handover 008: Timer Engine, Free-Plan Migration, and Cost Evaluation

## 1. The Durable Object had never worked in production

Before this session the deployed Worker returned **500 on every Durable Object route**:

```
GET https://ontime-server-rs.example.workers.dev/api/state  → 500
GET .../ws                                                          → 500
```

The cause was in `wrangler.toml`:

```toml
[[migrations]]
tag = "v1"
new_classes = ["OntimeRoom"]     # key-value backed
```

`new_classes` creates a **key-value backed** Durable Object, which is a paid-plan
feature. On the Workers Free plan only **SQLite-backed** Durable Objects
(`new_sqlite_classes`) can be instantiated. Every REST route happened to be answered
by hardcoded literals in the Worker itself (see handover 007), so the failure was
invisible — nothing in the UI depended on the DO actually working.

### The migration

Storage type is immutable once a namespace exists, so the class could not be
converted. `OntimeRoom` was replaced by a new SQLite-backed class:

```toml
[[durable_objects.bindings]]
name = "ONTIME_PROJECT"
class_name = "OntimeProject"

[[migrations]]
tag = "v2"
deleted_classes = ["OntimeRoom"]
new_sqlite_classes = ["OntimeProject"]
```

Two failed attempts are worth recording, because the ordering is not obvious:

- Renaming only the class → `10061 Cannot apply --delete-class migration to class
'OntimeRoom' without also removing the binding that references it`
- Dropping `deleted_classes` → `10064 New version of script does not export class
'OntimeRoom' which is depended on by existing Durable Objects`

The binding **name** has to change too (`ONTIME_ROOM` → `ONTIME_PROJECT`), not just
the class name. With both renamed and `deleted_classes` present, the deploy succeeds
in one step.

## 2. Timer engine

`packages/ontime-core/src/runtime.rs` (new) — the playback state machine and timer
maths, kept pure so it is testable without a Worker.

**Time model.** `startedAt` / `pausedAt` are stored as **epoch milliseconds**, so
pausing and resuming never has to reason about midnight. The client's `RuntimeStore`
speaks _time of day_ (ms since local midnight), so the conversion happens once, in
`snapshot()`.

**Timezone.** A Worker has no local timezone — it is always UTC. Without correction,
every clock in the UI reads UTC. `Project.timezone_offset` holds the operator's UTC
offset and is applied in `snapshot()`. Set it over REST:

```bash
# Thailand, UTC+7
curl -X POST $WORKER/data/settings -H 'content-type: application/json' \
     -d '{"timezoneOffset": 25200000}'
```

It is returned from `GET /data/settings` alongside the normal settings. This field is
not part of the client's `Settings` type; the client ignores it.

**Commands implemented** (parsed from the websocket tags `apps/client` sends):
`start`, `start:next`, `start:{id}`, `load:next`, `load:previous`, `load:{id}`,
`pause`, `stop`, `reload`, `addtime`. `addtime` is in **milliseconds** — the previous
code treated it as seconds.

**End actions.** `on_tick` applies the loaded event's `endAction` when its time is up:
`load-next` arms the next event, `play-next` rolls straight into it, `none` runs into
overtime (the default an operator expects). On the last event it holds rather than
looping.

`phase` follows the event's own `timeWarning` / `timeDanger`, and `current` goes
negative in overtime.

### Wiring in `apps/server-rs/src/room.rs`

- `websocket_message` parses a `Command`, applies it, persists, broadcasts
  `runtime-data`, then reconciles the alarm.
- `alarm` ticks, applies any end action, broadcasts, and reschedules.
- On websocket connect the DO sends a `runtime-data` packet immediately, so a client
  joining mid-show sees the state without waiting for the next tick — and sees
  something at all when nothing is running.
- `commit()` also broadcasts `runtime-data`, because editing the loaded event changes
  its duration or title.

**Alarm discipline.** `sync_alarm()` sets a 1 s alarm only while `playback == play`,
and deletes it otherwise. This is the single most important cost decision in the
codebase — see below.

## 3. Verification

All against the deployed Worker unless stated.

- `cargo test -p ontime-core` — **49 passing** (26 new in `tests/runtime_tests.rs`:
  command parsing, countdown, overtime, warning/danger phases, pause/resume
  arithmetic, addtime both directions, load next/previous with bounds, start-next,
  reload, skipped events, end actions, snapshot shape against `runtimeStorePlaceholder`,
  playback enum wire values, timezone conversion, serialisation round trip).
- REST smoke test — 40 checks passing.
- WebSocket test — `client-init`, `ping`→`pong`, mutation broadcasts `refetch`.
- Timer test over a real websocket — 20 checks passing, including
  _"no ticks while stopped"_, which is what keeps the cost model valid.
- Endpoint sweep — all 15 paths the client fetches on load return 200.

## 4. Does the free plan actually hold? Yes, with room to spare

Verified limits (Workers Free / Durable Objects Free):

| Resource        | Free allowance              |
| --------------- | --------------------------- |
| Worker requests | 100,000 / day               |
| DO requests     | 100,000 / day               |
| DO duration     | 13,000 GB-s / day           |
| DO rows written | 100,000 / day               |
| DO rows read    | 5,000,000 / day             |
| DO storage      | 5 GB total, 1 GB per object |
| Pages bandwidth | unlimited                   |

The decisive billing rule: **outgoing WebSocket messages are not charged**, and
incoming ones are discounted 20:1. Broadcasting the timer to viewers is therefore
effectively free, and viewer count is not a cost driver at all.

What does cost:

| Activity                                    | DO requests | Rows written |
| ------------------------------------------- | ----------- | ------------ |
| 1 hour of playback (1 Hz alarm)             | 3,600       | 3,600        |
| One viewer loading the app (~15 REST calls) | 15          | 0            |
| One operator edit                           | 1           | 1            |
| Idle (nothing playing)                      | **0**       | **0**        |

Binding constraint is rows written: 100,000 ÷ 3,600 ≈ **27 hours of playback per day**.
For any realistic show that is unlimited. On the Worker side, 100,000 requests/day is
roughly 6,000 app loads a day.

**Conclusion: this runs at 0 baht and is not close to any limit.** The idle cost being
exactly zero is what makes it safe — an unattended deployment cannot quietly burn the
daily budget.

## 5. Is Cloudflare the right choice, though?

Cost is not the differentiator — both viable options are 0 baht. The difference is
what you give up and what you maintain.

### What Workers structurally cannot do

These are not unfinished work; they are platform limits:

- **OSC / UDP** — automations and the OSC integration need raw sockets. Workers has none.
- **Filesystem** — project files, Excel/CSV import and export, CSS and logo uploads.
- **Long-lived OAuth sessions** — Google Sheets sync.
- **Local network presence** — the original is discoverable on the LAN, which is how
  most Ontime rigs are operated.

### What is ported but incomplete

Roll mode, aux timers, the message service, report, custom views, and the full offset
calculation. The offset currently shipped is a single-event approximation, not the
original's rundown-wide calculation.

### The alternative: run the original server, also at 0 baht

`apps/server` is 20,046 lines of TypeScript and works today with full parity. Hosting
it free:

| Option                                | Verdict                                                                                                                                            |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Oracle Cloud Always Free**          | 2 OCPU / 12 GB ARM VM, free with no time limit (halved from 4/24 in June 2026). Comfortably runs the Node server. **Best parity-for-free option.** |
| **Spare machine + Cloudflare Tunnel** | Free if the hardware exists; Tunnel is free and gives TLS without port forwarding.                                                                 |
| **Render free tier**                  | Not viable — spins down after 15 min of inactivity, which drops WebSocket connections, and takes ~1 min to wake. Fatal for live show control.      |
| **Fly.io / Railway**                  | No usable free tier any more.                                                                                                                      |

### Recommendation

- **If you need full Ontime** (automations, OSC, import/export, roll mode): put the
  original Node server on an Oracle Always Free VM. Same 0 baht, 100% parity, and no
  ongoing cost of tracking a 20k-line upstream in a parallel Rust implementation.
- **If you need a cloud rundown + timer for many viewers** and no hardware
  integrations: the Workers build now does that, and does it _better_ than a single
  VM — global edge, no server to patch, and viewers are free because outgoing
  WebSocket traffic is not billed.

The honest framing: the Rust/Workers path buys operational simplicity and viewer
scale, and pays for it in feature coverage and in maintaining a second implementation
of a moving target. It is not cheaper — both are free.

### Storage ceiling

Moving to the SQLite backend also raised the per-value ceiling: a key plus its value
may be up to **2 MB** (the key-value backend allowed 128 KiB). The project document is
a few KB today, so splitting storage per rundown is not urgent.

## 6. Running it

```bash
# backend
cd apps/server-rs && worker-build --release && npx wrangler dev --port 8787 --local
npx wrangler deploy                     # to Cloudflare

# client against it
cd apps/client && VITE_WORKER_URL=http://127.0.0.1:8787 pnpm dev --port 5173
```

Deployed: `https://ontime-server-rs.example.workers.dev`
