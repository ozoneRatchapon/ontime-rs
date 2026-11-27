# 024 — BeThere's look for the screens and the stream

Status: **done locally, not committed, not deployed.** Item 3 of linking
ontime-rs with BeThere (after plan 023's attendee feed and BeThere's plan 050
card), chosen by the user on 2026-11-27.

## Why

Attendees register on BeThere, then see Ontime's views on the screens in the
room and on the stream. They should look like one event. Plan 023's attendee
page already uses BeThere's colours and fonts; the OBS pages (plan 022) and the
app's own views (timer, timeline, backstage…) did not.

## What it does

- **OBS pages:** `theme=bethere` in the address of `obs-countdown` and
  `obs-timer`. Navy (`#12152a`), cream text (`#e9e4d3`), indigo numbers
  (`#8f88ff`), BeThere's warning and danger colours, Inter with Anuphan for
  Thai. Any option given in the address still wins over the theme.
  Anuphan is asked for at 400 and 700 only, because Google refuses a whole
  request when one weight is missing from a family.
- **App views:** **Settings → View settings → Edit CSS override → Load
  BeThere theme** puts a ready stylesheet in the editor. It ships with the
  client and needs no request. It sets upstream's `--*-override` variables,
  plus two `.timeline` rules for colours upstream hard-codes (the live title
  in red, the times in green). Navy is the default, for a dark room. The paper
  values are listed in a comment at the end, for a bright room.
- Colours come from BeThere's `frontend-leptos/styles/style-23-landing.css`
  (dark theme) and `style-24-attendee-site.css` (status colours).
- `.docs/004_obs_macbook_iphone_th.md`: the option and how to load the theme,
  in Thai.

## How it is pinned

- `apps/client/.../composite/__tests__/bethereTheme.test.ts`: the `@import`
  comes first (or the browser drops it), the variables the views read are
  set, every colour is a valid hex.
- `apps/server-rs/tests/browser/obs_views.mjs`, new "BeThere theme" section:
  navy background, cream text, an option in the address beats the theme,
  `Inter, Anuphan` font stack, both fonts load from Google, timer box
  `rgba(18, 21, 42, 0.8)`. All 27 checks pass against a local Worker.
- `custom_views_tests` (8) still pass.
- Checked by hand in Chromium against a local Worker and client: the button
  fills the editor, "Save and enable" stores the CSS, and /timeline, /timer
  and /backstage turn navy with Inter and Anuphan, with no page errors.

## Deploy

Worker (the OBS pages are compiled in) and Pages (the client), as in plan 022.
Rooms that added the OBS views before this must press **Add OBS views** again
to get the new pages.

## Not done

- "Load example CSS" asks for `GET /data/assets/css/example`, which the
  Worker answers with 501 (only `/data/assets/css` is routed). That was
  already broken before this plan and is left for its own fix.
- The editor's own look (the operator's screens) is not themed. Only the views
  are.
