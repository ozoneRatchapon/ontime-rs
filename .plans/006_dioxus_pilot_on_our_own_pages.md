# 006 — Pilot Dioxus on the pages that are ours

Status: **plan, waiting for approval** (2026-11-24). Proposed by the user:
https://github.com/dioxuslabs/dioxus.

## Why only these pages

- **Leave the rest alone.** The rest of the client is upstream's React UI,
  about 500 files.
  - On 2026-11-24 alone we pulled 130+ upstream patches into it.
  - Rewriting it would end that stream and take months.
  - Its heavy widgets (react-table, react-virtuoso, base-ui) have no Dioxus
    equivalents.
- **These pages are ours and have no upstream counterpart:**

  | page                 | files                                                | lines |
  | -------------------- | ---------------------------------------------------- | ----- |
  | `/admin`             | Admin, AdminLogin, RoomRow, OwnerPicker, PeoplePanel | 573   |
  | `/join`, `/my-rooms` | Join, MyRooms, TeamPanel                             | 362   |
  | their API code       | `admin.ts`, `personSession.ts`                       | 293   |

- **What Rust gains here:**
  - The types already live in `ontime-core`: `Role`, `MemberRoom`,
    `PersonSummary`, `RoomRecord`, `Directory`.
  - Dioxus can use them directly, with no hand-copied TS types to drift.
  - That is a real gain, not just a change of language.

## Shape

- **A new crate, `apps/people-ui`.**
  - Dioxus web, client-side rendering only. Use the newest **stable** Dioxus,
    checked at the start; crates.io shows 0.8 as alpha only.
  - It depends on `ontime-core` (types only, with the `spreadsheet` feature off).
- **Talks to the same Worker endpoints** (`/admin/*`, `/auth/*`) with the same
  headers. No server change.
- **Served by Pages, next to the React client.**
  - Built to `apps/client/build/people/`; `deploy-cloud.sh` copies it in.
  - Routes `/admin`, `/join` and `/my-rooms` get rewrite rules in
    `_redirects` to that build.
  - The React routes stay until the pilot is accepted, then they are deleted.
- **Shared state is localStorage:** `ontime.session` (the person session) and
  the admin token. Same keys, so moving between the two UIs keeps you signed in.
- **Styling:** reuse the CSS variables of the React client (copied
  `:root` tokens), so the pages look the same.

## What gets measured

| measure                                         | React today | Dioxus pilot        | acceptable       |
| ----------------------------------------------- | ----------- | ------------------- | ---------------- |
| first load of `/my-rooms` (bytes, gzip)         | measure     | measure             | ≤ 2× React       |
| time to interactive on a mid-range laptop       | measure     | measure             | ≤ React + 100 ms |
| lines of code for the same pages                | 1,228       | measure             | —                |
| type definitions duplicated between Rust and TS | 4           | 0                   | 0                |
| e2e and browser checks                          | pass        | must pass unchanged | all              |

## Steps

1. Spike `/join` alone. It is the smallest page: redeem a code, store the
   session, redirect. Measure. **Stop and report** if the bundle is over 2×.
2. `/my-rooms`, with TeamPanel and the members-only switch.
3. `/admin`, with the room list, people, owner and operator pickers.
4. Run the browser checks `check.mjs` and `members.mjs` against the Dioxus
   pages, unchanged.
5. Report the numbers. **[ต้องถาม]** keep Dioxus or revert.

## Risks

- **Bundle size:** Rust wasm UIs are often several hundred KB. These are admin
  pages, not the timer screens viewers open, so a larger bundle matters less.
- **Two UI stacks to maintain.** Fine for a pilot; decided at step 5.
- **Dioxus API churn between versions.** Pin the version.

## Size

- Step 1: half a day.
- Steps 2–4: two to three days.
