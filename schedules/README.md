# Schedules

One file per event. `scripts/seed-room.mjs` turns it into a room's rundown.

```sh
cp schedules/example.json schedules/<room>.json   # then edit it
node scripts/seed-room.mjs schedules/<room>.json --dry       # preview, writes nothing
node scripts/seed-room.mjs schedules/<room>.json             # create
node scripts/seed-room.mjs schedules/<room>.json --replace   # swap after editing
```

The master token is taken from the Keychain item `ontime-session-password`
(or `--token` / `ONTIME_TOKEN`). `--replace` refuses while the room's timer is
running unless `--live` is given.

Everything here except this README and `example.json` is git-ignored, because
real schedules carry people's names and student IDs.

## Fields

| key             | meaning                                                                                         |
| --------------- | ----------------------------------------------------------------------------------------------- |
| `room`          | room name (`A-Z a-z 0-9 - _`, max 64)                                                           |
| `owner`, `note` | optional — shown on the `/admin` room list                                                      |
| `project`       | `{ "title", "description" }` shown in the room                                                  |
| `rundown`       | rundown title; `--replace` swaps the one with this title                                        |
| `template`      | `linked` or `exam-slot` (below)                                                                 |
| `parts`         | `exam-slot` only: `[{ "label", "minutes", "colour" }]`, default 7-min presentation + 10-min Q&A |
| `fields`        | extra columns: `[{ "label", "colour" }]` — every `custom` key must be one of these labels       |
| `rows`          | `{ "start": "HH:MM", "end": "HH:MM", "title", "note"?, "cue"?, "colour"?, "custom"? }`          |
| `timezoneHours` | default `7` (Bangkok)                                                                           |

A `custom` value may be an array; it becomes one `a • b • c` line.

## Templates

- **`linked`**: one unbroken track. Only the first row keeps its clock time, and the
  rest follow it, so an overrun moves the rest of the day. Each row must start
  where the previous one ends.
- **`exam-slot`**: a fixed slot per row, split into `parts`. Each slot is pinned
  to its printed start, so one student overrunning never moves the next. The
  rest of the slot stays a gap for scoring and changeover.
