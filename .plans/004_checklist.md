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
- **Client:** Pages `33ac5d08` (B3). D3 and the studio text are merged but
  not deployed: the deploy was refused by the agent's permission check.
  Earlier: Pages `f2fcd906`, from branch `feature/upstream-4.15` (B1 + B2).
- **Branch:** `feat/rust-cloud-backend` holds everything
  (`feature/upstream-4.15` is merged into it). Not pushed.

## B3 — upstream client patches (done 2026-11-24, merged into `feat/rust-cloud-backend`; client not deployed yet)

- [x] 979de757 "scope rundown data sources".
- [x] All 7 patches left out of B1. Along with them came:
  - a84b8b55: navigation menu polish;
  - de2c2c1c: multi-selection context menu;
  - a60d9fff: title list waits for its scroll parent;
  - `@mantine/hooks` 8 → 9, which upstream also uses.
- [x] SourcesPanel test un-skipped.
- [x] The B3 fixes:
  - c6d23af6: PWA insets;
  - 08031fa4: rundown rows don't refetch while scrolling;
  - 3ffce666: keyboard moves made faster than the server replies;
  - 858c1ffb, a5426750: optimistic delete;
  - b4e0c15d: the list no longer jumps;
  - 07a6b314: confirm before deleting the running event;
  - 1f4f01b5: cuesheet performance;
  - the Safari clipboard fix was already in from B1.
- Tests:
  - client typecheck: only the 3 known errors;
  - client unit tests: 448 pass, 1 skipped (the countdown case, D3);
  - build passes;
  - browser: 10 views with no console errors; upstream 9, people 14 and
    members 10 checks pass.
- [x] Client deployed 2026-11-24 (Pages `33ac5d08`). The read-only production
      check passed.

## B4 — new features from upstream (chosen 2026-11-24)

- [x] **A sound when the timer ends.** It was already in from B1 (f3156b02):
      Timer view → "Play sound on timer end".
- [x] **Secondary text in the studio and timeline cards.**
  - Timeline (5c7f3732): done in B3.
  - Studio (e7237950): done 2026-11-24.
- Not taking, for now: export every rundown to Excel, and searchable settings.

## D — RIIR

- [x] **D3, countdown with skipped events (759ea78e)**, done 2026-11-24.
  - `getExpectedEnd` was ported into utils, with tests.
  - The countdown test is un-skipped.
  - Client tests: 454 pass, 0 skipped.
- [ ] **D2, gap and schedule calculations shared through the wasm.** Plan:
      `005_d2_one_copy_of_schedule_math.md`.
  - Step 1 (the shared contract, test only) is done: 36 cases, run by both
    cargo and vitest. The copies agree.
  - Step 2 (the browser uses the wasm) is deferred, as recommended and
    approved on 2026-11-24.
    - The copies agree and the contract guards them, so it isn't urgent.
    - Pick it up when the timeline or countdown math changes next.
- [ ] **D1, teleprompter.** Plan written: `008_d1_teleprompter.md`
      (2026-11-24).
  - T0 contract, T1 core, T2 Durable Object, T4 upstream client, T5 tests.
  - Its 4 decisions use the recommended answers.
  - It is the largest remaining item (about 9 days), on `feature/teleprompter`.
  - T0–T5 done locally on 2026-11-24: core, Durable Object, upstream client,
    browser check.
  - Merged and deployed on 2026-11-24: Worker `c9560deb`, Pages `1ca3aa2c`.
    The production smoke test passed 35/35.
- [x] **Both client URLs current again (2026-11-24).**
  - The user set the Pages production branch back to `main`.
  - `deploy-cloud.sh` now deploys the client to `master` (Preview, which moves
    the `master.` alias that links use) and to `main` (Production, the root).
  - First run: Preview `81aa811c`, Production `89924a11`. Both URLs serve the
    same bundle.

## Dioxus pilot

