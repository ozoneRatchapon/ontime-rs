use ontime_core::rundown::{Insert, Rundown};
use ontime_core::runtime::{load_roll, Command, Playback, Runtime, TimerPhase};
use serde_json::json;

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// Epoch midnight, so `epoch(t) = t` as a time of day with a zero timezone.
const MIDNIGHT: i64 = 1_700_000_000_000 - (1_700_000_000_000 % DAY);
fn at(time_of_day: i64) -> i64 {
    MIDNIGHT + time_of_day
}

/// Three one-hour events at 10:00, 11:00 and 12:00.
fn day_show() -> (Rundown, Vec<String>) {
    let mut rundown = Rundown::new("default", "Roll");
    let mut ids = Vec::new();
    for hour in [10, 11, 12] {
        let entry = rundown.add_entry(
            &json!({
                "title": format!("{hour}:00"),
                "timeStart": hour * HOUR,
                "duration": HOUR,
                "timeEnd": (hour + 1) * HOUR,
            }),
            Insert::End,
        );
        ids.push(entry["id"].as_str().unwrap().to_string());
    }
    (rundown, ids)
}

// --- finding what should be on air ----------------------------------------

#[test]
fn before_the_show_the_first_event_is_armed() {
    let (rundown, ids) = day_show();
    let found = load_roll(&rundown, 9 * HOUR).unwrap();

    assert_eq!(found.id, ids[0]);
    assert!(found.is_pending);
    assert_eq!(found.time_start, 10 * HOUR);
}

#[test]
fn during_an_event_that_event_is_joined_part_way() {
    let (rundown, ids) = day_show();
    let found = load_roll(&rundown, 10 * HOUR + 20 * MINUTE).unwrap();

    assert_eq!(found.id, ids[0]);
    assert!(!found.is_pending);
    assert_eq!(found.elapsed, 20 * MINUTE);
}

#[test]
fn between_events_the_next_one_is_armed() {
    let (mut rundown, ids) = day_show();
    // leave a gap: shorten the first event so 10:45 falls in dead air
    rundown
        .edit_entry(
            &ids[0],
            &json!({ "duration": 30 * MINUTE, "timeEnd": 10 * HOUR + 30 * MINUTE }),
        )
        .unwrap();

    let found = load_roll(&rundown, 10 * HOUR + 45 * MINUTE).unwrap();
    assert_eq!(found.id, ids[1]);
    assert!(found.is_pending);
}

#[test]
fn a_later_event_is_picked_up_mid_show() {
    let (rundown, ids) = day_show();
    let found = load_roll(&rundown, 12 * HOUR + 30 * MINUTE).unwrap();

    assert_eq!(found.id, ids[2]);
    assert!(!found.is_pending);
    assert_eq!(found.elapsed, 30 * MINUTE);
}

#[test]
fn after_the_show_the_top_is_armed_for_the_next_day() {
    let (rundown, ids) = day_show();
    let found = load_roll(&rundown, 20 * HOUR).unwrap();

    assert_eq!(found.id, ids[0]);
    assert!(found.is_pending);
}

#[test]
fn zero_duration_events_are_skipped_so_playback_cannot_stick() {
    let mut rundown = Rundown::new("default", "Roll");
    let stuck = rundown.add_entry(
        &json!({ "title": "zero", "timeStart": 10 * HOUR, "duration": 0, "timeEnd": 10 * HOUR }),
        Insert::End,
    );
    let stuck = stuck["id"].as_str().unwrap().to_string();
    let real = rundown.add_entry(
        &json!({ "title": "real", "timeStart": 10 * HOUR, "duration": HOUR, "timeEnd": 11 * HOUR }),
        Insert::End,
    );
    let real = real["id"].as_str().unwrap().to_string();

    let found = load_roll(&rundown, 10 * HOUR + MINUTE).unwrap();
    assert_ne!(found.id, stuck);
    assert_eq!(found.id, real);
}

#[test]
fn an_event_running_through_midnight_is_found_from_the_small_hours() {
    let mut rundown = Rundown::new("default", "Overnight");
    // 23:30 to 00:30
    let overnight = rundown.add_entry(
        &json!({
            "title": "overnight",
            "timeStart": 23 * HOUR + 30 * MINUTE,
            "duration": HOUR,
            "timeEnd": 30 * MINUTE,
        }),
        Insert::End,
    );
    let overnight = overnight["id"].as_str().unwrap().to_string();

    // at 00:10 we are twenty minutes into it, not waiting for tomorrow
    let found = load_roll(&rundown, 10 * MINUTE).unwrap();
    assert_eq!(found.id, overnight);
    assert!(!found.is_pending);
    assert_eq!(found.elapsed, 40 * MINUTE);
}

#[test]
fn an_empty_rundown_has_nothing_to_roll() {
    let rundown = Rundown::new("default", "Empty");
    assert!(load_roll(&rundown, 10 * HOUR).is_none());
}

#[test]
fn skipped_events_are_not_rolled_into() {
    let (mut rundown, ids) = day_show();
    rundown
        .edit_entry(&ids[0], &json!({ "skip": true }))
        .unwrap();

    let found = load_roll(&rundown, 10 * HOUR + 20 * MINUTE).unwrap();
    assert_ne!(found.id, ids[0]);
}

// --- the runtime in roll mode ---------------------------------------------

