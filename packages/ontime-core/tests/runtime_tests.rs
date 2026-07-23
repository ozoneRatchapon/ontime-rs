use ontime_core::rundown::{Insert, Rundown};
use ontime_core::runtime::{Command, Playback, Runtime, TimerPhase};
use serde_json::{json, Value};

const T0: i64 = 1_700_000_000_000;

/// A rundown of three one-minute events starting at 10:00.
fn fixture() -> (Rundown, Vec<String>) {
    let mut rundown = Rundown::new("default", "Test");
    let mut ids = Vec::new();
    for (index, title) in ["A", "B", "C"].iter().enumerate() {
        let entry = rundown.add_entry(
            &json!({
                "title": title,
                "duration": 60_000,
                "timeStart": 36_000_000 + (index as i64) * 60_000,
                "timeWarning": 30_000,
                "timeDanger": 10_000,
            }),
            Insert::End,
        );
        ids.push(entry["id"].as_str().unwrap().to_string());
    }
    (rundown, ids)
}

fn timer(runtime: &Runtime, rundown: &Rundown, now: i64) -> Value {
    runtime.snapshot(rundown, now, 0)["timer"].clone()
}

#[test]
fn parses_the_commands_the_client_sends() {
    assert_eq!(Command::parse("start", None), Some(Command::Start));
    assert_eq!(
        Command::parse("start", Some(&json!("next"))),
        Some(Command::StartNext)
    );
    assert_eq!(
        Command::parse("start", Some(&json!({"id": "x"}))),
        Some(Command::StartId("x".into()))
    );
    assert_eq!(
        Command::parse("load", Some(&json!("next"))),
        Some(Command::LoadNext)
    );
    assert_eq!(
        Command::parse("load", Some(&json!("previous"))),
        Some(Command::LoadPrevious)
    );
    assert_eq!(Command::parse("pause", None), Some(Command::Pause));
    assert_eq!(Command::parse("stop", None), Some(Command::Stop));
    // addTime is sent in milliseconds
    assert_eq!(
        Command::parse("addtime", Some(&json!(300_000))),
        Some(Command::AddTime(300_000))
    );
    assert_eq!(Command::parse("roll", None), Some(Command::Roll));

    // handled elsewhere: aux timers and the overlay are not playback commands
    assert_eq!(Command::parse("auxtimer", Some(&json!({}))), None);
    assert_eq!(Command::parse("message", Some(&json!({}))), None);
}

#[test]
fn start_loads_the_first_event_when_nothing_is_selected() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();

    assert!(runtime.apply(Command::Start, &rundown, T0, 0));

    assert_eq!(runtime.playback, Playback::Play);
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert_eq!(runtime.actual_start, Some(T0));
}

#[test]
fn the_timer_counts_down_from_the_event_duration() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    let at_start = timer(&runtime, &rundown, T0);
    assert_eq!(at_start["duration"], 60_000);
    assert_eq!(at_start["elapsed"], 0);
    assert_eq!(at_start["current"], 60_000);

    let after_20s = timer(&runtime, &rundown, T0 + 20_000);
    assert_eq!(after_20s["elapsed"], 20_000);
    assert_eq!(after_20s["current"], 40_000);
}

#[test]
fn the_timer_goes_negative_in_overtime() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    let overrun = timer(&runtime, &rundown, T0 + 75_000);
    assert_eq!(overrun["current"], -15_000);
    assert_eq!(overrun["phase"], "overtime");
}

#[test]
fn phase_follows_the_warning_and_danger_thresholds() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    // thresholds in the fixture: warning 30s, danger 10s
    assert_eq!(timer(&runtime, &rundown, T0 + 10_000)["phase"], "default");
    assert_eq!(timer(&runtime, &rundown, T0 + 35_000)["phase"], "warning");
    assert_eq!(timer(&runtime, &rundown, T0 + 55_000)["phase"], "danger");
}

