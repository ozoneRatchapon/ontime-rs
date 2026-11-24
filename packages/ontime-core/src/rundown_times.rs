//! The timing pass over a rundown: each event's linked start, gap, delay and
//! day, and each group's span. Ported from upstream's server
//! (`rundown.parser.ts` processEntry with `getLinkedTimes`, `calculateDayOffset`,
//! `getTimeFrom` and `isNewLatest`, and `rundown.dao.ts` processRundown for
//! groups), pinned by `tests/fixtures/rundown_process.json`, which is generated
//! by running that code.
//!
//! One deliberate difference: an unlinked event's end or duration is derived
//! from its `timeStrategy` here too. Upstream relies on every edit having been
//! validated already; deriving keeps a stored rundown with inconsistent times
//! from reaching the screens.

use serde_json::{json, Value};

use crate::rundown::{entry_type, Rundown, MS_PER_DAY};

/// What the pass remembers of an event: its start, duration, end and day.
#[derive(Debug, Clone, Copy)]
struct Timed {
    start: i64,
    duration: i64,
    end: i64,
    day: i64,
}

impl Timed {
    /// The end on a running clock across days, as upstream normalises it.
    fn normalised_end(&self) -> i64 {
        self.start + self.duration + self.day * MS_PER_DAY
    }
}

fn wrap_day(value: i64) -> i64 {
    value.rem_euclid(MS_PER_DAY)
}

/// A duration across midnight is still positive (upstream `calculateDuration`).
fn duration_between(start: i64, end: i64) -> i64 {
    match end < start {
        true => end + MS_PER_DAY - start,
        false => end - start,
    }
}

/// Upstream `calculateDayOffset`: whether an event starts on the day after the
/// previous playable one.
fn day_offset(start: i64, previous: Option<&Timed>) -> i64 {
    let Some(previous) = previous else { return 0 };
    if previous.duration == 0 {
        return 0;
    }
    // the previous event crossed midnight, or this one starts no later than it did
    match previous.start + previous.duration >= MS_PER_DAY || start <= previous.start {
        true => 1,
        false => 0,
    }
}

/// Upstream `getTimeFrom`: the gap from the latest-ending event, negative for an overlap.
fn time_from(start: i64, day: i64, latest: Option<&Timed>) -> i64 {
    latest.map_or(0, |latest| {
        start + day * MS_PER_DAY - latest.normalised_end()
    })
}

