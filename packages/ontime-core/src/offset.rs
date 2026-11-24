//! How far the run is from the plan.
//!
//! This is the number an operator reads to decide whether to cut or stretch, so
//! it is worth being exact about. Ported from the original `getRuntimeOffset`
//! and `getExpectedStart`.
//!
//! Everything here is milliseconds. Times of day are milliseconds since local
//! midnight; durations are plain spans.

use serde::{Deserialize, Serialize};

use crate::rundown::MS_PER_DAY;

/// Which day a clock reading belongs to, relative to a planned time.
///
/// A rundown can cross midnight, so a planned start of 23:50 and a clock of
/// 00:05 are twelve minutes apart, not twenty-three hours. Anything more than
/// half a day apart is assumed to be the neighbouring day.
pub fn find_day_offset(planned_start: i64, clock: i64) -> i64 {
    let distance = clock - planned_start;
    if distance >= MS_PER_DAY / 2 {
        -1
    } else if distance < -(MS_PER_DAY / 2) {
        1
    } else {
        0
    }
}

/// Everything the offset depends on, so the calculation can be tested without a
/// rundown or a running server.
#[derive(Debug, Clone)]
pub struct OffsetInputs {
    /// Time of day the loaded event actually started.
    pub started_at: i64,
    /// Which day the run began on, relative to the rundown's planned start.
    pub start_day_offset: i64,
    /// The loaded event's planned start and day.
    pub event_time_start: i64,
    pub event_day_offset: i64,
    /// The timer's current value; negative means overtime.
    pub current: i64,
    pub added_time: i64,
    /// How long playback has been paused during this event.
    pub paused_for: i64,
    /// `countToEnd` events are measured only by how far past their end they run.
    pub count_to_end: bool,
    /// The rundown's planned start, and the time of day the run actually began.
    pub planned_start: i64,
    pub actual_start: i64,
}

/// Positive means behind schedule.
///
/// - `absolute` compares against the planned clock times
/// - `relative` compares against the plan as shifted by when the run actually
///   began, which is what you want when a show starts late by design
pub fn runtime_offset(inputs: &OffsetInputs) -> (i64, i64) {
    // how late the loaded event started against its planned slot
    let event_start_offset = inputs.started_at + inputs.start_day_offset * MS_PER_DAY
        - (inputs.event_time_start + inputs.event_day_offset * MS_PER_DAY);

    // running past the end adds to the debt; `current` is negative there
    let overtime = inputs.current.min(0).abs();

    // an event counting to a fixed end is not "late" for having started late:
    // only the overrun and the operator's added time count (upstream 57a4c495)
    let absolute = match inputs.count_to_end {
        true => overtime + inputs.added_time,
        false => event_start_offset + overtime + inputs.paused_for + inputs.added_time,
    };

    let relative = absolute + inputs.planned_start
        - inputs.actual_start
        - inputs.start_day_offset * MS_PER_DAY;

    (absolute, relative)
}

/// Which offset the operator asked to be measured against. Set from the editor
/// (`offsetmode`), kept for the room until it is changed again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OffsetMode {
    #[default]
    Absolute,
    Relative,
}

/// When an event that has not run yet is now expected to start.
#[derive(Debug, Clone)]
pub struct ExpectedStartInputs {
    pub time_start: i64,
    pub day_offset: i64,
    pub delay: i64,
    /// The day the running event is on.
    pub current_day: i64,
    /// Idle time between here and the running event, which can absorb lateness.
    pub total_gap: i64,
    /// Whether this event is in an unbroken chain of linked starts from the
    /// running one, in which case gaps cannot help.
    pub is_linked_to_loaded: bool,
    pub offset: i64,
    pub mode: OffsetMode,
    pub actual_start: Option<i64>,
    pub planned_start: Option<i64>,
}

pub fn expected_start(inputs: &ExpectedStartInputs) -> i64 {
    let relative_day_offset = inputs.day_offset - inputs.current_day;
    let delayed_start = (inputs.time_start + inputs.delay).max(0);
    let normalised_time_start = delayed_start + relative_day_offset * MS_PER_DAY;

    let relative_start_offset = match inputs.mode {
        OffsetMode::Relative => {
            inputs.actual_start.unwrap_or(0) + inputs.current_day * MS_PER_DAY
                - inputs.planned_start.unwrap_or(0)
        }
        OffsetMode::Absolute => 0,
    };

    let scheduled = normalised_time_start + relative_start_offset;
    let with_offset = scheduled + inputs.offset;

    // a linked chain has no slack, so it simply moves with the offset
    if inputs.is_linked_to_loaded {
        return with_offset;
    }

    // if the idle time ahead is more than the lateness, the plan still holds
    if inputs.total_gap > inputs.offset {
        return scheduled;
    }

    // otherwise the gaps absorb what they can
    with_offset - inputs.total_gap
}

/// The parts of an event that decide when it ends.
#[derive(Debug, Clone)]
pub struct ExpectedEndInputs {
    pub time_start: i64,
    pub day_offset: i64,
    pub duration: i64,
    pub count_to_end: bool,
}

/// When an event is expected to end, given when it is expected to start.
///
/// Ported from upstream `getExpectedEnd`. A `countToEnd` event ends on the wall
/// clock at its planned end, or at its start if it starts after that.
pub fn expected_end(event: &ExpectedEndInputs, expected_start: i64, current_day: i64) -> i64 {
    match event.count_to_end {
        true => {
            let relative_day_offset = event.day_offset - current_day;
            let planned_end = event.time_start + event.duration + relative_day_offset * MS_PER_DAY;
            expected_start.max(planned_end)
        }
        false => expected_start + event.duration,
    }
}
