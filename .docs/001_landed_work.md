# Landed work and closed issues

## Closed issues

### Issue 001: rundown schema parity (closed 2026-11-24)

- **Problem:** the client's `GET /data/rundowns/current` expects a full
  normalised `Rundown` (`id`, `title`, `order`, `flatOrder`, `entries`,
  `revision`). The server returned `RundownState`, so `isValidRundown` failed
  with "Network Error".
- **Fix:** 94ed137c. It wired `ontime-core` into `server-rs` and served the
  normalised `Rundown`. `/data/rundowns` returns `ProjectRundownsList`.
- **Where it lives now:** `packages/ontime-core/src/rundown_routes.rs`, at the
  `(Get, ["current"])` arm. It moved there in 2bf273ac and 156ec4f2, and later
  in the `api.rs` split.
- **Still verified by:** the browser checks (`views.mjs`, `upstream.mjs`).
  They load the current rundown in every view.
- The issue file `.issues/001_rundown_schema_parity.md` was removed after
  this record was written.

## Landed work

- **2026-11-24, D2 step 1** (ac88e051): a shared schedule-math contract.
  - The fixture is `packages/ontime-core/tests/fixtures/schedule_math.json`.
  - It is run by `schedule_math_tests.rs` and `scheduleMath.contract.test.ts`.
  - See `.plans/005_d2_one_copy_of_schedule_math.md`.
- **2026-11-24, plan 007 audit log** (ab9894c3 for the server, 3f768647 for
  the client, and the commit that adds people events).
  - Room activity for owners and the admin; people events for the admin.
  - See `.plans/007_audit_log.md` → "As built". Not deployed yet.
