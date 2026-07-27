// The report archive is the permanent, never-overwritten history of runs.
// These pin the guarantees that matter: a real run is preserved, it survives the
// rundown being edited or deleted, and switching days does not lose the day left.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use serde_json::{json, Value};

const T0: i64 = 1_700_000_000_000;

fn req(method: HttpMethod, path: &str, body: Value) -> ApiRequest {
    ApiRequest {
        method,
        path: path.into(),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
    }
}

/// A project with two rundowns, each holding one event, and returns their ids.
fn two_day_project() -> (Project, String, String) {
    let mut project = Project::default();
    let day1 = project.loaded.clone();

    // the default rundown ships with demo events; clear them so the fixture holds
    // exactly the one known event
    project.get_mut(&day1).unwrap().delete_all();
    project.get_mut(&day1).unwrap().add_entry(
        &json!({ "title": "opening", "duration": 60_000 }),
        ontime_core::rundown::Insert::End,
    );

    // create day 2
    let created = handle(
        &mut project,
        &req(
            HttpMethod::Post,
            "/data/rundowns",
            json!({ "title": "Day 2" }),
        ),
        T0,
    );
    let day2 = created.body.unwrap()["rundowns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["title"] == "Day 2")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    project.get_mut(&day2).unwrap().add_entry(
        &json!({ "title": "keynote", "duration": 60_000 }),
        ontime_core::rundown::Insert::End,
    );

    (project, day1, day2)
}

/// Marks an event as having run, in the live report.
fn record_run(project: &mut Project, rundown_id: &str, index: usize, start: i64, end: i64) {
    let id = project.rundowns[rundown_id].flat_order[index].clone();
    project.report.on_start(&id, start);
    project.report.on_stop(&id, end);
}

#[test]
fn switching_rundown_archives_the_run_you_are_leaving() {
    let (mut project, day1, day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );

    // move to day 2 — day 1's run must be preserved automatically
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );

    assert_eq!(project.report_archive.len(), 1);
    let run = &project.report_archive.runs()[0];
    assert_eq!(run.rundown_id, day1);
    assert_eq!(run.events.len(), 1);
    assert_eq!(run.events[0].title, "opening");
    assert_eq!(run.events[0].started_at, Some(9 * 3_600_000));

    // and it is out of the live report, so it cannot be archived twice
    let live: Value = serde_json::to_value(&project.report).unwrap();
    assert!(live.as_object().unwrap().is_empty());
}

#[test]
fn an_archived_run_survives_the_rundown_being_deleted() {
    let (mut project, day1, day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );

    // delete day 1 entirely
    handle(
        &mut project,
        &req(
            HttpMethod::Delete,
            &format!("/data/rundowns/{day1}"),
            json!({}),
        ),
        T0,
    );
    assert!(!project.rundowns.contains_key(&day1));

    // the archived run still has its title and planned duration — the whole point
    let run = &project.report_archive.runs()[0];
    assert_eq!(run.events[0].title, "opening");
    assert_eq!(run.events[0].planned_duration, 60_000);
}

#[test]
fn re_running_the_same_rundown_can_be_preserved_with_a_manual_snapshot() {
    let (mut project, day1, _day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );

    // operator saves the run before redoing it
    let resp = handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );
    assert_eq!(resp.body.unwrap()["archived"], true);
    assert_eq!(project.report_archive.len(), 1);

    // a second run of the same rundown, then save again → two distinct records
    record_run(
        &mut project,
        &day1,
        0,
        14 * 3_600_000,
        14 * 3_600_000 + 70_000,
    );
    handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );
    assert_eq!(project.report_archive.len(), 2);
    assert_eq!(
        project.report_archive.runs()[0].events[0].started_at,
        Some(9 * 3_600_000)
    );
    assert_eq!(
        project.report_archive.runs()[1].events[0].started_at,
        Some(14 * 3_600_000)
    );
}

#[test]
fn switching_between_rundowns_that_never_ran_does_not_litter_the_archive() {
    let (mut project, _day1, day2) = two_day_project();
    // nothing ran; switching should archive nothing
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );
    assert_eq!(project.report_archive.len(), 0);
}

#[test]
fn a_manual_snapshot_with_nothing_to_save_is_a_no_op() {
    let (mut project, _day1, _day2) = two_day_project();
    let resp = handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );
    assert_eq!(resp.body.unwrap()["archived"], false);
    assert_eq!(project.report_archive.len(), 0);
}

#[test]
fn the_archive_can_be_read_back() {
    let (mut project, day1, day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );

    let resp = handle(
        &mut project,
        &req(HttpMethod::Get, "/data/report/archive", json!({})),
        T0,
    );
    let runs = resp.body.unwrap();
    assert!(runs.is_array());
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(runs[0]["rundownTitle"], "Default"); // day1 is the default rundown
    assert_eq!(runs[0]["events"][0]["title"], "opening");
}

#[test]
fn only_events_that_actually_started_are_archived() {
    let (mut project, day1, day2) = two_day_project();
    // add a second event to day 1 but only run the first
    project.get_mut(&day1).unwrap().add_entry(
        &json!({ "title": "never ran", "duration": 30_000 }),
        ontime_core::rundown::Insert::End,
    );
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );

    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );
    let run = &project.report_archive.runs()[0];
    assert_eq!(run.events.len(), 1);
    assert_eq!(run.events[0].title, "opening");
}

#[test]
fn the_archive_survives_a_serialisation_round_trip() {
    let (mut project, day1, day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );

    let raw = serde_json::to_string(&project).unwrap();
    let restored: Project = serde_json::from_str(&raw).unwrap();
    assert_eq!(restored.report_archive.len(), 1);
    assert_eq!(restored.report_archive.runs()[0].events[0].title, "opening");
}
