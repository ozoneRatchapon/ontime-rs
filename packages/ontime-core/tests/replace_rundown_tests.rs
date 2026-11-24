//! Upstream 51801f91: replace a rundown in place without stopping playback.
//! Here it is `PUT /data/rundowns/:id { title?, entries: [...] }`; upstream only
//! reaches it through import, which the cloud backend does not offer.

use ontime_core::api::{apply_socket_command, handle, ApiRequest, Effect, HttpMethod};
use ontime_core::project::Project;
use ontime_core::runtime::Playback;
use serde_json::{json, Value};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY_START: i64 = 1_700_000_000_000 - (1_700_000_000_000 % 86_400_000);
const NOW: i64 = DAY_START + 10 * HOUR;

fn request(method: HttpMethod, path: &str, body: Value) -> ApiRequest {
    ApiRequest {
        method,
        path: path.into(),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    }
}

fn event(id: &str, title: &str, start_minute: i64) -> Value {
    json!({ "id": id, "type": "event", "title": title, "timeStart": 10 * HOUR + start_minute * MINUTE, "duration": 10 * MINUTE })
}

/// A room with rundown `show` (a, b, c) loaded and `a` playing, plus a background `other`.
fn playing_room() -> Project {
    let mut project = Project::default();
    for (id, title) in [("show", "Show"), ("other", "Other")] {
        let body = json!({ "title": title, "entries": [event("a", "A", 0), event("b", "B", 10), event("c", "C", 20)] });
        project
            .rundowns
            .insert(id.into(), ontime_core::Rundown::new(id, title));
        let response = handle(
            &mut project,
            &request(HttpMethod::Put, &format!("/data/rundowns/{id}"), body),
            NOW,
        );
        assert_eq!(response.status, 200);
    }
    project.loaded = "show".into();
    project.ensure_consistent();
    apply_socket_command(&mut project, "start", None, NOW);
    assert_eq!(project.runtime.playback, Playback::Play);
    project
}

fn replace(project: &mut Project, id: &str, body: Value) -> (u16, Vec<Effect>) {
    let response = handle(
        project,
        &request(HttpMethod::Put, &format!("/data/rundowns/{id}"), body),
        NOW + MINUTE,
    );
    (response.status, response.effects)
}

