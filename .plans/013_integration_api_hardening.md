# 013 — The integration API on the public internet

Status: **planned (2026-11-25).** Item 4 of the RIIR 5 review.

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