- [ ] Try Dioxus on our own pages, `/admin`, `/join` and `/my-rooms`. Plan:
      `006_dioxus_pilot_on_our_own_pages.md`.
  - Deferred, as recommended and approved on 2026-11-24.
  - The React pages work, and the bundle-size risk isn't worth it now.
  - The audit log UI was built in React.

## Sign-in phase 3

- [x] **Audit log:** who changed what, in which room. Plan: `007_audit_log.md`.
  - Approved 2026-11-24 with the 4 recommended decisions. Built and tested
    locally: core 311, e2e `activity.mjs` (25), browser check (11).
  - Deployed on 2026-11-24: Worker `02b203be` (rollback point `f366b3c1`),
    Pages `781e445f`. The production smoke test passed 25/25.

## Housekeeping

- [x] `upstream.mjs` waits for the packet it expects instead of a fixed pause.
      Three runs on production all passed.
  - **Correction:** the suite has 25 checks, not 26. The run earlier reported
    as "one failed" had in fact passed completely; that was a miscount.
- [x] Browser test for a **truly** silent socket drop.
  - Chrome's CDP "offline" mode is **not** silent: it fires a close event.
  - So the test puts a TCP proxy in front of the Worker that freezes without
    closing (`/tmp/ontime-people-check/freeze-proxy.mjs` and
    `silent-drop2.mjs`).
  - Result: no close event, the watchdog reconnects, and after the thaw the
    new socket answers.
- [x] Removed the trial worktree `/tmp/ontime-pull-trial` (2026-11-24).
- In `raknong_4ever`, one entry says it is in a group that does not list it (from
  the old `parent` bug). **Decided: leave it** (2026-11-24). Nothing is broken,
  and the room is not to be written.
- [x] Merge `feature/upstream-4.15` into `feat/rust-cloud-backend`: fast-forward
      to `58ba3d97`, 2026-11-24. Work on `feat/rust-cloud-backend` from now on.
- [x] Push to `backup`: done 2026-11-24 (`f56c8fd4`). Later pushes: `git push backup <branch>` is allowed in `.claude/settings.local.json`; pushes to `origin` are denied there.
  - `origin` (ontime-rs) is **PUBLIC**. Never push to it.
- [ ] `scripts/split-project-sharing.mjs` is not ours. Leave it alone.

## Publishing (2026-11-24)

- [x] A pre-push check, `scripts/hooks/check-push.mjs`, turned on with
      `git config core.hooksPath scripts/hooks`.
  - For a public remote it refuses real schedule files and personal values
    taken from them.
  - For every remote it refuses the master password and token-shaped text.
  - Tested by planting all three; each was refused.
  - **Never bypass it:** no `--no-verify`.
- [x] A "Source code of this deployment" link in Settings → About, for
      AGPL-3.0. Committed, not deployed.
- [x] The user edited `.claude/settings.local.json` (2026-11-24) to allow
      `git push origin *` and to deny force pushes and `--no-verify`. The agent's
      permission check refuses to widen its own permissions.
- [x] Pushed `feat/rust-cloud-backend` to `origin` (public), 2026-11-24, at
      `ba61bdd6`. The hook checked 113 commits and found them clean. From now on push to both remotes,
      and let the hook run.
- [x] Client deployed 2026-11-24 (Pages `1e580e95`). The source link on production opens github.com/ozoneRatchapon/ontime-rs. Previously:
      points at current source.
- [x] Real schedules backed up by the user to `ozoneRatchapon/ontime-ops`,
      2026-11-24. Checked: the repo is PRIVATE. Copy new schedule files there too.

## Dependabot (2026-11-24): 206 open alerts on the default branch `feat/rust-cloud-backend`

- **Nothing that ships is exposed.** Production is the static client bundle plus
  the Rust Worker.
- **The critical alert** is `proxy-addr`, which comes in through Express in the
  TS server. We don't deploy that.
- **Most alerts** come from code that never ships:
  - `apps/electron`: 27 alerts;
  - `apps/server` (TS): multer, express and the like;
  - dev and test tools: vite, happy-dom, undici, xmldom.
