# 004 — Checklist of remaining work (for auto prompt)

Updated: 2026-11-24. Details live in 003 (upstream) and 001/002 (sign-in).

How to use it:
- Work top to bottom.
- Tick `[x]` only when the item's **done when** is true and its tests have run.
- Items marked **[ต้องถาม]** need the user. Stop and ask; never do them
  automatically.

## Rules that always apply

- Never write to the `raknong_4ever` room (real data). Reading it is fine.
- Deploy, push, merge into another branch, or rollback: **[ต้องถาม]** every time.
- The master password is read from the Keychain item `ontime-session-password`
  at the moment it is used. Never print it or store it.
- Before pushing, check that the repo is private.
- Commit messages are a single line, or written with `-F <file>`.
- Tests that must stay green after every item:
  - core: `cargo test` in `packages/ontime-core`;
  - client: `npx tsc -p tsconfig.app.json --noEmit` shows only the 3 known
    errors, and `npx vitest run` passes;
  - utils: `npx vitest run`;
  - e2e (`apps/server-rs/tests/e2e/run.sh`) against `wrangler dev` on :8787,
    **without** a master password;
  - `admin`, `people` and `members` with `ADMIN_PASSWORD=admin-e2e` on :8788.
    Start the :8787 and :8788 Workers one after the other, not at the same time.
- Stop every local server when finished: `wrangler dev` and `vite preview`.

## Current state

- **Worker:** `f366b3c1` (A1–A9). Rollback point: `1bf1ba98`.
- **Client:** Pages `f2fcd906`, from branch `feature/upstream-4.15` (B1 + B2).
- **Branch:** `feat/rust-cloud-backend` holds everything
  (`feature/upstream-4.15` is merged into it). Not pushed.

## B3 — upstream client patches still missing (work on a new `feature/upstream-b3` branch from `feat/rust-cloud-backend`)

- [ ] **979de757 "scope rundown data sources"** (52 files): the root that the
  rest below depend on.
  - Pull it in with `git format-patch -1 979de757 --stdout | git am -3
    --include='apps/client/*' --include='packages/*'`.
  - Resolve conflicts by keeping our room, session and timer-engine hooks.
  - Done when typecheck is at baseline and every test passes.
- [ ] **The 7 patches left out of B1.** Retry each after 979de757:
  - timeline secondary text (5c7f3732);
  - share link offers all views (f9eff7a3);
  - sentry logging (f96ca94b);
  - rundown cursor across editor layouts (989b3119);
  - cuesheet delay rows (e7f07d48);
  - quick add menu UX (e13cdf87);
  - title list follows the live event (cd6cbaef).

  Done when each one is either in, or listed in 003 with the reason it isn't.
- [ ] **Un-skip** the SourcesPanel test (Excel upload message) once 979de757 is in.
- [ ] **The conflicted fixes from 003 B3,** one commit per item:
  - the rundown list stops jumping on edits and clicks;
  - keyboard moves made faster than the server replies;
  - an optimistic delete does not mutate the cache;
  - confirm before deleting the running event;
  - cuesheet render performance;
  - the Safari clipboard for share links;
  - PWA insets.

  Done when tests pass and there is a browser check for the visible items.
- [ ] Update the B section of 003 with what came in.
- [ ] **[ต้องถาม]** Deploy the client.

## B4 — new features from upstream (chosen 2026-11-24)

- [ ] **A sound when the timer ends.** Client only. Done when there is a setting
  to turn it on or off, it plays when the timer ends, and its tests pass.
- [ ] **Secondary text in the studio and timeline cards** (5c7f3732, e7237950).
  It needs 979de757 first.
- Not taking, for now: export every rundown to Excel, and searchable settings.

## D — RIIR

- [ ] **D3, countdown with skipped events (759ea78e).**
  - It needs `getExpectedEnd` and the `getExpected` refactor in utils.
  - Pull the client side, then un-skip the test in `countdown.utils.test.ts`.
  - Consider putting the logic in `ontime-core` and the wasm.
- [ ] **D2, gap and schedule calculations shared through the wasm.** Write a plan
  file first. **[ต้องถาม]** before starting.
- [ ] **D1, teleprompter.** Write a plan file (core model + wasm + shared
  transport in the room's Durable Object). **[ต้องถาม]** before starting.

## Sign-in phase 3

- [ ] **Audit log:** who changed what, in which room. Write the plan file first.
  **[ต้องถาม]** before starting.

## Housekeeping

- [ ] `upstream.mjs` on production: the 0.5–0.8 s waits are too short for
  production's latency, and one check failed once. Wait for a condition, or
  wait longer.
- [ ] A browser test for a silently dropped socket, the case B2 actually fixes,
  for example by blocking the socket without closing it.
- [ ] Remove the trial worktree: `git worktree remove /tmp/ontime-pull-trial`.
- In `raknong_4ever`, one entry says it is in a group that does not list it (from
  the old `parent` bug). **Decided: leave it** (2026-11-24). Nothing is broken,
  and the room is not to be written.
- [x] Merge `feature/upstream-4.15` into `feat/rust-cloud-backend`: fast-forward
  to `58ba3d97`, 2026-11-24. Work on `feat/rust-cloud-backend` from now on.
- [ ] **[ต้องถาม]** Push.
  - Use only `backup` (ontime-rs-private, PRIVATE).
  - `origin` (ontime-rs) is **PUBLIC**. Never push to it.
  - The push is done by the user; the agent's permission check refuses it.
- [ ] `scripts/split-project-sharing.mjs` is not ours. Leave it alone.
