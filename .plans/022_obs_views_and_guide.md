# 022 — OBS pages and a guide for MacBook + iPhone

Status: **done and deployed (2026-11-26), Worker `82a3fda0`, Pages `9e03647b` (master) and `9220b816` (main), hash-checked.** Chosen by the user after plan 021,
as the first step towards running live events as a service: an online event
with only a MacBook, an iPhone and OBS.

## What it does

- **Settings → Custom views → Add OBS views** (Worker only) adds two pages to
  the room, or puts them back as shipped:
  - `obs-countdown`: a "starting soon" screen. It counts down to `?to=HH:MM`,
    or to the first event's planned start. Title (from Project unless
    `?title=`), label, done text, logo, colours and a Google Font all come
    from the address.
  - `obs-timer`: the running event's timer, see-through, for a corner over the
    camera. It hides when stopped, dims when paused, and goes negative and red
    when over time. Warning/danger colours follow the event's own times.
    Corner, size, title, box, colours and font come from the address.
- Both count by themselves from the room's last message, since the Worker
  sends nothing while a room is idle. They keep working if the heartbeat is
  slowed later (plan 017 step 2).
- `POST /data/custom-views/restore-obs` (ours, like upstream's
  `restore-demo`).
- The page policy also allows `https://fonts.googleapis.com` for styles, so a
  theme can use a Thai font (Prompt, Kanit, Sarabun).
- `.docs/004_obs_macbook_iphone_th.md`: the guide in Thai. Covers Continuity
  Camera, the two scenes, every address option, stream settings, a pre-live
  checklist and troubleshooting.
- License claim in `package.json` (root, cli, electron) is now
  `GPL-3.0-or-later`, as `LICENSE.md` and upstream `dfa6147c1`.

## How it is pinned

- `tests/custom_views_tests.rs`: both pages pass upstream's checks and open
  the socket as a page for the app does; the policy allows Google Fonts.
- `tests/e2e/custom_views.mjs`: restore-obs, listing, putting back, serving.
- `tests/browser/obs_views.mjs` in Chromium, under a locked room (in the
  browser `run.sh`):
  - the countdown counts to the time in the address and keeps counting while
    the room sends nothing; title through the room; Thai label; accent
    colour; see-through; first event named;
  - the timer is hidden when idle, in the corner asked for, shows and counts
    the running event with its title, stands still and dims when paused,
    shows `-` and the danger colour over time, and hides once stopped;
  - no page errors and nothing refused by the policy, Google Font included.
- Screenshots checked by eye: the Thai label's letter-spacing was removed, as
  it split Thai syllables.

## Next

- Theme presets, from what customers ask to change on these pages.
- Companion against a room (item 4 of the comparison).
