# 017 — Keeping a show inside the free tier (free-tier work, item 2)

Status: **step 1 done and deployed (2026-11-25), Pages `264d9e7e` / `401a4f71`; step 2 waits for the user.** The user's goal is to pay nothing.

## The limits (Cloudflare docs, read 2026-11-25, pages dated November 2026)

- Durable Objects (Workers Free): **100,000 requests a day** and 13,000 GB-s
  a day, resetting at 00:00 UTC (07:00 in Thailand). Requests include HTTP
  requests, WebSocket connects, incoming WebSocket messages (counted 20:1)
  and **alarm invocations**. Outgoing messages are free. An idle object
  eligible for hibernation is not billed for duration.
- Workers: 100,000 requests a day (REST calls and WebSocket upgrades; alarms
  and socket messages do not pass through the Worker).
- SQLite storage: 100,000 rows written and 5 million rows read a day, 5 GB in
  total.
- Past a limit, further operations of that type **fail**. Nothing is billed.

## Measured on production (one timer screen, 1 minute per state)

| State   | What the screen received or sent | Cost                                                    |
| ------- | -------------------------------- | ------------------------------------------------------- |
| playing | `runtime-data` 61 times          | the 1 s heartbeat alarm: ~3,600 requests an hour a room |
| paused  | `runtime-data` 61 times          | the same                                                |
| stopped | 3 new connections, 18 pings      | **the screen reconnected every ~10 s**                  |

The reconnecting is a bug. The client's watchdog (upstream's) treats 10 s of
silence as a dead connection, because upstream's server publishes every
second. This Worker deliberately sends nothing while nothing runs, and the
client pings only every 30 s once its clock is sampled. So an idle screen
drops and reconnects every 10 s. Each reconnect costs a Worker request, a DO
request and a burst of fast pings.

## Plan

1. **Pings under the watchdog.** On the Worker backend, the settled ping
   interval is 8 s, inside the 10 s silence window. That is about 450
   incoming messages an hour a screen, which counts as about 22 requests.
2. **A 1 s heartbeat only for screens that need it.** A screen running the
   wasm engine counts by itself; the heartbeat is there for clients that do
   not (Companion, scripts, an old bundle). The client says
   `client-set { engine: true }` once its engine is running. While every
   connected socket has the engine, the room wakes only for event ends and a
   60 s safety heartbeat. Any other client brings the 1 s heartbeat back.
3. Measure again, and write down how many screens and hours fit in a day.

## Result of step 1

Measured again on production after the deploy, one screen, a minute each:

| State   | Before                            | After                  |
| ------- | --------------------------------- | ---------------------- |
| stopped | 3 reconnects, 18 pings            | no reconnect, 9 pings  |
| playing | 61 `runtime-data` (1 s heartbeat) | the same, plus 8 pings |
| paused  | 61 `runtime-data`                 | the same, plus 9 pings |

## The budget (Durable Object requests, 100,000 a day)

| What                                  | Requests                       |
| ------------------------------------- | ------------------------------ |
| a room playing or paused, any screens | ~3,600 an hour (the heartbeat) |
| each connected screen                 | ~27 an hour (pings, 20:1)      |
| each connection                       | 1, plus 1 Worker request       |

So a day fits:

- one room running 8 hours with 20 screens: about 28,800 + 4,300 = **~33,000** (a third);
- two such rooms: about 66,000;
- three: about 99,000, at the limit;
- 20 screens left open but stopped all day: about 13,000.

Where the day starts matters: the counters reset at 07:00 Thailand time.

## Step 2, not done: a slower heartbeat

The heartbeat is about nine tenths of a show's cost. A screen with the wasm
engine counts its own timer and clock, but the **offset ("over/under") and the
expected end times come only from the server**, once a second. A slower
heartbeat would make those figures on the editor step every few seconds. The
choice is the user's, and only worth making if more than two rooms run for a
full day.