#[test]
fn pausing_freezes_the_timer_and_resuming_continues_it() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    runtime.apply(Command::Pause, &rundown, T0 + 20_000, 0);
    assert_eq!(runtime.playback, Playback::Pause);

    // time passes while paused, the timer does not move
    assert_eq!(timer(&runtime, &rundown, T0 + 20_000)["current"], 40_000);
    assert_eq!(timer(&runtime, &rundown, T0 + 50_000)["current"], 40_000);

    runtime.apply(Command::Start, &rundown, T0 + 50_000, 0);
    assert_eq!(runtime.playback, Playback::Play);

    // 10s after resuming, 30s total elapsed
    assert_eq!(timer(&runtime, &rundown, T0 + 60_000)["current"], 30_000);

    // resuming re-anchors to a whole second rather than carrying the paused span
    // forward, so the countdown stays in step with the wall clock
    assert_eq!(runtime.started_at.unwrap() % 1000, 0);
}

#[test]
fn stop_clears_everything() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::Stop, &rundown, T0 + 10_000, 0);

    assert_eq!(runtime.playback, Playback::Stop);
    assert!(runtime.selected_id.is_none());
    assert!(runtime.started_at.is_none());

    let stopped = timer(&runtime, &rundown, T0 + 10_000);
    assert!(stopped["current"].is_null());
    assert_eq!(stopped["phase"], "none");
}

#[test]
fn addtime_extends_the_running_event() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    runtime.apply(Command::AddTime(30_000), &rundown, T0 + 10_000, 0);

    let extended = timer(&runtime, &rundown, T0 + 10_000);
    assert_eq!(extended["addedTime"], 30_000);
    assert_eq!(extended["duration"], 90_000);
    assert_eq!(extended["current"], 80_000);
}

#[test]
fn addtime_can_remove_time() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::AddTime(-30_000), &rundown, T0 + 10_000, 0);

    assert_eq!(timer(&runtime, &rundown, T0 + 10_000)["current"], 20_000);
}

#[test]
fn load_next_and_previous_walk_the_rundown() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();

    runtime.apply(Command::LoadNext, &rundown, T0, 0);
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert_eq!(runtime.playback, Playback::Armed);

    runtime.apply(Command::LoadNext, &rundown, T0, 0);
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[1].as_str()));

    runtime.apply(Command::LoadPrevious, &rundown, T0, 0);
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));

    // does not walk off the start
    assert!(!runtime.apply(Command::LoadPrevious, &rundown, T0, 0));
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
}

#[test]
fn loading_resets_the_timer_of_the_previous_event() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::AddTime(30_000), &rundown, T0 + 10_000, 0);

    runtime.apply(Command::LoadId(ids[1].clone()), &rundown, T0 + 10_000, 0);

    assert_eq!(runtime.added_time, 0);
    assert!(runtime.started_at.is_none());
    // armed shows the full duration, not counting
    let armed = timer(&runtime, &rundown, T0 + 10_000);
    assert_eq!(armed["current"], 60_000);
    assert!(armed["elapsed"].is_null());
}

#[test]
fn start_next_advances_and_runs() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::StartNext, &rundown, T0 + 60_000, 0);

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[1].as_str()));
    assert_eq!(runtime.playback, Playback::Play);
    assert_eq!(timer(&runtime, &rundown, T0 + 60_000)["elapsed"], 0);
    // the run's start time is preserved across events
    assert_eq!(runtime.actual_start, Some(T0));
}

#[test]
fn reload_restarts_the_current_event_and_keeps_playing() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::Reload, &rundown, T0 + 40_000, 0);

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert_eq!(runtime.playback, Playback::Play);
    assert_eq!(timer(&runtime, &rundown, T0 + 40_000)["elapsed"], 0);
}

#[test]
fn skipped_events_are_not_selectable() {
    let (mut rundown, ids) = fixture();
    rundown
        .edit_entry(&ids[1], &json!({ "skip": true }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::StartNext, &rundown, T0 + 60_000, 0);

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[2].as_str()));
    assert_eq!(runtime.snapshot(&rundown, T0, 0)["rundown"]["numEvents"], 2);
}

