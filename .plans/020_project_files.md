# 020 — Project files to and from the Ontime app

Status: **done, not deployed (2026-11-25).** Item 1 of the gaps found comparing
the rooms with Ontime Cloud's paid plans (PRO, STUDIO): their "Project files
move between Cloud and local installs". Until now the Worker hid the Project
panel, so a show could not leave a room for the app or come in from it.

## What it does

- `POST /r/<room>/data/db/import` with `{ "file": "<the file's text>" }` puts
  a project file from the app in place of the room's show. Answer:
  `{ imported: true, warnings: [...] }`, what was dropped or defaulted, each
  once. A 400 and nothing changed for a file that is not JSON, has no
  `settings.version`, is from Ontime v3, or has a rundown without
  `order`/`entries`.
- `POST /r/<room>/data/db/export` gives the room as a `DatabaseModel`, the file
  the app opens: project id, rundowns, project, settings, view settings, URL
  presets, custom fields, automation, teleprompter. Nothing of the room's own:
  no playback, report, password or activity log.
- The room keeps what is the room's rather than the show's: its token and
  members-only flag, activity log, report archive, CSS, time zone and
  heartbeat. Playback stops; the teleprompter pauses.
- An import is copied into the undo history first (like a restore) and logged
  as `project-import`.
- The web editor, on the Worker, shows **Settings → Project** again, with one
  card: Import (with a confirm) and Download. "Create" stays hidden.
- `download`/`restore` are unchanged: they are the room's own backup format
  (with the report archive), used by `scripts/backup-rooms.mjs`.

## How it is pinned

- `scripts/upstream-oracle/gen-project_file.ts` runs upstream's own
  `parseDatabaseModel` over 12 files: the demo project, files with every
  section, invalid custom fields, presets, project custom data, automation and
  teleprompter values, groups with missing references, a legacy `block`,
  unknown entry types, several rundowns, none at all, and two that upstream
  refuses. Fixture: `packages/ontime-core/tests/fixtures/project_file.json`.
- `tests/project_file_tests.rs`: each file imported then exported equals
  upstream's parse (the version, and ids upstream makes at random, aside);
  an export imports back unchanged; the room keeps its own; refusals change
  nothing; each warning once. Checked that it fails when a default is off.
- `apps/server-rs/tests/e2e/project_file.mjs` on a local worker, in `run.sh`.
- By hand, locally only: both project files in the desktop app's folder import
  with no warnings; the three shows in the nightly backup, exported, read
  through upstream's parser with no errors. Upstream changes only what it
  works out again on load (revision, gap, dayOffset, a group's times).

## Deliberate differences

- A v3 file is refused with "open it in the Ontime app and save it", where
  upstream migrates it. v3 is from 2023 and the app migrates it on opening.
- Values of the wrong type in settings or project details (a number as a
  title) are defaulted; upstream keeps them as they are.
- A room never imported makes a new project id on each export (export changes
  nothing); after an import it keeps the file's.
- The export lists rundowns by id, not in the room's order, since the core's
  JSON does not keep key order. The app loads the first; switch in the app.

## Found on the way (not fixed)

`raknong_4ever` (from the nightly backup, read only): one event is in a
rundown's top-level order but names a group as its parent, and that group does
not list it. Some edit path leaves a stale `parent`. Upstream's parser makes it
a top-level event, which is where the order has it, so the export is right.
Worth finding which edit leaves it, in a throwaway room.

## Next from the comparison

2. Spreadsheet import on the Worker (calamine is already in the core).
3. Automations over HTTP from the Worker (no OSC: a Worker has no UDP).
4. Try Companion against a room.
5. More rooms at once on the free tier: plan 017 step 2 (slower heartbeat).
