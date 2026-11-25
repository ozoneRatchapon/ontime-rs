# 015 — End actions and rundown edits against upstream (RIIR 7)

Status: **both items done locally (2026-11-25).**

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

## 2. Rundown edits over REST (done)

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

### Result

`gen-rundown_edits.ts` drives upstream's real `rundown.service` with in-memory
storage (`rundown-stubs/`): 54 cases, each a script of edits. After every edit
it records the processed rundown. `rundown_edits_tests.rs` sends the same edits
through our REST routes and compares the order and each entry's fields.

The first run found four real bugs:

1. **New events had no cue.** Upstream gives one: the cue before it, plus one
   (`getCueCandidate` and `getIncrement`). Now ported: `"2"` gives `"3"`,
   `"1.09"` gives `"1.10"`, `"A-7"` gives `"A-8"`, `"4A"` gives `"4A-2"`, and
   `""` gives `"-2"`, all checked against upstream. A cue sent with the event
   is kept.
2. **An edited end or duration was overwritten.** Upstream infers the
   strategy from what was sent (`createEventPatch`, `validateTimes`,
   `inferStrategy`). An end alone locks the end and works out the duration; a
   duration alone does the opposite. Ours kept the old strategy and
   recomputed over the value just typed.
3. **Swap moved the schedule too.** Upstream (`swapEventData`) exchanges what
   two events are, while each place keeps its times, link, group, delay, gap
   and day. Ours swapped everything but the id, including the group.
4. **Cloning a group shared its entries.** The copy pointed at the
   original's children, so the same events sat in two groups and twice in the
   order. Now each child is copied with a new id, and the copy is titled
   "… (copy)", as upstream does.

The new behaviour lives in `entry_edits.rs`. It applies to edits from the
editor and the integration API. Import and replace (`Rundown::place_entry`)
keep entries exactly as given, since upstream's import does not invent cues
either.

The comparison treats a missing field and `""` or `false` as the same. The
fixture's starting events are upstream's partial mocks, which only fill in on
the first edit. A missing number never counts as 0.

ontime-core: 367 tests pass. e2e: every suite passes on wrangler and on the
native server.