fn refetched(effects: &[Effect]) -> Vec<(String, Option<String>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Refetch { target, rundown_id } => Some((target.clone(), rundown_id.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn keeps_playing_when_the_loaded_event_survives() {
    let mut project = playing_room();
    let revision = project.rundowns["show"].revision;
    let (status, effects) = replace(
        &mut project,
        "show",
        json!({ "entries": [event("a", "Replaced", 0), event("c", "C", 10)] }),
    );

    assert_eq!(status, 200);
    assert_eq!(project.runtime.playback, Playback::Play);
    assert_eq!(project.runtime.selected_id.as_deref(), Some("a"));
    let show = &project.rundowns["show"];
    assert_eq!(show.get("a").unwrap()["title"], "Replaced");
    assert_eq!(show.order, vec!["a", "c"]);
    assert!(show.revision > revision);
    assert_eq!(show.title, "Show", "the title stays unless one is given");
    assert!(refetched(&effects).contains(&("rundown".into(), Some("show".into()))));
}

#[test]
fn stops_when_the_loaded_event_is_gone() {
    let mut project = playing_room();
    let (status, _) = replace(
        &mut project,
        "show",
        json!({ "entries": [event("b", "B", 10)] }),
    );
    assert_eq!(status, 200);
    assert_eq!(project.runtime.playback, Playback::Stop);
    assert_eq!(project.runtime.selected_id, None);
    let (_, effects) = replace(
        &mut playing_room(),
        "show",
        json!({ "entries": [event("b", "B", 10)] }),
    );
    assert!(
        effects.iter().any(|e| matches!(e, Effect::Reschedule)),
        "the stop is rescheduled"
    );
}

#[test]
fn stops_when_the_loaded_event_is_now_skipped() {
    let mut project = playing_room();
    let mut skipped = event("a", "A", 0);
    skipped["skip"] = json!(true);
    replace(
        &mut project,
        "show",
        json!({ "entries": [skipped, event("b", "B", 10)] }),
    );
    assert_eq!(project.runtime.playback, Playback::Stop);
}

#[test]
fn a_background_rundown_leaves_playback_alone() {
    let mut project = playing_room();
    let (status, _) = replace(
        &mut project,
        "other",
        json!({ "title": "Other v2", "entries": [event("x", "X", 0)] }),
    );
    assert_eq!(status, 200);
    assert_eq!(project.runtime.playback, Playback::Play);
    assert_eq!(project.runtime.selected_id.as_deref(), Some("a"));
    assert_eq!(project.loaded, "show");
    assert_eq!(project.rundowns["other"].title, "Other v2");
    assert_eq!(project.rundowns["other"].order, vec!["x"]);
}

#[test]
fn the_rundown_list_is_refetched_only_when_it_shows_a_change() {
    let mut project = playing_room();
    let (_, effects) = replace(
        &mut project,
        "show",
        json!({ "entries": [event("a", "A2", 0), event("b", "B", 10), event("c", "C", 20)] }),
    );
    assert!(!refetched(&effects)
        .iter()
        .any(|(t, _)| t == "project-rundowns"));
    let (_, effects) = replace(
        &mut project,
        "show",
        json!({ "entries": [event("a", "A", 0)] }),
    );
    assert!(refetched(&effects)
        .iter()
        .any(|(t, _)| t == "project-rundowns"));
}

#[test]
fn a_duplicate_id_keeps_its_first_place() {
    let mut project = playing_room();
    replace(
        &mut project,
        "show",
        json!({ "entries": [event("a", "First", 0), event("a", "Second", 10)] }),
    );
    let show = &project.rundowns["show"];
    assert_eq!(show.get("a").unwrap()["title"], "First");
    assert_eq!(
        show.order.len(),
        2,
        "the second gets a fresh id rather than overwriting"
    );
}

#[test]
fn report_records_of_dropped_entries_are_cleared() {
    let mut project = playing_room();
    project.report.on_start("b", 0);
    replace(
        &mut project,
        "show",
        json!({ "entries": [event("a", "A", 0)] }),
    );
    assert!(project.report.get("b").is_none());
}

#[test]
fn refuses_an_unknown_rundown_and_creates_nothing() {
    let mut project = playing_room();
    let (status, effects) = replace(&mut project, "missing", json!({ "entries": [] }));
    assert_eq!(status, 404);
    assert!(effects.is_empty());
    assert!(!project.rundowns.contains_key("missing"));
}

#[test]
fn refuses_a_body_without_an_entry_list() {
    let mut project = playing_room();
    let before = project.rundowns["show"].order.clone();
    for body in [
        json!({}),
        json!({ "entries": "a" }),
        json!({ "entries": [1] }),
    ] {
        let (status, _) = replace(&mut project, "show", body);
        assert_eq!(status, 400);
    }
    assert_eq!(project.rundowns["show"].order, before);
    assert_eq!(project.runtime.playback, Playback::Play);
}

// --- the same reconciliation after any edit of the loaded rundown (upstream e951f8a7) ---

fn call(project: &mut Project, method: HttpMethod, path: &str, body: Value) -> (u16, Vec<Effect>) {
    let response = handle(project, &request(method, path, body), NOW + MINUTE);
    (response.status, response.effects)
}

#[test]
fn deleting_the_playing_entry_stops_playback() {
    let mut project = playing_room();
    let (status, effects) = call(
        &mut project,
        HttpMethod::Delete,
        "/data/rundowns/show/entries",
        json!({ "ids": ["a"] }),
    );
    assert_eq!(status, 200);
    assert_eq!(project.runtime.playback, Playback::Stop);
    assert!(effects.iter().any(|e| matches!(e, Effect::Reschedule)));
}

#[test]
fn skipping_the_playing_entry_stops_playback() {
    let mut project = playing_room();
    call(
        &mut project,
        HttpMethod::Put,
        "/data/rundowns/show/entry",
        json!({ "id": "a", "skip": true }),
    );
    assert_eq!(project.runtime.playback, Playback::Stop);
}

#[test]
fn clearing_the_loaded_rundown_stops_playback() {
    let mut project = playing_room();
    call(
        &mut project,
        HttpMethod::Delete,
        "/data/rundowns/show/all",
        Value::Null,
    );
    assert_eq!(project.runtime.playback, Playback::Stop);
}

#[test]
fn deleting_another_entry_keeps_playing() {
    let mut project = playing_room();
    call(
        &mut project,
        HttpMethod::Delete,
        "/data/rundowns/show/entries",
        json!({ "ids": ["b"] }),
    );
    assert_eq!(project.runtime.playback, Playback::Play);
}

#[test]
fn deleting_in_a_background_rundown_keeps_playing() {
    let mut project = playing_room();
    call(
        &mut project,
        HttpMethod::Delete,
        "/data/rundowns/other/entries",
        json!({ "ids": ["a"] }),
    );
    assert_eq!(project.runtime.playback, Playback::Play);
}