- **Client runtime:**
  - axios is already past the fix (1.21.1), so that alert is stale.
  - react-router #50 applies only to RSC mode, which we don't use. Bumped to
    8.5.2 anyway (`cc025051`): tests and browser walk-throughs pass. It reaches
    production with the next client deploy.
- [x] **Alerts that come only from `apps/electron` or `apps/server` were
      dismissed** on 2026-11-24.
  - 156 of 206, dismissed as `not_used` with a comment.
  - **How they were sorted:**
    - `pnpm why -r` traced each installed version back to the workspace
      packages that pull it in;
    - each alert's vulnerable range was matched against those versions.
  - **Why dismiss rather than remove the packages:** both are still
    referenced.
    - `apps/server` is used by `vitest.workspace.js`, the `Dockerfile`, the
      root `dev:prefix` script and `docs/tracking-upstream.md`.
    - Deleting them would also make upstream patches that touch them fail to
      apply.
- [x] **45 of the remaining 50 fixed** on 2026-11-24 by a dependency bump.
  - **Direct bumps:**
    - `deepmerge-ts` ^8 in utils (only the TS server's test mocks call it);
    - `happy-dom` ^21 in the client (tests only).
  - **Scoped overrides in `pnpm-workspace.yaml`:**
    - `miniflare>undici` and `miniflare>sharp`. Even the newest miniflare pins
      both inside the vulnerable ranges.
    - Same-major lifts for source-map-js, js-yaml, browserslist,
      baseline-browser-mapping, brace-expansion 5, and the vite 7 that comes
      with vitest.
    - Transitive postcss and nanoid went up within their ranges.
  - **Checks:**
    - client tests 461, utils 467, typecheck unchanged, client build OK;
    - `wrangler dev` with e2e `activity`, `people` and `shapes` all pass;
    - the 32 "local timer engine unavailable" test logs were there before the
      bump too.
- [x] **The client moved to vite 8, matching upstream (2026-11-24, `feature/vite-8`).**
  - **New versions:** vite 8.3.4, plugin-react 6.1.2, svgr 5.2.0, Sentry plugin
    5.4.0, `oxc-transform-react` in place of `babel-plugin-react-compiler`,
    and the vitest catalog at 5.0.5.
  - **Config:** upstream's `vite.config.js`, with three local choices kept:
    the legacy build off, no `.br` files (Pages compresses), and the wasm
    alias.
  - **Removed:** the pinned `@rolldown/binding-darwin-arm64` and `rollup`.
  - **Root cause of the missing native bindings (oxlint, oxfmt, rolldown):**
    - `devEngines.runtime` allowed `^22.0.0`.
    - pnpm 11 therefore skipped every binding needing node >=22.12.
    - The floor is now `^22.12.0`, and lint and format work again.
  - **Checks:**
    - the build takes 1.2–1.35 s (it was about 4.6 s);
    - wasm emitted, Worker URL present, no localhost;
    - client 511 and utils 609 tests;
    - typecheck shows only the 3 known errors;
    - oxlint shows 0 warnings on the files touched this session, and oxfmt is
      clean;
    - browser checks: `check` 14, `members` 10, `activity` 11,
      teleprompter 8.
  - The 5 remaining vite and vitest alerts should close: none of the
    vulnerable versions are installed.
  - **Deployed on 2026-11-24** with the teleprompter audit log, the script
    cache and the `api` split.
    - Worker `abbd7a7d`; the rollback point is `c9560deb`.
    - Pages Preview `46fc3dc5` (`master`) and Production `7d5cfedf` (`main`).
  - **Production smoke test:**
    - `teleprompter.mjs` 35/35 and `activity.mjs` 25/25, with throwaway rooms;
    - both URLs serve the new bundle;
    - all 4 rooms answer 200, checked read-only;
    - `/admin` and the teleprompter view boot in a browser with no errors.
