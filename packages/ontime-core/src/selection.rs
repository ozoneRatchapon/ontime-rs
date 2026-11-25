//! Which event a playback command picks: next, previous, by position, by cue.
//!
//! Ported from upstream `runtime.utils.ts` (findNextPlayableId,
//! findPreviousPlayableId, findNextPlayableWithCue, getEventAtIndex,
//! findNextRollEvent) and pinned to it by `tests/selection_tests.rs` against
//! fixtures generated from upstream's own code.
//!
//! Two orders matter, as upstream:
//! - **timed**: every event, skipped ones included. The editor numbers events
//!   by it, so an operator's "event 3" is `timed[2]`.
//! - **playable**: the events playback can run, which leaves out skipped ones.

use serde_json::Value;

use crate::rundown::Rundown;

/// Ids of the events that playback can select: real events, in order, not skipped.
pub fn playable(rundown: &Rundown) -> Vec<String> {
    events_where(rundown, is_playable)
}

/// Ids of every event in order, skipped ones included (upstream's
/// `timedEventOrder`), which is what the client numbers events by.
pub fn timed(rundown: &Rundown) -> Vec<String> {
    events_where(rundown, |_| true)
}

fn is_playable(entry: &Value) -> bool {
    entry.get("skip").and_then(Value::as_bool) != Some(true)
}

fn is_event(entry: &Value) -> bool {
    entry.get("type").and_then(Value::as_str) == Some("event")
}

fn events_where(rundown: &Rundown, keep: impl Fn(&Value) -> bool) -> Vec<String> {
    rundown
        .flat_order
        .iter()
        .filter(|id| {
            rundown
                .get(id)
                .is_some_and(|entry| is_event(entry) && keep(entry))
        })
        .cloned()
        .collect()
}

/// Whether `id` names an event that can be loaded (upstream loadEvent's
/// checks): an event, not skipped, and part of the rundown's order.
pub fn is_loadable(rundown: &Rundown, id: &str) -> bool {
    rundown
        .get(id)
        .is_some_and(|entry| is_event(entry) && is_playable(entry))
        && rundown.flat_order.iter().any(|entry| entry == id)
}

/// The event after `current`, wrapping to the first after the last; the first
/// when nothing, or something no longer playable, is loaded.
pub fn next_playable<'a>(playable: &'a [String], current: Option<&str>) -> Option<&'a String> {
    let first = playable.first()?;
    let Some(current) = current else {
        return Some(first);
    };
    match playable.iter().position(|id| id == current) {
        Some(index) if index + 1 < playable.len() => playable.get(index + 1),
        _ => Some(first),
    }
}

/// The event before `current`; the first when `current` is the first, is not
/// playable, or nothing is loaded. It never wraps to the end.
pub fn previous_playable<'a>(playable: &'a [String], current: Option<&str>) -> Option<&'a String> {
    let first = playable.first()?;
    let Some(current) = current else {
        return Some(first);
    };
    match playable.iter().position(|id| id == current) {
        Some(index) if index >= 1 => playable.get(index - 1),
        _ => Some(first),
    }
}

/// The operator's `index`th event, counted in timed order from 0. It may be a
/// skipped one, which then fails to load, as upstream.
pub fn event_at_index(timed: &[String], index: usize) -> Option<&String> {
    timed.get(index)
}

/// The next playable event with `cue`, searching from after the loaded event
/// (or from the loaded event itself with `allow_current`) and then from the
/// top. `current_index` is a position in timed order, as upstream passes
/// `selectedEventIndex` whichever order it searches.
pub fn next_with_cue(
    rundown: &Rundown,
    order: &[String],
    cue: &str,
    current_index: Option<usize>,
    allow_current: bool,
) -> Option<String> {
    let current = current_index.unwrap_or(0);
    let from = match allow_current {
        true => current,
        false => current + 1,
    };
    let matches = |id: &&String| {
        rundown.get(id).is_some_and(|entry| {
            is_event(entry)
                && is_playable(entry)
                && entry.get("cue").and_then(Value::as_str) == Some(cue)
        })
    };
    let from = from.min(order.len());
    order[from..]
        .iter()
        .find(matches)
        .or_else(|| order[..from].iter().find(matches))
        .cloned()
}

/// The event roll moves to after `from`, wrapping to the top after the last.
/// Zero-length events are passed over, or roll would stick on them
/// (upstream findNextRollEvent).
pub fn next_roll_event(rundown: &Rundown, from: Option<&str>) -> Option<String> {
    let events = playable(rundown);
    let first = from
        .and_then(|id| events.iter().position(|e| e == id))
        .map_or(0, |index| index + 1);
    (0..events.len())
        .map(|step| &events[(first + step) % events.len()])
        .find(|id| {
            rundown
                .get(id)
                .and_then(|e| e.get("duration"))
                .and_then(Value::as_i64)
                .unwrap_or(0)
                != 0
        })
        .cloned()
}
