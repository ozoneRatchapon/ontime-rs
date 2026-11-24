use ontime_core::aux_timer::{AuxTimer, AuxTimers, SimpleDirection, SimplePlayback};
use serde_json::json;

const T0: i64 = 1_700_000_000_000;

#[test]
fn a_fresh_timer_is_stopped_at_five_minutes() {
    let timer = AuxTimer::default();
    assert_eq!(timer.playback, SimplePlayback::Stop);
    assert_eq!(timer.direction, SimpleDirection::CountDown);
    assert_eq!(timer.current(T0), 300_000);
}

#[test]
fn counting_down_reaches_zero_and_keeps_going() {
    let mut timer = AuxTimer::default();
    timer.set_time(10_000);
    timer.start(T0);

    assert_eq!(timer.current(T0), 10_000);
    assert_eq!(timer.current(T0 + 4_000), 6_000);
    // an aux timer is a stopwatch: it does not stop at zero
    assert_eq!(timer.current(T0 + 15_000), -5_000);
}

#[test]
fn counting_up_starts_from_the_duration() {
    let mut timer = AuxTimer::default();
    timer.set_time(0);
    timer.set_direction(SimpleDirection::CountUp, T0);
    timer.start(T0);

    assert_eq!(timer.current(T0), 0);
    assert_eq!(timer.current(T0 + 7_000), 7_000);
}

#[test]
fn pausing_freezes_the_value_and_resuming_continues_it() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0);

    timer.pause(T0 + 20_000);
    assert_eq!(timer.current(T0 + 20_000), 40_000);
    // time passing while paused must not move it
    assert_eq!(timer.current(T0 + 90_000), 40_000);

    timer.start(T0 + 90_000);
    assert_eq!(timer.current(T0 + 90_000), 40_000);
    assert_eq!(timer.current(T0 + 95_000), 35_000);
}

#[test]
fn stopping_returns_to_the_configured_duration() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0);
    timer.stop();

    assert_eq!(timer.playback, SimplePlayback::Stop);
    assert_eq!(timer.current(T0 + 999_999), 60_000);
}

#[test]
fn add_time_moves_the_value_whether_running_or_not() {
    let mut running = AuxTimer::default();
    running.set_time(60_000);
    running.start(T0);
    running.add_time(30_000);
    assert_eq!(running.current(T0 + 10_000), 80_000);

    let mut stopped = AuxTimer::default();
    stopped.set_time(60_000);
    stopped.add_time(30_000);
    assert_eq!(stopped.current(T0), 90_000);
}

#[test]
fn add_time_can_remove_time() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0);
    timer.add_time(-20_000);
    assert_eq!(timer.current(T0), 40_000);
}

#[test]
fn changing_direction_while_running_does_not_jump_the_value() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0);

    // 20s in, showing 40s remaining
    assert_eq!(timer.current(T0 + 20_000), 40_000);

    timer.set_direction(SimpleDirection::CountUp, T0 + 20_000);
    // the displayed value is continuous across the switch
    assert_eq!(timer.current(T0 + 20_000), 40_000);
    assert_eq!(timer.current(T0 + 25_000), 45_000);
}

#[test]
fn starting_an_already_running_timer_does_nothing() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0);
    timer.start(T0 + 10_000);

    // if start had rebased, this would read 60_000
    assert_eq!(timer.current(T0 + 10_000), 50_000);
}

#[test]
fn pausing_a_stopped_timer_does_nothing() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.pause(T0);
    assert_eq!(timer.playback, SimplePlayback::Stop);
}

#[test]
fn a_timer_survives_a_serialisation_round_trip() {
    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0);
    timer.pause(T0 + 15_000);

    let restored: AuxTimer = serde_json::from_str(&serde_json::to_string(&timer).unwrap()).unwrap();
    assert_eq!(restored.current(T0 + 99_999), timer.current(T0 + 99_999));
}

// --- the websocket payloads the client sends -------------------------------

#[test]
fn the_client_payload_starts_the_addressed_timer_only() {
    let mut timers = AuxTimers::default();
    assert!(timers.apply(&json!({ "2": "start" }), T0));

    assert!(!timers.timers[0].is_running());
    assert!(timers.timers[1].is_running());
    assert!(!timers.timers[2].is_running());
}

#[test]
fn the_client_payload_sets_direction_and_duration() {
    let mut timers = AuxTimers::default();

    assert!(timers.apply(&json!({ "1": { "duration": 90_000 } }), T0));
    assert_eq!(timers.timers[0].current(T0), 90_000);

    assert!(timers.apply(&json!({ "1": { "direction": "count-up" } }), T0));
    assert_eq!(timers.timers[0].direction, SimpleDirection::CountUp);
}

