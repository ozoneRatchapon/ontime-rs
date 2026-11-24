//! Upstream 84b28083 / 98c1832f: aux timers can be named, through the settings.
//! Plus: a bad settings patch is refused instead of resetting every setting.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::aux_timer::{sanitise_aux_timer_names, AUX_TIMER_NAME_MAX_LENGTH};
use ontime_core::project::{Project, Settings};
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

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

fn post_settings(project: &mut Project, body: Value) -> (u16, Value) {
    let response = handle(
        project,
        &request(HttpMethod::Post, "/data/settings", body),
        NOW,
    );
    (response.status, response.body.unwrap_or(Value::Null))
}

// --- the names -----------------------------------------------------------------

#[test]
fn names_are_trimmed_and_capped() {
    let long = "x".repeat(AUX_TIMER_NAME_MAX_LENGTH + 5);
    assert_eq!(
        sanitise_aux_timer_names(&json!(["  Q&A  ", long, "Break"])),
        [
            "Q&A".to_string(),
            "x".repeat(AUX_TIMER_NAME_MAX_LENGTH),
            "Break".to_string()
        ]
    );
}

#[test]
fn names_cap_counts_characters_not_bytes() {
    let thai = "พักเบรก".repeat(10);
    let [first, _, _] = sanitise_aux_timer_names(&json!([thai]));
    assert_eq!(first.chars().count(), AUX_TIMER_NAME_MAX_LENGTH);
}

#[test]
fn missing_or_malformed_names_become_empty() {
    let empty = [String::new(), String::new(), String::new()];
    assert_eq!(sanitise_aux_timer_names(&Value::Null), empty);
    assert_eq!(sanitise_aux_timer_names(&json!("Q&A")), empty);
    assert_eq!(
        sanitise_aux_timer_names(&json!([1, null, "Reset", "extra"])),
        [String::new(), String::new(), "Reset".to_string()]
    );
}

#[test]
fn old_saved_settings_load_with_unnamed_timers() {
    let saved = json!({ "version": "4.11.0", "editorKey": null, "operatorKey": null, "timeFormat": "24", "language": "en" });
    let settings: Settings = serde_json::from_value(saved).unwrap();
    assert_eq!(
        settings.aux_timer_names,
        [String::new(), String::new(), String::new()]
    );
}

#[test]
fn each_timer_state_carries_its_name() {
    let mut project = Project::default();
    project.settings.aux_timer_names = ["Q&A".into(), String::new(), "Reset".into()];
    let snapshot = project
        .aux_timers
        .snapshot(NOW, &project.settings.aux_timer_names);
    assert_eq!(snapshot["auxtimer1"]["name"], "Q&A");
    assert_eq!(snapshot["auxtimer2"]["name"], "");
    assert_eq!(snapshot["auxtimer3"]["name"], "Reset");
}

// --- the settings endpoint -----------------------------------------------------

#[test]
fn settings_post_stores_clean_names() {
    let mut project = Project::default();
    let (status, body) =
        post_settings(&mut project, json!({ "auxTimerNames": [" Q&A ", "Break"] }));
    assert_eq!(status, 200);
    assert_eq!(body["auxTimerNames"], json!(["Q&A", "Break", ""]));
    assert_eq!(project.settings.aux_timer_names[0], "Q&A");
}

#[test]
fn settings_get_includes_names() {
    let mut project = Project::default();
    project.settings.aux_timer_names = ["A".into(), "B".into(), "C".into()];
    let response = handle(
        &mut project,
        &request(HttpMethod::Get, "/data/settings", Value::Null),
        NOW,
    );
    assert_eq!(
        response.body.unwrap()["auxTimerNames"],
        json!(["A", "B", "C"])
    );
}

#[test]
fn names_that_are_not_a_list_are_refused() {
    let mut project = Project::default();
    project.settings.aux_timer_names = ["Keep".into(), String::new(), String::new()];
    let (status, _) = post_settings(&mut project, json!({ "auxTimerNames": "Q&A" }));
    assert_eq!(status, 400);
    assert_eq!(project.settings.aux_timer_names[0], "Keep");
}

#[test]
fn a_badly_typed_patch_is_refused_and_changes_nothing() {
    let mut project = Project::default();
    project.settings.language = "th".into();
    project.settings.time_format = "12".into();
    let (status, body) = post_settings(&mut project, json!({ "timeFormat": 24 }));
    assert_eq!(status, 400);
    assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert_eq!(project.settings.language, "th");
    assert_eq!(project.settings.time_format, "12");
}

#[test]
fn a_refused_patch_does_not_half_apply_the_room_fields() {
    let mut project = Project::default();
    let (status, _) = post_settings(
        &mut project,
        json!({ "timezoneOffset": 25_200_000, "timeFormat": 24 }),
    );
    assert_eq!(status, 400);
    assert_eq!(project.timezone_offset, 0);
}

// --- the same rule for project data and view settings -----------------------------

#[test]
fn a_badly_typed_project_patch_is_refused_and_changes_nothing() {
    let mut project = Project::default();
    project.project.title = "Exam".into();
    let response = handle(
        &mut project,
        &request(
            HttpMethod::Post,
            "/data/project",
            json!({ "description": 5 }),
        ),
        NOW,
    );
    assert_eq!(response.status, 400);
    assert_eq!(project.project.title, "Exam");
}

#[test]
fn a_good_project_patch_still_merges() {
    let mut project = Project::default();
    project.project.title = "Exam".into();
    let response = handle(
        &mut project,
        &request(
            HttpMethod::Post,
            "/data/project",
            json!({ "description": "Day 1" }),
        ),
        NOW,
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        (
            project.project.title.as_str(),
            project.project.description.as_str()
        ),
        ("Exam", "Day 1")
    );
}

#[test]
fn a_badly_typed_view_settings_patch_is_refused_and_changes_nothing() {
    let mut project = Project::default();
    let before = serde_json::to_value(&project.view_settings).unwrap();
    let key = before.as_object().unwrap().keys().next().unwrap().clone();
    let response = handle(
        &mut project,
        &request(
            HttpMethod::Post,
            "/data/view-settings",
            json!({ key: [1, 2] }),
        ),
        NOW,
    );
    assert_eq!(response.status, 400);
    assert_eq!(
        serde_json::to_value(&project.view_settings).unwrap(),
        before
    );
}

// upstream c6ecff7c: a settings write keeps the stored app version
#[test]
fn a_settings_write_keeps_the_stored_version() {
    let mut project = Project::default();
    let version = project.settings.version.clone();
    let (status, body) = post_settings(
        &mut project,
        json!({ "version": "0.0.1", "language": "de" }),
    );
    assert_eq!(status, 200);
    assert_eq!(project.settings.version, version);
    assert_eq!(body["version"], json!(version));
    assert_eq!(project.settings.language, "de");
}