#[test]
fn the_snapshot_reports_the_current_and_next_event() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    let snapshot = runtime.snapshot(&rundown, T0, 0);
    assert_eq!(snapshot["eventNow"]["id"], ids[0].as_str());
    assert_eq!(snapshot["eventNext"]["id"], ids[1].as_str());
    assert_eq!(snapshot["rundown"]["selectedEventIndex"], 0);
    assert_eq!(snapshot["rundown"]["numEvents"], 3);
}

#[test]
fn the_snapshot_matches_the_clients_runtime_store_shape() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    let snapshot = runtime.snapshot(&rundown, T0, 0);

    for key in [
        "clock",
        "timer",
        "rundown",
        "offset",
        "eventNow",
        "eventNext",
        "groupNow",
    ] {
        assert!(snapshot.get(key).is_some(), "missing {key}");
    }
    for key in [
        "addedTime",
        "current",
        "duration",
        "elapsed",
        "expectedFinish",
        "phase",
        "playback",
        "secondaryTimer",
        "startedAt",
    ] {
        assert!(snapshot["timer"].get(key).is_some(), "timer missing {key}");
    }
    for key in [
        "selectedEventIndex",
        "numEvents",
        "plannedStart",
        "plannedEnd",
        "actualStart",
        "actualGroupStart",
        "currentDay",
    ] {
        assert!(
            snapshot["rundown"].get(key).is_some(),
            "rundown missing {key}"
        );
    }
    for key in [
        "absolute",
        "relative",
        "mode",
        "expectedFlagStart",
        "expectedGroupEnd",
        "expectedRundownEnd",
    ] {
        assert!(
            snapshot["offset"].get(key).is_some(),
            "offset missing {key}"
        );
    }
}

#[test]
fn playback_serialises_to_the_values_the_client_expects() {
    assert_eq!(serde_json::to_value(Playback::Play).unwrap(), json!("play"));
    assert_eq!(
        serde_json::to_value(Playback::Pause).unwrap(),
        json!("pause")
    );
    assert_eq!(serde_json::to_value(Playback::Stop).unwrap(), json!("stop"));
    assert_eq!(
        serde_json::to_value(Playback::Armed).unwrap(),
        json!("armed")
    );
    assert_eq!(serde_json::to_value(Playback::Roll).unwrap(), json!("roll"));
    assert_eq!(
        serde_json::to_value(TimerPhase::Overtime).unwrap(),
        json!("overtime")
    );
    assert_eq!(
        serde_json::to_value(TimerPhase::None).unwrap(),
        json!("none")
    );
}

#[test]
fn the_clock_is_reported_as_time_of_day_in_the_operators_timezone() {
    let (rundown, _) = fixture();
    let runtime = Runtime::default();

    // 2023-11-14T22:13:20Z
    let utc = runtime.snapshot(&rundown, T0, 0)["clock"].as_i64().unwrap();
    assert_eq!(utc, 80_000_000); // 22:13:20

    // same instant, UTC+7 — rolls into the next day
    let ict = runtime.snapshot(&rundown, T0, 7 * 3_600_000)["clock"]
        .as_i64()
        .unwrap();
    assert_eq!(ict, 18_800_000); // 05:13:20
}

#[test]
fn started_at_is_reported_as_time_of_day() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    let snapshot = runtime.snapshot(&rundown, T0, 0);
    assert_eq!(snapshot["timer"]["startedAt"], 80_000_000);
    assert_eq!(snapshot["timer"]["expectedFinish"], 80_060_000);
}

