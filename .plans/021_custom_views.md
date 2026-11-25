# 021 — Custom views on the Worker

Status: **done and deployed (2026-11-25), Worker `140bc918`, Pages `e281cb68` (master) and `e5d73a83` (main), hash-checked.** Chosen after plan 020, for events
dressed to their own theme and for OBS: a page of the room's own, added as a
browser source. Upstream's Custom views panel was hidden on the Worker, since
the Worker refused uploads.

## What it does

- **Settings → Custom views** works on the Worker as in the app: New (name +
  `index.html`), Restore demo, preview, download, delete.
- Each room keeps its own pages, stored apart from the project
  (`custom_view:<name>`, index `custom_views`), so an edit to the show never
  rewrites them. At most 20 pages of up to 1 MB each (upstream: 4 MB on disk;
  a Durable Object value holds 2 MB).
- A page is served at `https://<worker>/r/<room>/external/<name>/`, plus
  `?token=` in a locked room. That address goes in OBS as a browser source.
- The routes and answers are upstream's (`customViews.router.ts`): list,
  `POST /:name/upload` (201, 409 when taken), `GET /:name/download`,
  `POST /restore-demo`, `DELETE /:name` (204). One difference: the page is sent
  as JSON `{ html }`, as the Worker reads no multipart.
- Adding and removing a page is in the activity log (`custom-view-add`,
  `custom-view-remove`).

## How a page is kept safe, and still works

- Served with `Content-Security-Policy: sandbox allow-scripts …`: the page
  gets an origin of its own (`null`). It cannot read the Worker's or the
  editor's storage, cookies or tokens. It may run its inline script and
  style, show images and fonts from anywhere, and connect only to its Worker.
  `nosniff`, `no-referrer` and `no-store` as well.
- Upstream's checks on upload are kept and pinned against upstream (below):
  HTML only, no external scripts or stylesheets, no iframes.
- A page written for the app opens `ws://<host>/ws` and asks `/data/…` of
  its host. A small script put first in the page (`custom_views::serve`)
  points these at the room, carrying the token the page was opened with. So
  upstream's demo, and pages written from upstream's docs, work unchanged.
- Reads of a page are gated like any read of the room: a locked room's page
  needs its token (401 without). Changes need what any edit needs, including
  sign-in in a members-only room.

## How it is pinned

- `scripts/upstream-oracle/gen-custom_views.ts` runs upstream's
  `isValidCustomViewSlug` and `validateHtmlContent` over 18 names and 22
  pages. `tests/custom_views_tests.rs` gets the same answers and messages.
  It also checks the demo page and size limits, that the script goes before
  the page's own, and the policy.
- `tests/e2e/custom_views.mjs` (in `run.sh`): 33 checks covering
  storage, refusals, serving, a locked room, removal and the log.
- `tests/browser/custom_view.mjs` (in the browser `run.sh`):
  - upstream's demo page, unchanged, in Chromium under a locked room, shows
    the running event and the playing timer, from an origin of its own;
  - the editor's panel lists it, and a page uploaded through the panel is
    listed with its Worker address and token.

## Next

- Theme presets: one choice of logo, font and colours for every view of a
  room, for people who do not write CSS.
- An OBS guide: which view, which options, transparent background.