fn int(entry: &Value, key: &str) -> i64 {
    entry.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn flag(entry: &Value, key: &str) -> bool {
    entry.get(key).and_then(Value::as_bool).unwrap_or(false)
}

impl Rundown {
    /// Walks the running order, as upstream's processEntry does.
    pub(crate) fn recompute_times(&mut self) {
        let mut total_delay: i64 = 0;
        let mut total_days: i64 = 0;
        let mut previous_event: Option<Timed> = None;
        let mut latest_event: Option<Timed> = None;
        // the entry processed just before, whatever it was: a delay right before
        // a gap is kept apart from the delay the gap absorbs
        let mut previous_delay: Option<i64> = None;

        for id in self.flat_order.clone() {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            match entry_type(entry) {
                "delay" => {
                    let duration = int(entry, "duration");
                    total_delay += duration;
                    previous_delay = Some(duration);
                    continue;
                }
                "event" => {}
                _ => {
                    previous_delay = None;
                    continue;
                }
            }

            let lock_end = entry.get("timeStrategy").and_then(Value::as_str) == Some("lock-end");
            let mut start = int(entry, "timeStart");
            let mut duration = int(entry, "duration").max(0);
            let mut end = entry
                .get("timeEnd")
                .and_then(Value::as_i64)
                .unwrap_or(start + duration);
            let mut link_start = flag(entry, "linkStart");

            // 1. a link follows the previous playable event, or is dropped (getLinkedTimes)
            match (link_start, previous_event) {
                (true, Some(previous)) => {
                    start = previous.end;
                    match lock_end {
                        true => duration = duration_between(start, end),
                        false => end = wrap_day(start + duration),
                    }
                }
                (true, None) => link_start = false,
                // not linked: derive what the strategy does not keep (see the module note)
                (false, _) => match lock_end {
                    true => duration = duration_between(start, end),
                    false => end = wrap_day(start + duration),
                },
            }

            // skipped events get no day of their own, no gap and no delay
            let mut day = previous_event.map_or(0, |previous| previous.day);
            let mut gap = 0;
            let mut delay = 0;
            let playable = !flag(entry, "skip");
            if playable {
                total_days += day_offset(start, previous_event.as_ref());
                day = total_days;
                gap = time_from(start, day, latest_event.as_ref());

                // a gap absorbs accumulated delay, but not a delay placed right before it
                if total_delay > 0 && gap > 0 {
                    let kept = previous_delay.unwrap_or(0);
                    total_delay = (total_delay - kept - gap).max(0) + kept;
                }
                delay = total_delay;
            }

            if let Some(object) = entry.as_object_mut() {
                object.insert("timeStart".into(), json!(wrap_day(start)));
                object.insert("timeEnd".into(), json!(wrap_day(end)));
                object.insert("duration".into(), json!(duration));
                object.insert("linkStart".into(), json!(link_start));
                object.insert("gap".into(), json!(gap));
                object.insert("delay".into(), json!(delay));
                object.insert("dayOffset".into(), json!(day));
            }

            if playable {
                let timed = Timed {
                    start: wrap_day(start),
                    duration,
                    end: wrap_day(end),
                    day,
                };
                previous_event = Some(timed);
                // the latest is the event ending furthest forwards (isNewLatest)
                if latest_event
                    .is_none_or(|latest| timed.normalised_end() >= latest.normalised_end())
                {
                    latest_event = Some(timed);
                }
            }
            previous_delay = None;
        }
    }

    /// How long the rundown runs from its first start: every playable event's
    /// duration with the gap before it, less any overlap (processEntry's
    /// `totalDuration`). Reads the values [`Rundown::recompute`] stored.
    pub fn total_duration(&self) -> i64 {
        self.flat_order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|entry| entry_type(entry) == "event" && !flag(entry, "skip"))
            .map(|entry| {
                let (gap, duration) = (int(entry, "gap"), int(entry, "duration"));
                match gap >= 0 {
                    true => gap + duration,
                    // an overlap takes back what it overlaps, never below nothing
                    false => (duration + gap).max(0),
                }
            })
            .sum()
    }

    /// Groups span their playable events: the first one's start, the last one's
    /// end, and their durations with the gaps between them (processRundown).
    pub(crate) fn recompute_groups(&mut self) {
        let group_ids: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry_type(entry) == "group")
            .map(|(id, _)| id.clone())
            .collect();

        for group_id in group_ids {
            let children: Vec<String> = self
                .entries
                .get(&group_id)
                .and_then(|group| group.get("entries"))
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            let mut start: Option<i64> = None;
            let mut end: Option<i64> = None;
            let mut duration: i64 = 0;
            let mut is_first_linked = false;
            for (position, child) in children.iter().enumerate() {
                let Some(entry) = self.entries.get(child) else {
                    continue;
                };
                if entry_type(entry) != "event" || flag(entry, "skip") {
                    continue;
                }
                if start.is_none() {
                    start = Some(int(entry, "timeStart"));
                    is_first_linked = flag(entry, "linkStart");
                }
                end = Some(int(entry, "timeEnd"));
                // upstream counts position among all the group's entries, not only events
                if position > 0 {
                    duration += int(entry, "gap");
                }
                duration += int(entry, "duration");
            }

            if let Some(object) = self
                .entries
                .get_mut(&group_id)
                .and_then(Value::as_object_mut)
            {
                object.insert("timeStart".into(), start.map_or(Value::Null, |v| json!(v)));
                object.insert("timeEnd".into(), end.map_or(Value::Null, |v| json!(v)));
                object.insert("duration".into(), json!(duration));
                object.insert("isFirstLinked".into(), json!(is_first_linked));
            }
        }
    }
}