#[test]
fn a_runtime_survives_a_serialisation_round_trip() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    runtime.apply(Command::Pause, &rundown, T0 + 15_000, 0);

    let raw = serde_json::to_string(&runtime).unwrap();
    let restored: Runtime = serde_json::from_str(&raw).unwrap();

    assert_eq!(restored.playback, Playback::Pause);
    assert_eq!(restored.selected_id, runtime.selected_id);
    assert_eq!(
        timer(&restored, &rundown, T0 + 99_000)["current"],
        timer(&runtime, &rundown, T0 + 99_000)["current"]
    );
}

#[test]
fn start_is_ignored_when_the_rundown_is_empty() {
    let rundown = Rundown::new("default", "Empty");
    let mut runtime = Runtime::default();

    assert!(!runtime.apply(Command::Start, &rundown, T0, 0));
    assert_eq!(runtime.playback, Playback::Stop);
}

#[test]
fn end_action_none_runs_into_overtime() {
    let (rundown, ids) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    assert!(!runtime.on_tick(&rundown, T0 + 90_000, 0));
    assert_eq!(runtime.selected_id.as_deref(), Some(ids[0].as_str()));
    assert_eq!(timer(&runtime, &rundown, T0 + 90_000)["current"], -30_000);
}

#[test]
fn end_action_load_next_arms_the_following_event() {
    let (mut rundown, ids) = fixture();
    rundown
        .edit_entry(&ids[0], &json!({ "endAction": "load-next" }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);

    assert!(!runtime.on_tick(&rundown, T0 + 59_000, 0));
    assert!(runtime.on_tick(&rundown, T0 + 60_000, 0));

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[1].as_str()));
    assert_eq!(runtime.playback, Playback::Armed);
}

#[test]
fn end_action_play_next_rolls_straight_into_the_following_event() {
    let (mut rundown, ids) = fixture();
    rundown
        .edit_entry(&ids[0], &json!({ "endAction": "play-next" }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0, 0);
    assert!(runtime.on_tick(&rundown, T0 + 60_000, 0));

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[1].as_str()));
    assert_eq!(runtime.playback, Playback::Play);
    assert_eq!(timer(&runtime, &rundown, T0 + 60_000)["elapsed"], 0);
}

#[test]
fn end_action_on_the_last_event_holds_instead_of_looping() {
    let (mut rundown, ids) = fixture();
    rundown
        .edit_entry(&ids[2], &json!({ "endAction": "play-next" }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::StartId(ids[2].clone()), &rundown, T0, 0);
    assert!(runtime.on_tick(&rundown, T0 + 60_000, 0));

    assert_eq!(runtime.selected_id.as_deref(), Some(ids[2].as_str()));
    assert_eq!(runtime.playback, Playback::Pause);
}

#[test]
fn a_tick_does_nothing_when_playback_is_stopped() {
    let (mut rundown, ids) = fixture();
    rundown
        .edit_entry(&ids[0], &json!({ "endAction": "play-next" }))
        .unwrap();

    let mut runtime = Runtime::default();
    assert!(!runtime.on_tick(&rundown, T0 + 60_000, 0));
}

// --- ticking in step with the wall clock -----------------------------------
//
// The countdown's displayed second changes when the remaining time crosses a
// whole second. If that is measured from the instant the operator pressed the
// button, it changes at some arbitrary fraction of a second and visibly drifts
// out of step with the clock beside it. These pin the alignment.

/// The instant at which `ceil(current / 1000)` next changes, searched to the ms.
/// This is exactly what the client displays: see `millisToSeconds` in
/// packages/utils, which uses `Math.ceil` for a count-down.
fn next_display_change(runtime: &Runtime, rundown: &Rundown, from: i64) -> i64 {
    let displayed = |t: i64| {
        let current = timer(runtime, rundown, t)["current"].as_i64().unwrap();
        (current as f64 / 1000.0).ceil() as i64
    };
    let start = displayed(from);
    (from + 1..=from + 2000)
        .find(|t| displayed(*t) != start)
        .expect("the countdown should change within two seconds")
}

#[test]
fn the_countdown_changes_on_a_whole_second_however_late_the_press() {
    let (rundown, _) = fixture();

    // press at every awkward fraction of a second
    for offset in [0, 1, 137, 350, 500, 501, 750, 999] {
        let mut runtime = Runtime::default();
        let pressed = T0 + offset;
        runtime.apply(Command::Start, &rundown, pressed, 0);

        let change = next_display_change(&runtime, &rundown, pressed);
        assert_eq!(
            change % 1000,
            0,
            "pressed at +{offset}ms, countdown changed at {}ms past the second",
            change % 1000
        );
    }
}

#[test]
fn the_countdown_keeps_ticking_on_whole_seconds_for_the_whole_event() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0 + 350, 0);

    // walk several transitions, not just the first
    let mut at = T0 + 350;
    for _ in 0..5 {
        at = next_display_change(&runtime, &rundown, at);
        assert_eq!(at % 1000, 0, "drifted off the second at {at}");
    }
}

