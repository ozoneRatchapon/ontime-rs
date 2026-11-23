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
            room_is_new: false,
            member: None,
        },
        0,
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
            room_is_new: false,
            member: None,
        },
        0,
    );

    // leaving it running would report `play` with nothing playing
    assert_eq!(project.runtime.playback, Playback::Stop);
    assert!(project.runtime.selected_id.is_none());
}

// --- what a room costs when nobody is watching -----------------------------

#[test]
fn an_unwatched_room_does_not_wake_on_the_heartbeat() {
    use ontime_core::api::next_wake_in;
    use ontime_core::project::Project;

    let mut project = Project::default();
    let rundown = project.current().cloned().unwrap();
    project
        .runtime
        .apply(Command::Start, &rundown, at(10 * HOUR), 0);

    let now = at(10 * HOUR);

    // with someone connected, the heartbeat keeps the display current
    assert_eq!(
        next_wake_in(&project, now, true),
        Some(project.heartbeat_ms)
    );

    // with nobody connected there is nothing to broadcast to, and the default
    // event has no end action, so the room falls silent entirely
    assert_eq!(next_wake_in(&project, now, false), None);
}

#[test]
fn an_unwatched_room_still_wakes_to_advance_the_rundown() {
    use ontime_core::api::next_wake_in;
    use ontime_core::project::Project;
    use serde_json::json as j;

    let mut project = Project::default();
    let rundown_id = project.loaded.clone();
    let first = project.current().unwrap().flat_order[0].clone();
    project
        .get_mut(&rundown_id)
        .unwrap()
        .edit_entry(&first, &j!({ "endAction": "play-next" }))
        .unwrap();

    let rundown = project.current().cloned().unwrap();
    project
        .runtime
        .apply(Command::StartId(first), &rundown, at(10 * HOUR), 0);

    // an end action has to fire whether or not anyone is looking, but it is one
    // wake per event rather than one a second
    let unwatched = next_wake_in(&project, at(10 * HOUR), false);
    assert!(unwatched.is_some());
    assert!(
        unwatched.unwrap() > project.heartbeat_ms,
        "woke on the heartbeat rather than at the end of the event"
    );
}

#[test]
fn a_paused_or_aux_running_room_is_silent_without_a_watcher() {
    use ontime_core::api::next_wake_in;
    use ontime_core::project::Project;
    use serde_json::json as j;

    let mut project = Project::default();
    let rundown = project.current().cloned().unwrap();
    project
        .runtime
        .apply(Command::Start, &rundown, at(10 * HOUR), 0);
    project
        .runtime
        .apply(Command::Pause, &rundown, at(10 * HOUR + MINUTE), 0);

    assert!(next_wake_in(&project, at(10 * HOUR + MINUTE), true).is_some());
    assert_eq!(next_wake_in(&project, at(10 * HOUR + MINUTE), false), None);

    project
        .aux_timers
        .apply(&j!({ "1": "start" }), at(10 * HOUR));
    assert!(next_wake_in(&project, at(10 * HOUR + MINUTE), true).is_some());
    assert_eq!(next_wake_in(&project, at(10 * HOUR + MINUTE), false), None);
}

// --- who may bring a room into existence -----------------------------------

#[test]
fn any_room_may_be_created_when_no_master_password_is_configured() {
    use ontime_core::api::{may_create_room, ApiRequest, HttpMethod};

    let request = ApiRequest {
        method: HttpMethod::Get,
        path: "/data/settings".into(),
        body: json!({}),
        token: None,
        master_token: None,
        room_is_new: true,
        member: None,
    };
    // a single operator, or a LAN install, should not need ceremony
    assert!(may_create_room(&request));
}

#[test]
fn creating_a_room_needs_the_master_password_once_one_is_configured() {
    use ontime_core::api::{may_create_room, ApiRequest, HttpMethod};
    use ontime_core::project::hash_password;

    let master = hash_password("the-master");
    let request = |token: Option<&str>, room_is_new: bool| ApiRequest {
        method: HttpMethod::Get,
        path: "/data/settings".into(),
        body: json!({}),
        token: token.map(str::to_string),
        master_token: Some(master.clone()),
        room_is_new,
        member: None,
    };

    assert!(!may_create_room(&request(None, true)));
    assert!(!may_create_room(&request(Some("wrong"), true)));
    assert!(may_create_room(&request(Some(&master), true)));

    // rooms that already exist are untouched, so turning this on cannot lock
    // anyone out of what they already have
    assert!(may_create_room(&request(None, false)));
}

// --- custom fields -------------------------------------------------------

#[test]
fn a_custom_field_is_keyed_the_way_the_client_expects() {
    use ontime_core::api::{handle, ApiRequest, HttpMethod};
    use ontime_core::project::Project;

    let mut project = Project::default();
    let create = |project: &mut Project, label: &str| {
        handle(
            project,
            &ApiRequest {
                method: HttpMethod::Post,
                path: "/data/custom-fields".into(),
                body: json!({ "type": "text", "colour": "", "label": label }),
                token: None,
                master_token: None,
                room_is_new: false,
                member: None,
            },
            0,
        )
    };

    // spaces become underscores, as customFieldLabelToKey does
    let response = create(&mut project, "Mic 2");
    assert_eq!(response.status, 200);
    assert!(project.custom_fields.get("Mic_2").is_some());

    // the original only accepts letters, numbers and spaces; accepting more
    // would create fields the real client cannot handle
    assert_eq!(create(&mut project, "สถานที่").status, 400);
    assert_eq!(create(&mut project, "").status, 400);
    assert_eq!(create(&mut project, "with-hyphen").status, 400);
}
