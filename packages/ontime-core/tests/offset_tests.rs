use ontime_core::offset::{
    expected_start, find_day_offset, runtime_offset, ExpectedStartInputs, OffsetInputs, OffsetMode,
};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// A run that started exactly on time at 10:00, on its planned day.
fn on_time() -> OffsetInputs {
    OffsetInputs {
        started_at: 10 * HOUR,
        start_day_offset: 0,
        started_day: 0,
        event_time_start: 10 * HOUR,
        event_day_offset: 0,
        current: 5 * MINUTE,
        added_time: 0,
        paused_for: 0,
        count_to_end: false,
        planned_start: 10 * HOUR,
        actual_start: 10 * HOUR,
    }
}

#[test]
fn a_run_exactly_on_schedule_has_no_offset() {
    let (absolute, relative) = runtime_offset(&on_time());
    assert_eq!(absolute, 0);
    assert_eq!(relative, 0);
}

#[test]
fn starting_late_puts_the_run_behind() {
    let inputs = OffsetInputs {
        started_at: 10 * HOUR + 3 * MINUTE,
        ..on_time()
    };
    // behind schedule reads positive
    assert_eq!(runtime_offset(&inputs).0, 3 * MINUTE);
}

#[test]
fn starting_early_puts_the_run_ahead() {
    let inputs = OffsetInputs {
        started_at: 10 * HOUR - 90_000,
        ..on_time()
    };
    assert_eq!(runtime_offset(&inputs).0, -90_000);
}

#[test]
fn running_over_adds_to_the_offset() {
    // the timer is 2 minutes past zero
    let inputs = OffsetInputs {
        current: -2 * MINUTE,
        ..on_time()
    };
    assert_eq!(runtime_offset(&inputs).0, 2 * MINUTE);
}

#[test]
fn time_remaining_does_not_make_the_run_early() {
    // an event still running to plan is not "ahead" just because time is left
    let inputs = OffsetInputs {
        current: 20 * MINUTE,
        ..on_time()
    };
    assert_eq!(runtime_offset(&inputs).0, 0);
}

#[test]
fn a_pause_counts_against_the_schedule_while_it_lasts() {
    let inputs = OffsetInputs {
        paused_for: 45_000,
        ..on_time()
    };
    assert_eq!(runtime_offset(&inputs).0, 45_000);
}

#[test]
fn added_time_counts_against_the_schedule() {
    let inputs = OffsetInputs {
        added_time: 5 * MINUTE,
        ..on_time()
    };
    assert_eq!(runtime_offset(&inputs).0, 5 * MINUTE);
}

#[test]
fn lateness_overtime_pause_and_added_time_accumulate() {
    let inputs = OffsetInputs {
        started_at: 10 * HOUR + MINUTE,
        current: -30_000,
        paused_for: 15_000,
        added_time: 2 * MINUTE,
        ..on_time()
    };
    assert_eq!(
        runtime_offset(&inputs).0,
        MINUTE + 30_000 + 15_000 + 2 * MINUTE
    );
}

#[test]
fn count_to_end_only_counts_the_overrun() {
    // an event pinned to a wall-clock end is not late for starting late
    let inputs = OffsetInputs {
        started_at: 10 * HOUR + 10 * MINUTE,
        current: -MINUTE,
        count_to_end: true,
        ..on_time()
    };
    assert_eq!(runtime_offset(&inputs).0, MINUTE);
}

#[test]
fn relative_offset_forgives_a_deliberately_late_start() {
    // the show was always going to start 30 minutes late; from that point on it
    // has run exactly to plan, so relative is zero while absolute is not
    let inputs = OffsetInputs {
        started_at: 10 * HOUR + 30 * MINUTE,
        actual_start: 10 * HOUR + 30 * MINUTE,
        ..on_time()
    };
    let (absolute, relative) = runtime_offset(&inputs);
    assert_eq!(absolute, 30 * MINUTE);
    assert_eq!(relative, 0);
}

#[test]
fn relative_offset_still_reports_drift_after_a_late_start() {
    // started 30 min late by design, then lost a further 5 minutes
    let inputs = OffsetInputs {
        started_at: 10 * HOUR + 35 * MINUTE,
        actual_start: 10 * HOUR + 30 * MINUTE,
        ..on_time()
    };
    let (absolute, relative) = runtime_offset(&inputs);
    assert_eq!(absolute, 35 * MINUTE);
    assert_eq!(relative, 5 * MINUTE);
}

#[test]
fn an_event_planned_for_the_next_day_is_not_a_day_late() {
    // the run began on day 0; its event planned for 00:30 on day 1 starts then
    let inputs = OffsetInputs {
        started_at: 30 * MINUTE,
        start_day_offset: 0,
        started_day: 1,
        event_time_start: 30 * MINUTE,
        event_day_offset: 1,
        ..on_time()
    };
    // measured from the day it started, not the run's first day (where
    // upstream reads it as a whole day early)
    assert_eq!(runtime_offset(&inputs).0, 0);

    // started at 00:30 but still on day 0, it really is a day early
    let inputs = OffsetInputs {
        started_day: 0,
        ..inputs
    };
    assert_eq!(runtime_offset(&inputs).0, -DAY);
}

