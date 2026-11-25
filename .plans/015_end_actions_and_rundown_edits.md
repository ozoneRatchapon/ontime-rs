# 015 — End actions and rundown edits against upstream (RIIR 7)

Status: **item 1 done (2026-11-25); item 2 in progress.**

## 1. End actions (done)

`gen-runtime_offset.ts` runs the end action as `runtime.service` does once a
played event finishes: `load-next` is `loadNext()`, and `play-next` is
`startNext(end)`, which starts the next event at the moment the last one
ended. Eight new cases cover:

- `load-next` and `play-next` at the end of an event;
- `play-next` when started late, with added time, and after a pause;
- `play-next` passing over a skipped event;
- both actions at the last event.

Six matched at once. The two that differ are both at the last event:

- **upstream** wraps to the first event. `play-next` plays it, so the show
  restarts on its own. `load-next` arms it.
- **ours** holds the last event at 00:00, paused.

**The user chose to keep ours (2026-11-25).** A show should not restart by
itself when it ends. Both cases are listed in `DELIBERATE_CASES` in
`runtime_offset_tests.rs`. The test also checks that upstream still wraps, so
the list cannot go stale. `end_action_on_the_last_event_holds_instead_of_looping`
pins our behaviour.

## 2. Rundown edits over REST (in progress)

What an operator does every day: add, delete, move, group and ungroup, clone,
and apply a delay. Also what an edit to a time does under `lock-end` and
`lock-duration`.

These are pinned today only by our own e2e (`rest.sh`). The plan:

1. An oracle over upstream's own rundown mutation code. It applies a script of
   edits to a rundown and records the processed rundown after each one: the
   order, and each entry's times, gap, delay, day and parent.
2. The same script through our `rundown_routes` / `Rundown` methods, compared
   field by field.
3. Fix each difference, or list it as deliberate with the reason. Ask the user
   wherever the choice affects a show.
