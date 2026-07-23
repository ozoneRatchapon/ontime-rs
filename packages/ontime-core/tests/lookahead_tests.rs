use ontime_core::rundown::{Insert, Rundown};
use ontime_core::runtime::{Command, Runtime};
use serde_json::json;

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY_START: i64 = 1_700_000_000_000 - (1_700_000_000_000 % 86_400_000);
fn at(time_of_day: i64) -> i64 {
    DAY_START + time_of_day
}

/// Four back-to-back half-hour events from 10:00.
fn show() -> (Rundown, Vec<String>) {
    let mut rundown = Rundown::new("default", "Look ahead");
    let mut ids = Vec::new();
    for i in 0..4 {
        let start = 10 * HOUR + i * 30 * MINUTE;
        let entry = rundown.add_entry(
            &json!({
                "title": format!("event {i}"),
                "timeStart": start,
                "duration": 30 * MINUTE,
                "timeEnd": start + 30 * MINUTE,
            }),
            Insert::End,
        );
        ids.push(entry["id"].as_str().unwrap().to_string());
    }
    (rundown, ids)
}

#[test]
fn nothing_is_ahead_when_nothing_is_loaded() {
    let (rundown, _) = show();
    let runtime = Runtime::default();
    let ahead = runtime.look_ahead(&rundown);

    assert!(ahead.flag.is_none());
    assert!(ahead.group_end.is_none());
    assert!(ahead.rundown_end.is_none());
}

#[test]
fn the_end_of_the_rundown_is_the_last_playable_event() {
    let (rundown, ids) = show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let end = runtime.look_ahead(&rundown).rundown_end.unwrap();
    assert_eq!(end.id, ids[3]);
}

