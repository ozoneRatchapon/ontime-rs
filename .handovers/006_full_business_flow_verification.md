# Handover 006: Full Business Flow Parity & Frontend Worker URL Resolution

## 1. What Happened
- Identified and resolved the root cause of the blank screen (`หน้าตาที่แสดงยังเป็นว่างเปล่าอยู่`) at `https://ontime-client.example.pages.dev/editor?settings=manage__rundowns`.
- **Root Cause:** When `apps/client` was built for Cloudflare Pages without setting `VITE_WORKER_URL`, `externals.ts` defaulted `serverURL` to `window.location.origin` (`https://ontime-client.example.pages.dev`). Since Cloudflare Pages hosts static assets and has no worker backend on that domain, API requests to `/data/*` received 404 HTML pages, causing React Query / Axios parsing errors and unhandled React ErrorBoundary blank screen crashes inside `ManagePanel`.
- **Fix:** Rebuilt `apps/client` with `VITE_WORKER_URL=https://ontime-server-rs.example.workers.dev` and deployed the compiled bundle to Cloudflare Pages (`your-pages-project`).

## 2. Code & Deployment References
- **Frontend Code:** [/Users/ozone/ontime-rs/apps/client/src/externals.ts](file:///Users/ozone/ontime-rs/apps/client/src/externals.ts)
- **Backend Worker:** `https://ontime-server-rs.example.workers.dev`
- **Pages URL:** `https://ontime-client.example.pages.dev/editor?settings=manage__rundowns`

## 3. How to Verify
- Open `https://ontime-client.example.pages.dev/editor?settings=manage__rundowns` after clearing browser cache (or using Incognito / Hard Refresh `Ctrl+Shift+R`).
- Verify that Manage Rundowns, Rundown Defaults, and Custom Fields render cleanly with data from the Rust edge backend.
