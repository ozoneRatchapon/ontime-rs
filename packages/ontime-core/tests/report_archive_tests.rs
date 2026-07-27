// The report archive is the permanent, never-overwritten history of runs.
//
// The model these pin: switching rundown is NON-DESTRUCTIVE — it stops playback
// but leaves the report intact, so an accidental switch or a peek at another day
// does not lose the run. Preserving a finished run is an EXPLICIT action
// (POST /data/report/archive), which snapshots it and clears the live report for
// a clean next run. The archive itself is append-only and never overwritten.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use ontime_core::rundown::Insert;
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

/// Two rundowns, each with one known event; returns their ids.
fn two_day_project() -> (Project, String, String) {
    let mut project = Project::default();
    let day1 = project.loaded.clone();

    // the default rundown ships with demo events; clear them so the fixture holds
    // exactly the one known event
    project.get_mut(&day1).unwrap().delete_all();
    project.get_mut(&day1).unwrap().add_entry(
        &json!({ "title": "opening", "duration": 60_000 }),
        Insert::End,
    );

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
        Insert::End,
    );

    (project, day1, day2)
}

fn record_run(project: &mut Project, rundown_id: &str, index: usize, start: i64, end: i64) {
    let id = project.rundowns[rundown_id].flat_order[index].clone();
    project.report.on_start(&id, start);
    project.report.on_stop(&id, end);
}

fn live_report_len(project: &Project) -> usize {
    serde_json::to_value(&project.report)
        .unwrap()
        .as_object()
        .map(|o| o.len())
        .unwrap_or(0)
}

#[test]
fn switching_rundown_does_not_archive_or_clear_the_report() {
    let (mut project, day1, day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );

    // switch to day 2 — the run is NOT finished, this could be an accidental tap
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day2}/load"),
            json!({}),
        ),
        T0,
    );

    // nothing archived, and day 1's record is still in the live report intact —
    // so coming back and resuming continues the same run rather than fragmenting it
    assert_eq!(project.report_archive.len(), 0);
    assert_eq!(live_report_len(&project), 1);

    // and switching back leaves it still there
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day1}/load"),
            json!({}),
        ),
        T0,
    );
    assert_eq!(live_report_len(&project), 1);
    assert_eq!(project.report_archive.len(), 0);
}

#[test]
fn resuming_after_a_switch_keeps_one_coherent_run() {
    let (mut project, day1, day2) = two_day_project();
    // add a second event to day 1
    project.get_mut(&day1).unwrap().add_entry(
        &json!({ "title": "act two", "duration": 60_000 }),
        Insert::End,
    );

    // ran the first event, then someone switched away mid-show and came back
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 60_000,
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
    handle(
        &mut project,
        &req(
            HttpMethod::Post,
            &format!("/data/rundowns/{day1}/load"),
            json!({}),
        ),
        T0,
    );

    // resume: run the second event
    record_run(
        &mut project,
        &day1,
        1,
        10 * 3_600_000,
        10 * 3_600_000 + 60_000,
    );

    // now finish and save — the whole run is one record, both events present
    handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );
    assert_eq!(project.report_archive.len(), 1);
    let titles: Vec<&str> = project.report_archive.runs()[0]
        .events
        .iter()
        .map(|e| e.title.as_str())
        .collect();
    assert_eq!(titles, vec!["opening", "act two"]);
}

#[test]
fn an_explicit_snapshot_preserves_and_clears_the_run() {
    let (mut project, day1, _day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );

    let resp = handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );
    assert_eq!(resp.body.unwrap()["archived"], true);

    assert_eq!(project.report_archive.len(), 1);
    assert_eq!(project.report_archive.runs()[0].events[0].title, "opening");
    // cleared, so the next run starts fresh and cannot be double-archived
    assert_eq!(live_report_len(&project), 0);
}

#[test]
fn an_archived_run_survives_the_rundown_being_deleted() {
    let (mut project, day1, _day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );

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

    let run = &project.report_archive.runs()[0];
    assert_eq!(run.events[0].title, "opening");
    assert_eq!(run.events[0].planned_duration, 60_000);
}

#[test]
fn re_running_the_same_rundown_keeps_both_runs_when_each_is_saved() {
    let (mut project, day1, _day2) = two_day_project();

    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );

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
fn a_snapshot_with_nothing_to_save_is_a_no_op() {
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
    let (mut project, day1, _day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
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
    let (mut project, day1, _day2) = two_day_project();
    project.get_mut(&day1).unwrap().add_entry(
        &json!({ "title": "never ran", "duration": 30_000 }),
        Insert::End,
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
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );
    let run = &project.report_archive.runs()[0];
    assert_eq!(run.events.len(), 1);
    assert_eq!(run.events[0].title, "opening");
}

#[test]
fn the_archive_survives_a_serialisation_round_trip() {
    let (mut project, day1, _day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    handle(
        &mut project,
        &req(HttpMethod::Post, "/data/report/archive", json!({})),
        T0,
    );

    let raw = serde_json::to_string(&project).unwrap();
    let restored: Project = serde_json::from_str(&raw).unwrap();
    assert_eq!(restored.report_archive.len(), 1);
    assert_eq!(restored.report_archive.runs()[0].events[0].title, "opening");
}

#[test]
fn deleting_an_entry_clears_its_orphaned_report_record() {
    let (mut project, day1, _day2) = two_day_project();
    let entry = project.rundowns[&day1].flat_order[0].clone();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    assert_eq!(live_report_len(&project), 1);

    handle(
        &mut project,
        &req(
            HttpMethod::Delete,
            &format!("/data/rundowns/{day1}/entries"),
            json!({ "ids": [entry] }),
        ),
        T0,
    );

    // the entry is gone and its now-meaningless report record goes with it,
    // instead of lingering under an id that points at nothing
    assert_eq!(live_report_len(&project), 0);
    assert_eq!(project.report_archive.len(), 0);
}

#[test]
fn deleting_a_rundown_clears_its_report_records() {
    let (mut project, day1, _day2) = two_day_project();
    record_run(
        &mut project,
        &day1,
        0,
        9 * 3_600_000,
        9 * 3_600_000 + 55_000,
    );
    assert_eq!(live_report_len(&project), 1);

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
    assert_eq!(live_report_len(&project), 0);
}
