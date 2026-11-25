//! Roll: which event the wall clock says should be on air (upstream loadRoll,
//! checkIsNow). Split from `runtime.rs`, which carries the commands out.

use crate::rundown::{Rundown, MS_PER_DAY};
use crate::runtime::field;
use crate::selection::playable;

/// Which event the wall clock says should be on air, and how far into it we are.
pub struct RollTarget {
    pub id: String,
    pub time_start: i64,
    /// Milliseconds already elapsed into the event; zero when pending.
    pub elapsed: i64,
    /// True when the event has not started yet and we are counting down to it.
    pub is_pending: bool,
}

/// An event whose end reads earlier than its start runs through midnight.
fn normalise_end(start: i64, end: i64) -> i64 {
    if end < start {
        end + MS_PER_DAY
    } else {
        end
    }
}

/// Whether the clock is inside an event, its end included (upstream checkIsNow).
pub(crate) fn is_now(start: i64, end: i64, clock: i64) -> bool {
    match end < start {
        true => clock >= start || clock <= end,
        false => start <= clock && clock <= end,
    }
}

/// An event due earlier today than the current clock is due tomorrow.
pub(crate) fn normalise_roll_start(start: i64, clock: i64) -> i64 {
    if start < clock {
        start + MS_PER_DAY
    } else {
        start
    }
}

/// Finds the event a rolling rundown should be playing at `clock`.
///
/// Walks the playable events in order, keeping count of how many times the
/// rundown has gone past midnight, and returns the first one that has not
/// already finished. Zero-duration events are skipped: rolling into one would
/// leave playback stuck on it forever.
pub fn load_roll(rundown: &Rundown, clock: i64) -> Option<RollTarget> {
    let playable = playable(rundown);
    let first = playable.first()?;

    let mut day_span = 0;

    for id in &playable {
        let Some(event) = rundown.get(id) else {
            continue;
        };
        let duration = field(event, "duration").unwrap_or(0);
        if duration == 0 {
            continue;
        }

        let time_start = field(event, "timeStart").unwrap_or(0);
        let time_end = field(event, "timeEnd").unwrap_or(0);

        let corrected_start = time_start + MS_PER_DAY * day_span;
        let corrected_end = time_end + MS_PER_DAY * day_span;
        let normal_end = normalise_end(corrected_start, corrected_end);

        // already over
        if normal_end <= clock {
            if time_start > time_end {
                day_span += 1;
            }
            continue;
        }

        // under way — either normally, or carried over from the day before
        let is_from_day_before = normal_end > MS_PER_DAY && clock < time_end;
        if is_from_day_before || clock >= time_start {
            let elapsed = if is_from_day_before {
                clock + MS_PER_DAY - time_start
            } else {
                clock - time_start
            };
            return Some(RollTarget {
                id: id.clone(),
                time_start,
                elapsed,
                is_pending: false,
            });
        }

        // an overnight event later in the rundown may be on air right now
        for later_id in playable.iter().skip_while(|x| *x != id).skip(1) {
            let Some(later) = rundown.get(later_id) else {
                continue;
            };
            if field(later, "duration").unwrap_or(0) == 0 {
                continue;
            }
            let later_start = field(later, "timeStart").unwrap_or(0);
            let later_end = field(later, "timeEnd").unwrap_or(0);
            if later_start <= later_end {
                continue;
            }
            // it wraps midnight, so it is running if we are past its start or
            // before its end
            if clock >= later_start || clock < later_end {
                let elapsed = if clock >= later_start {
                    clock - later_start
                } else {
                    clock + MS_PER_DAY - later_start
                };
                return Some(RollTarget {
                    id: later_id.clone(),
                    time_start: later_start,
                    elapsed,
                    is_pending: false,
                });
            }
        }

        // still to come
        return Some(RollTarget {
            id: id.clone(),
            time_start,
            elapsed: 0,
            is_pending: true,
        });
    }

    // everything is behind us: arm the top of the rundown for the next day
    let event = rundown.get(first)?;
    Some(RollTarget {
        id: first.clone(),
        time_start: field(event, "timeStart").unwrap_or(0),
        elapsed: 0,
        is_pending: true,
    })
}
