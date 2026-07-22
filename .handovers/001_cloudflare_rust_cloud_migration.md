# Handover 001: Cloudflare Workers Rust Backend & Global Cloud Deployment

## Summary of Accomplishments
- **Cloud Architecture & Deployment:** Replaced local same-Wi-Fi dependency with a globally accessible Cloudflare platform.
- **Rust Engine (`apps/server-rs`):** Implemented `ontime-server-rs` with `worker-rs` and `OntimeRoom` Cloudflare Durable Objects for atomic state persistence (SQLite DO storage) and real-time WebSocket broadcasting.
- **React Frontend (`apps/client`):** Updated `externals.ts` to support `VITE_WORKER_URL`, redirected `/` to `/editor`, and implemented local timezone wall clock formatting. Deployed on Cloudflare Pages ([https://ontime-client.example.pages.dev](https://ontime-client.example.pages.dev)).
- **HTTP Automation API:** Implemented `/api/v1/control/*` REST endpoints for Stream Deck, OBS, Bitfocus Companion, and webhooks.

## Code & Test Locations
- **Rust Worker Server:** [apps/server-rs/src/lib.rs](file:///Users/ozone/ontime-rs/apps/server-rs/src/lib.rs), [apps/server-rs/src/room.rs](file:///Users/ozone/ontime-rs/apps/server-rs/src/room.rs), [apps/server-rs/src/types.rs](file:///Users/ozone/ontime-rs/apps/server-rs/src/types.rs)
- **React Client:** [apps/client/src/AppRouter.tsx](file:///Users/ozone/ontime-rs/apps/client/src/AppRouter.tsx), [apps/client/src/externals.ts](file:///Users/ozone/ontime-rs/apps/client/src/externals.ts), [apps/client/src/common/hooks/useAutoTickingClock.ts](file:///Users/ozone/ontime-rs/apps/client/src/common/hooks/useAutoTickingClock.ts)

## How to Dev & Test
1. **Develop Worker Backend Locally:**
   ```bash
   cd apps/server-rs
   npx wrangler dev
   ```
2. **Deploy Worker Backend:**
   ```bash
   cd apps/server-rs
   pnpm exec wrangler deploy
   ```
3. **Build & Deploy Client to Cloudflare Pages:**
   ```bash
   VITE_WORKER_URL=https://ontime-server-rs.example.workers.dev pnpm --filter=ontime-ui build
   pnpm exec wrangler pages deploy apps/client/build --project-name your-pages-project --branch master
   ```

## Remaining Work & Roadmap
See the detailed [Long-Term Roadmap](file:///Users/ozone/.gemini/antigravity-ide/brain/a4b1d636-8c40-4b3a-ace1-19a0f8dad5e1/longterm_roadmap.md):
- Multi-tenancy & dynamic room creation (`?room=stage-a`).
- Excel / CSV rundown import in Rust (`calamine` crate).
- Rust Wasm frontend port (Dioxus / Leptos).