#[test]
fn rolling_before_the_show_waits_and_counts_down() {
    let (rundown, ids) = day_show();
    let mut runtime = Runtime::default();

    assert!(runtime.apply(Command::Roll, &rundown, at(9 * HOUR), 0));

    assert_eq!(runtime.playback, Playback::Roll);
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert_eq!(runtime.secondary_target, Some(10 * HOUR));

    let snapshot = runtime.snapshot(&rundown, at(9 * HOUR), 0);
    assert_eq!(snapshot["timer"]["secondaryTimer"], HOUR);
    assert_eq!(snapshot["timer"]["playback"], "roll");
    // the timer views show a countdown to the start, not a running timer
    assert_eq!(snapshot["timer"]["phase"], "pending");
}

#[test]
fn the_countdown_shrinks_as_the_start_approaches() {
    let (rundown, _) = day_show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Roll, &rundown, at(9 * HOUR), 0);

    let snapshot = runtime.snapshot(&rundown, at(9 * HOUR + 45 * MINUTE), 0);
    assert_eq!(snapshot["timer"]["secondaryTimer"], 15 * MINUTE);
}

#[test]
fn the_armed_event_starts_itself_when_its_time_comes() {
    let (rundown, ids) = day_show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Roll, &rundown, at(9 * HOUR), 0);

    // not yet
    assert!(!runtime.on_tick(&rundown, at(10 * HOUR - 1000), 0));
    assert!(runtime.secondary_target.is_some());

    // now
    assert!(runtime.on_tick(&rundown, at(10 * HOUR), 0));
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert!(runtime.secondary_target.is_none());
    assert_eq!(runtime.started_at, Some(at(10 * HOUR)));

    let snapshot = runtime.snapshot(&rundown, at(10 * HOUR), 0);
    assert_eq!(snapshot["timer"]["current"], HOUR);
    assert!(snapshot["timer"]["secondaryTimer"].is_null());
}

#[test]
fn rolling_mid_event_joins_it_part_way_rather_than_restarting() {
    let (rundown, ids) = day_show();
    let mut runtime = Runtime::default();

    runtime.apply(Command::Roll, &rundown, at(10 * HOUR + 20 * MINUTE), 0);

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert!(runtime.secondary_target.is_none());

    let snapshot = runtime.snapshot(&rundown, at(10 * HOUR + 20 * MINUTE), 0);
    // forty minutes left of the hour, not a fresh hour
    assert_eq!(snapshot["timer"]["current"], 40 * MINUTE);
    assert_eq!(snapshot["timer"]["elapsed"], 20 * MINUTE);
}

#[test]
fn roll_moves_to_the_next_event_by_itself() {
    let (rundown, ids) = day_show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Roll, &rundown, at(10 * HOUR), 0);
    runtime.on_tick(&rundown, at(10 * HOUR), 0);
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));

    // the first event ends exactly as the second begins
    assert!(runtime.on_tick(&rundown, at(11 * HOUR), 0));
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[1].as_str()));
    assert!(runtime.secondary_target.is_none());
}

#[test]
fn roll_waits_through_a_gap_rather_than_running_the_next_event_early() {
    let (mut rundown, ids) = day_show();
    // first event now ends at 10:30, leaving half an hour of dead air
    rundown
        .edit_entry(
            &ids[0],
            &json!({ "duration": 30 * MINUTE, "timeEnd": 10 * HOUR + 30 * MINUTE }),
        )
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Roll, &rundown, at(10 * HOUR), 0);
    runtime.on_tick(&rundown, at(10 * HOUR), 0);

    // the first event runs out, but the second is not due yet
    assert!(runtime.on_tick(&rundown, at(10 * HOUR + 30 * MINUTE), 0));
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[1].as_str()));
    assert_eq!(runtime.secondary_target, Some(11 * HOUR));

    let snapshot = runtime.snapshot(&rundown, at(10 * HOUR + 30 * MINUTE), 0);
    assert_eq!(snapshot["timer"]["phase"], "pending");
    assert_eq!(snapshot["timer"]["secondaryTimer"], 30 * MINUTE);
}

#[test]
fn stopping_leaves_roll_mode() {
    let (rundown, _) = day_show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Roll, &rundown, at(10 * HOUR + MINUTE), 0);

    assert!(runtime.apply(Command::Stop, &rundown, at(10 * HOUR + 2 * MINUTE), 0));
    assert_eq!(runtime.playback, Playback::Stop);
    assert!(runtime.secondary_target.is_none());
    assert!(runtime.selected_id.is_none());
}

#[test]
fn a_roll_state_survives_a_serialisation_round_trip() {
    let (rundown, _) = day_show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Roll, &rundown, at(9 * HOUR), 0);

    let raw = serde_json::to_string(&runtime).unwrap();
    let restored: Runtime = serde_json::from_str(&raw).unwrap();

    assert_eq!(restored.playback, Playback::Roll);
    assert_eq!(restored.secondary_target, runtime.secondary_target);
    assert_eq!(
        restored.snapshot(&rundown, at(9 * HOUR + 30 * MINUTE), 0)["timer"]["secondaryTimer"],
        30 * MINUTE
    );
}

#[test]
fn the_pending_phase_is_reported_as_the_client_expects() {
    assert_eq!(
        serde_json::to_value(TimerPhase::Pending).unwrap(),
        json!("pending")
    );
}