#[test]
fn pause_and_stop_arrive_the_same_way() {
    let mut timers = AuxTimers::default();
    timers.apply(&json!({ "3": { "duration": 60_000 } }), T0);
    timers.apply(&json!({ "3": "start" }), T0);

    timers.apply(&json!({ "3": "pause" }), T0 + 10_000);
    assert_eq!(timers.timers[2].playback, SimplePlayback::Pause);

    timers.apply(&json!({ "3": "stop" }), T0 + 10_000);
    assert_eq!(timers.timers[2].playback, SimplePlayback::Stop);
    assert_eq!(timers.timers[2].current(T0 + 10_000), 60_000);
}

#[test]
fn an_unknown_index_or_action_is_ignored() {
    let mut timers = AuxTimers::default();
    assert!(!timers.apply(&json!({ "0": "start" }), T0));
    assert!(!timers.apply(&json!({ "4": "start" }), T0));
    assert!(!timers.apply(&json!({ "1": "explode" }), T0));
    assert!(!timers.apply(&json!({ "notanumber": "start" }), T0));
    assert!(!timers.apply(&json!("start"), T0));
    assert!(timers.timers.iter().all(|t| !t.is_running()));
}

#[test]
fn any_running_reports_whether_a_tick_is_needed() {
    let mut timers = AuxTimers::default();
    assert!(!timers.any_running());

    timers.apply(&json!({ "2": "start" }), T0);
    assert!(timers.any_running());

    timers.apply(&json!({ "2": "stop" }), T0);
    assert!(!timers.any_running());
}

#[test]
fn the_snapshot_matches_the_runtime_store_shape() {
    let timers = AuxTimers::default();
    let snapshot = timers.snapshot(T0, &Default::default());

    for key in ["auxtimer1", "auxtimer2", "auxtimer3"] {
        let state = &snapshot[key];
        for field in ["duration", "current", "playback", "direction", "name"] {
            assert!(state.get(field).is_some(), "{key} missing {field}");
        }
    }

    // wire values the client's enums expect
    assert_eq!(snapshot["auxtimer1"]["playback"], "stop");
    assert_eq!(snapshot["auxtimer1"]["direction"], "count-down");
}

#[test]
fn playback_and_direction_serialise_to_the_clients_values() {
    assert_eq!(
        serde_json::to_value(SimplePlayback::Start).unwrap(),
        json!("start")
    );
    assert_eq!(
        serde_json::to_value(SimplePlayback::Pause).unwrap(),
        json!("pause")
    );
    assert_eq!(
        serde_json::to_value(SimplePlayback::Stop).unwrap(),
        json!("stop")
    );
    assert_eq!(
        serde_json::to_value(SimpleDirection::CountUp).unwrap(),
        json!("count-up")
    );
    assert_eq!(
        serde_json::to_value(SimpleDirection::CountDown).unwrap(),
        json!("count-down")
    );
}

#[test]
fn an_aux_timer_ticks_on_whole_seconds_however_late_the_press() {
    // it sits next to the wall clock on screen, so its second has to change when
    // the clock's does — the same requirement as the main countdown
    let displayed = |timer: &AuxTimer, t: i64| (timer.current(t) as f64 / 1000.0).ceil() as i64;

    for offset in [0, 137, 350, 500, 750, 999] {
        let mut timer = AuxTimer::default();
        timer.set_time(60_000);
        let pressed = T0 + offset;
        timer.start(pressed);

        let first = displayed(&timer, pressed);
        let change = (pressed + 1..=pressed + 2000)
            .find(|t| displayed(&timer, *t) != first)
            .expect("should change within two seconds");

        assert_eq!(
            change % 1000,
            0,
            "pressed at +{offset}ms, changed {}ms past the second",
            change % 1000
        );
    }
}

#[test]
fn an_aux_timer_never_shows_more_than_it_was_set_to() {
    // anchoring forward can put the start briefly in the future
    for offset in [500, 750, 999] {
        let mut timer = AuxTimer::default();
        timer.set_time(60_000);
        let pressed = T0 + offset;
        timer.start(pressed);

        for t in pressed..pressed + 500 {
            assert!(
                timer.current(t) <= 60_000,
                "pressed at +{offset}ms showed {}ms of a 60000ms timer",
                timer.current(t)
            );
        }
    }
}

#[test]
fn aux_alignment_survives_pause_and_resume() {
    let displayed = |timer: &AuxTimer, t: i64| (timer.current(t) as f64 / 1000.0).ceil() as i64;

    let mut timer = AuxTimer::default();
    timer.set_time(60_000);
    timer.start(T0 + 350);
    timer.pause(T0 + 12_617);
    timer.start(T0 + 40_212);

    let from = T0 + 40_212;
    let first = displayed(&timer, from);
    let change = (from + 1..=from + 2000)
        .find(|t| displayed(&timer, *t) != first)
        .expect("should change within two seconds");
    assert_eq!(change % 1000, 0);
}
