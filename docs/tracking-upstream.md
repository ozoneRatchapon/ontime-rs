# Tracking upstream Ontime

`apps/client`, `apps/server`, `packages/types` and `packages/utils` are vendored
from [cpvalente/ontime](https://github.com/cpvalente/ontime). The Rust backend
(`apps/server-rs`, `packages/ontime-core`, `packages/ontime-wasm`) is ours.

Only `apps/client` carries local modifications. Everything else is either untouched
upstream code or entirely ours, so a merge conflict can only arise in one place.

```bash
git remote -v
# origin    https://github.com/ozoneRatchapon/ontime-rs.git
# upstream  https://github.com/cpvalente/ontime.git
```

## The whole local diff

Measured, not estimated:

```
apps/client: 8 files changed, 176 insertions(+), 11 deletions(-), 2 new files
```

| File | ± | What it does, and why upstream cannot provide it |
|---|---|---|
| `src/externals.ts` | +74 | `activeRoom`, `getRoom()`, `getToken()`; puts `/r/<room>` in `serverURL` and the token in the websocket query. Our backend is multi-tenant and on a different origin from the client — upstream is neither. |
| `src/common/stores/savedViewParams.ts` | +34 | `stickyParams` and `withStickyParams()`, so `?room=` survives navigation. Upstream has no room concept. |
| `src/common/stores/__tests__/savedViewParams.test.ts` | +34 | tests for the above |
| `src/common/api/axios.config.ts` | +22 | request interceptor attaching `Authorization: Bearer <token>`, scoped to our own `serverURL` |
| `src/common/utils/socket.ts` | +9 | starts the local WASM timer engine and feeds it each `runtime-data` packet |
| `src/common/components/.../ClientLink.tsx` | +7/-2 | carries sticky params to the destination view |
| `vite.config.js` | +4 | alias for `ontime-timer-engine` → `packages/ontime-wasm/pkg` |
| `src/AppRouter.tsx` | +3 | mounts `useStickyRoom()` |
| `src/common/utils/timerEngine.ts` | new | the WASM timer engine wrapper |
| `src/common/hooks/useStickyRoom.ts` | new | keeps `?room=` in the URL after any navigation |

The two new files are additive — they cannot conflict. Realistically only
`externals.ts` and `socket.ts` are likely to move under upstream churn.

## Merging upstream

```bash
git fetch upstream
git checkout -b sync-upstream
git merge upstream/master
```

Resolve conflicts in `apps/client` using the table above: **keep both sides**. Every
entry is an addition, not a replacement of upstream behaviour — the local changes
extend `serverURL`, add params, add an interceptor. If a conflict looks like a
choice between ours and theirs, that is usually a sign the hunk should contain both.

Then, in order:

```bash
packages/ontime-wasm/build.sh
cd apps/client && pnpm build && npx vitest run src/common/stores
cd ../server-rs && cargo test --manifest-path ../../packages/ontime-core/Cargo.toml
tests/e2e/run.sh --start
```

The e2e suites are the real check: they exercise the contract between the vendored
client's expectations and our backend, which is exactly what an upstream bump can
break.

### What to watch for after a bump

- **New or renamed endpoints.** `apps/server/src/api-data/index.ts` lists the router
  mounts; compare against the match arms in `apps/server-rs/src/room.rs`. A rename
  here is how `/data/automations` came to be answered in the singular for a while.
- **Changed response types.** `shapes.mjs` asserts field types against
  `packages/types`; if upstream changes a type, that suite fails rather than the UI
  crashing at runtime.
- **New `runtime-data` fields.** `packages/types/.../RuntimeStore.type.ts` against
  `Runtime::snapshot` in `packages/ontime-core/src/runtime.rs`.
- **Changes to `externals.ts`.** The most conflict-prone file, because upstream
  actively develops URL resolution for Ontime Cloud.

## What is deliberately not synced

`apps/server` is kept for reference — it is the specification the Rust port is
written against — but it is not deployed and does not need to build. Do not spend
time fixing it after a bump unless it is being used to compare behaviour.