#[test]
fn alignment_survives_a_pause_at_an_awkward_moment() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0 + 350, 0);

    // pause and resume on fractions of a second
    runtime.apply(Command::Pause, &rundown, T0 + 12_617, 0);
    runtime.apply(Command::Start, &rundown, T0 + 40_212, 0);

    let mut at = T0 + 40_212;
    for _ in 0..3 {
        at = next_display_change(&runtime, &rundown, at);
        assert_eq!(at % 1000, 0, "drifted off the second after a pause at {at}");
    }
}

#[test]
fn alignment_survives_added_time() {
    let (rundown, _) = fixture();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, T0 + 350, 0);
    runtime.apply(Command::AddTime(30_000), &rundown, T0 + 4_321, 0);

    let change = next_display_change(&runtime, &rundown, T0 + 4_321);
    assert_eq!(change % 1000, 0);
}

#[test]
fn a_press_is_never_moved_by_more_than_half_a_second() {
    let (rundown, _) = fixture();

    for offset in [0, 350, 500, 999] {
        let mut runtime = Runtime::default();
        runtime.apply(Command::Start, &rundown, T0 + offset, 0);
        let moved = (runtime.started_at.unwrap() - (T0 + offset)).abs();
        assert!(moved <= 500, "press at +{offset}ms was moved {moved}ms");
    }
}

#[test]
fn the_countdown_never_exceeds_the_duration_while_anchoring() {
    let (rundown, _) = fixture();

    // pressing just past a second rounds the anchor forward, so for a moment the
    // start is in the future; the remaining time must not read above the duration
    // or a 60s event would flash 01:01
    for offset in [500, 600, 750, 999] {
        let mut runtime = Runtime::default();
        let pressed = T0 + offset;
        runtime.apply(Command::Start, &rundown, pressed, 0);

        for t in pressed..pressed + 500 {
            let current = timer(&runtime, &rundown, t)["current"].as_i64().unwrap();
            assert!(
                current <= 60_000,
                "pressed at +{offset}ms: showed {current}ms remaining of a 60000ms event"
            );
        }
    }
}

#[test]
fn resuming_never_moves_the_displayed_value_by_a_whole_second() {
    let (rundown, _) = fixture();

    for (pause_at, resume_at) in [(12_617, 40_212), (5_001, 9_999), (30_500, 31_499)] {
        let mut runtime = Runtime::default();
        runtime.apply(Command::Start, &rundown, T0 + 350, 0);
        runtime.apply(Command::Pause, &rundown, T0 + pause_at, 0);

        let paused_value = timer(&runtime, &rundown, T0 + pause_at)["current"]
            .as_i64()
            .unwrap();

        runtime.apply(Command::Start, &rundown, T0 + resume_at, 0);
        let resumed_value = timer(&runtime, &rundown, T0 + resume_at)["current"]
            .as_i64()
            .unwrap();

        let moved = (resumed_value - paused_value).abs();
        assert!(
            moved <= 500,
            "paused at {pause_at} resumed at {resume_at}: value moved {moved}ms"
        );
    }
}
