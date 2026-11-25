# 018 — Free backups (free-tier work, item 3)

Status: **done locally (2026-11-25).** Two layers, neither costs anything.

## 1. A copy of every room on the user's Mac, nightly

- `scripts/backup-rooms.mjs` saves `POST /data/db/download` for every room on
  the admin list to `~/OntimeBackups/<date>/<room>.json` (mode 600), and keeps
  30 days. The folder is outside the repository because these are real events
  and the repository's origin is public.
- `scripts/install-backup-agent.sh` installs a launchd agent of this user:
  every night at 03:00, or on wake if the Mac was asleep. `--remove` takes it
  out again; the backups stay.
- **Installed on 2026-11-25.** The first run, under launchd, saved all 4 rooms
  (exit 0).
- **Restore checked:** a backup of `default_room` restored into a throwaway
  room through `/data/db/restore` gave back the same rundown, entry for entry.
  The test files in /tmp, which held real data, were deleted straight after.
- Cost: one request for the list and one per room, each night.

## 2. The room's own undo history (Worker)

- Before a write, the Durable Object copies the project as it was stored,
  into its own storage: before every change that removes things (a delete,
  a whole rundown replaced, a backup restored), and every ten minutes of
  ordinary edits. It keeps 72 copies (about twelve hours of ordinary edits).
  It never copies the same state twice, and a copy that fails never blocks
  the write.
- `GET /data/snapshots` lists them, newest first. `POST
/data/snapshots/<at>/restore` puts one back, after copying the state it
  replaces, so an undo can be undone. Both are for an owner or the admin.
  A restore is written to the activity log.
- The policy is `ontime_core::snapshots` (5 tests). The host is
  `apps/server-rs/src/room_snapshots.rs`. Restoring uses the same
  `api::restore_into` as a backup file: the room keeps its password, its log
  and its teleprompter playback, and playback starts stopped.
- `tests/e2e/snapshots.mjs` (needs ADMIN_PASSWORD): an empty rundown sent by
  mistake, undone back to the exact three events, and that undo undone.
- Cost: two row writes per copy, out of 100,000 a day; storage about 5 MB for
  a large room, out of 5 GB.
- The native server has no undo history yet (404); its project is a file on
  disk, which a normal file backup already covers.

There is no button for it in the editor yet. An undo is an API call, or a
small page later if it gets used.