#[test]
fn the_next_flag_is_found_and_the_loaded_event_is_never_it() {
    let (mut rundown, ids) = show();
    // flag the event that is about to run, and one further on
    rundown
        .edit_entry(&ids[0], &json!({ "flag": true }))
        .unwrap();
    rundown
        .edit_entry(&ids[2], &json!({ "flag": true }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let flag = runtime.look_ahead(&rundown).flag.unwrap();
    // the running event is not "the next flag" even though it carries one
    assert_eq!(flag.id, ids[2]);
}

#[test]
fn idle_time_between_events_is_accumulated_but_the_running_one_is_not_counted() {
    let (mut rundown, ids) = show();
    // push the third event ten minutes later, leaving a gap before it
    rundown
        .edit_entry(&ids[2], &json!({ "timeStart": 11 * HOUR + 10 * MINUTE }))
        .unwrap();
    rundown
        .edit_entry(&ids[3], &json!({ "flag": true }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let flag = runtime.look_ahead(&rundown).flag.unwrap();
    assert_eq!(flag.accumulated_gap, 10 * MINUTE);
}

#[test]
fn a_chain_of_linked_events_reports_no_slack() {
    let (mut rundown, ids) = show();
    for id in &ids[1..] {
        rundown
            .edit_entry(id, &json!({ "linkStart": true }))
            .unwrap();
    }
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let end = runtime.look_ahead(&rundown).rundown_end.unwrap();
    assert!(end.is_linked_to_loaded);
}

#[test]
fn one_unlinked_event_breaks_the_chain_for_everything_after_it() {
    let (mut rundown, ids) = show();
    rundown
        .edit_entry(&ids[1], &json!({ "linkStart": true }))
        .unwrap();
    rundown
        .edit_entry(&ids[2], &json!({ "linkStart": false }))
        .unwrap();
    rundown
        .edit_entry(&ids[3], &json!({ "linkStart": true }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let end = runtime.look_ahead(&rundown).rundown_end.unwrap();
    assert!(!end.is_linked_to_loaded);
}

#[test]
fn the_group_end_is_the_last_event_of_the_loaded_events_group() {
    let (mut rundown, ids) = show();
    rundown
        .group_entries(&[ids[0].clone(), ids[1].clone()])
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let group_end = runtime.look_ahead(&rundown).group_end.unwrap();
    assert_eq!(group_end.id, ids[1]);
}

#[test]
fn there_is_no_group_end_when_the_event_is_not_in_one() {
    let (rundown, _) = show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    assert!(runtime.look_ahead(&rundown).group_end.is_none());
}

// --- what the client is told ----------------------------------------------

#[test]
fn the_expected_rundown_end_matches_the_plan_when_on_schedule() {
    let (rundown, _) = show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let offset = runtime.snapshot(&rundown, at(10 * HOUR), 0)["offset"].clone();
    // four half-hour events from 10:00 end at 12:00
    assert_eq!(offset["expectedRundownEnd"], 12 * HOUR);
    assert_eq!(offset["absolute"], 0);
}

#[test]
fn running_late_pushes_the_expected_rundown_end_out() {
    let (mut rundown, ids) = show();
    // link everything so there is no slack to absorb the delay
    for id in &ids[1..] {
        rundown
            .edit_entry(id, &json!({ "linkStart": true }))
            .unwrap();
    }

    let mut runtime = Runtime::default();
    // start ten minutes late
    runtime.apply(Command::Start, &rundown, at(10 * HOUR + 10 * MINUTE), 0);

    let offset = runtime.snapshot(&rundown, at(10 * HOUR + 10 * MINUTE), 0)["offset"].clone();
    assert_eq!(offset["absolute"], 10 * MINUTE);
    assert_eq!(offset["expectedRundownEnd"], 12 * HOUR + 10 * MINUTE);
}

#[test]
fn the_expected_group_end_is_reported_for_a_grouped_event() {
    let (mut rundown, ids) = show();
    rundown
        .group_entries(&[ids[0].clone(), ids[1].clone()])
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let offset = runtime.snapshot(&rundown, at(10 * HOUR), 0)["offset"].clone();
    // the group holds the first two events, ending at 11:00
    assert_eq!(offset["expectedGroupEnd"], 11 * HOUR);
}

#[test]
fn the_expected_flag_start_is_reported_and_the_flagged_event_is_named() {
    let (mut rundown, ids) = show();
    rundown
        .edit_entry(&ids[2], &json!({ "flag": true }))
        .unwrap();

    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let snapshot = runtime.snapshot(&rundown, at(10 * HOUR), 0);
    assert_eq!(snapshot["offset"]["expectedFlagStart"], 11 * HOUR);
    assert_eq!(snapshot["eventFlag"]["id"], ids[2].as_str());
}

#[test]
fn without_a_flag_or_a_group_those_stay_null() {
    let (rundown, _) = show();
    let mut runtime = Runtime::default();
    runtime.apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let snapshot = runtime.snapshot(&rundown, at(10 * HOUR), 0);
    assert!(snapshot["offset"]["expectedFlagStart"].is_null());
    assert!(snapshot["offset"]["expectedGroupEnd"].is_null());
    assert!(snapshot["eventFlag"].is_null());
}

#[test]
fn switching_rundown_stops_playback() {
    use ontime_core::api::{handle, ApiRequest, HttpMethod};
    use ontime_core::project::Project;
    use ontime_core::runtime::Playback;

    let mut project = Project::default();

    // a second rundown to switch to
    let created = handle(
        &mut project,
        &ApiRequest {
            method: HttpMethod::Post,
            path: "/data/rundowns".into(),
            body: json!({ "title": "Second" }),
            token: None,
            master_token: None,
        },
    );
    let other = created.body.unwrap()["rundowns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["title"] == "Second")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // start the loaded rundown
    let rundown = project.current().cloned().unwrap();
    project
        .runtime
        .apply(Command::Start, &rundown, at(10 * HOUR), 0);
    assert_eq!(project.runtime.playback, Playback::Play);

    handle(
        &mut project,
        &ApiRequest {
            method: HttpMethod::Post,
            path: format!("/data/rundowns/{other}/load"),
            body: json!({}),
            token: None,
            master_token: None,
        },
    );

    // leaving it running would report `play` with nothing playing
    assert_eq!(project.runtime.playback, Playback::Stop);
    assert!(project.runtime.selected_id.is_none());
}
