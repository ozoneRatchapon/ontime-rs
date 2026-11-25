# 013 — The integration API on the public internet

Status: **done and deployed (2026-11-25), Worker `218983cc`.** Item 4 of the RIIR 5 review.

## Why

Upstream's API lives on a LAN. Ours is a public Worker, the calls are GETs,
and a token can sit in a URL.

## Items

1. **Rate limit per room.** A token bucket in the Durable Object, held in
   memory (eviction only resets it), for example 20 actions a second with a
   burst of 40. Over the limit answers 429 with `Retry-After`. Reads (`poll`,
   `version`) get their own, more generous bucket.
2. **Audit `change`.** An integration edit becomes an `EditEntry` note in the
   room's activity log, like the same edit made in the editor.
3. **OpenAPI spec.** `.docs/integration_api.openapi.yaml`, generated from the
   action table so it cannot drift, plus a test that every action in `parse`
   appears in it.
4. **Token in the URL.** Document a short-lived link token for senders that
   cannot set headers, if one is ever needed. Not built until asked for.

## Done when

1–3 are built, with e2e checks: a burst gets 429, an edit shows in
`/data/activity`, and the spec covers every action.

## Result

1. **Rate limit**: `integration::RateLimit` in the core, held per room by both
   hosts. Limits are as planned, with reads at 200 burst and 100 a second.
   Only calls that were let in count, so an unauthorised flood cannot lock
   the operator out. 5 unit tests. e2e: 60 parallel calls are refused in
   part, with `Retry-After`, and no more are taken than the burst plus the
   refill over the time they took to arrive. Polling still answers, and calls
   flow again after 2 seconds. The first version of this check expected at
   least 15 refusals; on production the calls arrive over 0.2–0.5 s and the
   refill takes some of them, so it now checks the bucket's own bound
   instead (measured: 45 taken in 0.47 s, 44 in 0.30 s).
2. **Audit `change`**: logged as `entry-edit`, with the fields changed and the
   sender, on both hosts. e2e reads it back where the admin token exists.
3. **OpenAPI**: `.docs/integration_api.openapi.json`, generated from
   `integration/catalog.rs` (41 calls). Tests check that every example parses,
   that all 15 actions upstream accepts are present, and that the committed
   document is current.
4. **Short-lived link tokens**: not built, as planned.
