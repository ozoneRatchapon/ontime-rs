//! What an edit does to an event before it is stored, as upstream's rundown
//! service does it (RIIR 7, plan 015). Pinned by
//! `tests/rundown_edits_tests.rs` against upstream's own service.
//!
//! - `event_times`: upstream `createEventPatch` with `validateTimes` and
//!   `inferStrategy`. Sending only an end makes the event keep its end
//!   (`lock-end`) and works out the duration. Sending only a duration makes it
//!   keep its duration and works out the end.
//! - `cue_candidate`: upstream `getCueCandidate`, the cue a new event gets.
//! - `swap_schedules`: upstream `swapEventData`. Swapping two events exchanges
//!   what they are, while each place in the schedule keeps its times.

use serde_json::{json, Map, Value};

use crate::rundown::{entry_type, Rundown, MS_PER_DAY};

const LOCK_END: &str = "lock-end";
const LOCK_DURATION: &str = "lock-duration";

/// JavaScript truthiness of a patch field that may be absent.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// upstream `convertToInteger`: a number, or 0.
fn integer(value: Option<&Value>) -> i64 {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .map_or(0, |n| n.trunc() as i64)
}

/// `patch[key] ?? original[key]`.
fn either<'a>(patch: &'a Value, original: &'a Value, key: &str) -> Option<&'a Value> {
    patch
        .get(key)
        .filter(|value| !value.is_null())
        .or_else(|| original.get(key))
}

/// The times an event has after `patch`: start, end, duration and strategy,
/// each worked out from the others as upstream's `validateTimes` does.
pub fn event_times(original: &Value, patch: &Value) -> Map<String, Value> {
    let strategy = match patch.get("timeStrategy").and_then(Value::as_str) {
        Some(strategy @ (LOCK_END | LOCK_DURATION)) => strategy,
        _ => {
            // upstream inferStrategy: an end alone locks the end, a duration
            // alone locks the duration, anything else keeps what it was
            let fallback = original
                .get("timeStrategy")
                .and_then(Value::as_str)
                .unwrap_or(LOCK_DURATION);
            match (truthy(patch.get("timeEnd")), truthy(patch.get("duration"))) {
                (true, false) => LOCK_END,
                (false, true) => LOCK_DURATION,
                _ if fallback == LOCK_END => LOCK_END,
                _ => LOCK_DURATION,
            }
        }
    };
    let start = integer(either(patch, original, "timeStart"));
    let mut end = integer(either(patch, original, "timeEnd"));
    let mut duration = integer(either(patch, original, "duration"));
    match strategy {
        // calculateDuration: a duration across midnight is still positive
        LOCK_END => {
            duration = if end < start {
                end + MS_PER_DAY - start
            } else {
                end - start
            }
        }
        // calculateEnd
        _ => end = (start + duration) % MS_PER_DAY,
    }
    Map::from_iter([
        ("timeStart".to_string(), json!(start)),
        ("timeEnd".to_string(), json!(end)),
        ("duration".to_string(), json!(duration)),
        ("timeStrategy".to_string(), json!(strategy)),
    ])
}

/// Whether an edit touches the times at all; one that does not leaves them be.
pub fn touches_times(patch: &Value) -> bool {
    ["timeStart", "timeEnd", "duration", "timeStrategy"]
        .iter()
        .any(|key| patch.get(*key).is_some())
}

/// The cue for a new event at `id`, already placed in `rundown`: one more than
/// the event before it, or "1" (upstream `getCueCandidate` with `getIncrement`).
pub fn cue_candidate(rundown: &Rundown, id: &str) -> String {
    let had_entries = rundown.flat_order.iter().any(|entry| entry != id);
    // the entry before it among its siblings, or the group it is first in
    let previous = previous_sibling(rundown, id).or_else(|| rundown.parent_id(id));
    let Some(previous) = previous.filter(|_| had_entries) else {
        return "1".to_string();
    };
    let event = match rundown.get(&previous) {
        Some(entry) if entry_type(entry) == "event" => Some(entry),
        _ => previous_event_in_flat_order(rundown, &previous),
    };
    match event.and_then(|e| e.get("cue")).and_then(Value::as_str) {
        Some(cue) => increment(cue),
        None if event.is_some() => increment(""),
        None => "1".to_string(),
    }
}

fn previous_sibling(rundown: &Rundown, id: &str) -> Option<String> {
    let siblings: Vec<String> = match rundown.parent_id(id) {
        Some(group) => rundown
            .get(&group)
            .and_then(|g| g.get("entries"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        None => rundown.order.clone(),
    };
    let index = siblings.iter().position(|entry| entry == id)?;
    index.checked_sub(1).and_then(|i| siblings.get(i).cloned())
}

/// upstream `getPreviousEventNormal`: the nearest event before `id`.
fn previous_event_in_flat_order<'a>(rundown: &'a Rundown, id: &str) -> Option<&'a Value> {
    let index = rundown.flat_order.iter().position(|entry| entry == id)?;
    rundown.flat_order[..index]
        .iter()
        .rev()
        .filter_map(|entry| rundown.get(entry))
        .find(|entry| entry_type(entry) == "event")
}

/// upstream `getIncrement`: the number at the end goes up by one ("1.09" is
/// "1.10", "A-7" is "A-8", "007" is "8"); a cue that does not end in one
/// gets "-2" ("4A" is "4A-2").
pub fn increment(cue: &str) -> String {
    // ^(\D*?)(?:([ -]))?(\d+)(?:(\.)(\d+))?$
    let digits_from = cue
        .char_indices()
        .find(|(_, c)| c.is_ascii_digit())
        .map(|(i, _)| i);
    let Some(digits_from) = digits_from else {
        return format!("{cue}-2");
    };
    let (head, number) = cue.split_at(digits_from);
    let (integer, fraction) = match number.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (number, None),
    };
    let is_digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if !is_digits(integer) || fraction.is_some_and(|f| !is_digits(f)) {
        return format!("{cue}-2");
    }
    match fraction {
        None => {
            let next = integer.parse::<u128>().map_or(0, |n| n + 1);
            format!("{head}{next}")
        }
        Some(fraction) => {
            let next = fraction.parse::<u128>().map_or(0, |n| n + 1);
            format!("{head}{integer}.{next:0>width$}", width = fraction.len())
        }
    }
}

/// The fields that belong to a place in the schedule rather than to the event
/// in it, which a swap leaves where they are.
const SCHEDULE: &[&str] = &[
    "timeStart",
    "timeEnd",
    "duration",
    "linkStart",
    "parent",
    "delay",
    "gap",
    "dayOffset",
];

/// upstream `swapEventData`: `a` takes `b`'s content and keeps its own place.
pub fn swap_schedules(a: &Value, b: &Value) -> (Value, Value) {
    let take = |content: &Value, place: &Value| {
        let mut swapped = content.clone();
        if let Some(object) = swapped.as_object_mut() {
            for key in SCHEDULE {
                match place.get(*key) {
                    Some(value) => object.insert(key.to_string(), value.clone()),
                    None => object.remove(*key),
                };
            }
            if let Some(id) = place.get("id") {
                object.insert("id".into(), id.clone());
            }
            let revision = place.get("revision").and_then(Value::as_i64).unwrap_or(0);
            object.insert("revision".into(), json!(revision + 1));
        }
        swapped
    };
    (take(b, a), take(a, b))
}