// --- which day a clock reading belongs to ----------------------------------

#[test]
fn day_offset_is_zero_for_times_close_together() {
    assert_eq!(find_day_offset(10 * HOUR, 10 * HOUR), 0);
    assert_eq!(find_day_offset(10 * HOUR, 10 * HOUR + 5 * MINUTE), 0);
    assert_eq!(find_day_offset(10 * HOUR, 9 * HOUR), 0);
}

#[test]
fn a_clock_that_has_wrapped_past_midnight_is_a_day_ahead_of_the_plan() {
    // planned 23:50, clock reads 00:05: fifteen minutes later, not a day earlier
    assert_eq!(find_day_offset(23 * HOUR + 50 * MINUTE, 5 * MINUTE), 1);
}

#[test]
fn a_clock_before_a_plan_that_wraps_is_a_day_behind_it() {
    // planned 00:30, clock reads 23:00 the evening before
    assert_eq!(find_day_offset(30 * MINUTE, 23 * HOUR), -1);
}

#[test]
fn the_day_boundary_sits_at_twelve_hours_and_is_asymmetric() {
    // exactly half a day ahead already counts as the previous day...
    assert_eq!(find_day_offset(0, 12 * HOUR - 1), 0);
    assert_eq!(find_day_offset(0, 12 * HOUR), -1);

    // ...but exactly half a day behind does not yet count as the next one.
    // The asymmetry is inherited from the original (`>= 12h` against `< -12h`)
    // and is pinned here so a later tidy-up does not silently change it.
    assert_eq!(find_day_offset(12 * HOUR, 0), 0);
    assert_eq!(find_day_offset(12 * HOUR, -1), 1);
}

// --- when an upcoming event is now expected to start -----------------------

fn upcoming() -> ExpectedStartInputs {
    ExpectedStartInputs {
        time_start: 11 * HOUR,
        day_offset: 0,
        delay: 0,
        current_day: 0,
        total_gap: 0,
        is_linked_to_loaded: false,
        offset: 0,
        mode: OffsetMode::Absolute,
        actual_start: Some(10 * HOUR),
        planned_start: Some(10 * HOUR),
    }
}

#[test]
fn an_upcoming_event_on_schedule_starts_when_planned() {
    assert_eq!(expected_start(&upcoming()), 11 * HOUR);
}

#[test]
fn a_linked_event_moves_with_the_offset_because_it_has_no_slack() {
    let inputs = ExpectedStartInputs {
        is_linked_to_loaded: true,
        offset: 5 * MINUTE,
        total_gap: 30 * MINUTE, // irrelevant when linked
        ..upcoming()
    };
    assert_eq!(expected_start(&inputs), 11 * HOUR + 5 * MINUTE);
}

#[test]
fn idle_time_ahead_absorbs_lateness_entirely() {
    let inputs = ExpectedStartInputs {
        offset: 5 * MINUTE,
        total_gap: 10 * MINUTE,
        ..upcoming()
    };
    // there is more slack than lateness, so the plan still holds
    assert_eq!(expected_start(&inputs), 11 * HOUR);
}

#[test]
fn idle_time_absorbs_what_it_can_of_a_larger_delay() {
    let inputs = ExpectedStartInputs {
        offset: 15 * MINUTE,
        total_gap: 5 * MINUTE,
        ..upcoming()
    };
    assert_eq!(expected_start(&inputs), 11 * HOUR + 10 * MINUTE);
}

#[test]
fn a_delay_entry_pushes_the_expected_start() {
    let inputs = ExpectedStartInputs {
        delay: 7 * MINUTE,
        ..upcoming()
    };
    assert_eq!(expected_start(&inputs), 11 * HOUR + 7 * MINUTE);
}

#[test]
fn an_event_on_a_later_day_is_expected_a_day_out() {
    let inputs = ExpectedStartInputs {
        time_start: HOUR,
        day_offset: 1,
        ..upcoming()
    };
    assert_eq!(expected_start(&inputs), HOUR + DAY);
}

#[test]
fn relative_mode_shifts_the_plan_by_how_late_the_run_began() {
    let inputs = ExpectedStartInputs {
        mode: OffsetMode::Relative,
        actual_start: Some(10 * HOUR + 30 * MINUTE),
        planned_start: Some(10 * HOUR),
        ..upcoming()
    };
    // the whole plan slid 30 minutes when the show started late
    assert_eq!(expected_start(&inputs), 11 * HOUR + 30 * MINUTE);
}
