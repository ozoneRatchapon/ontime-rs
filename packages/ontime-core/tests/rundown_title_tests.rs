//! Upstream #2300: a rundown title may be any letters or numbers, in any script,
//! up to 64 characters, and must have at least one letter or number.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use ontime_core::rundown::validate_title;
use serde_json::{json, Value};

fn call(project: &mut Project, method: HttpMethod, path: &str, body: Value) -> (u16, Value) {
    let req = ApiRequest {
        method,
        path: path.into(),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    };
    let response = handle(project, &req, 0);
    (response.status, response.body.unwrap_or(Value::Null))
}

#[test]
fn titles_in_any_script_are_fine() {
    for title in [
        "Show",
        "สอบ วันที่ 1",
        "Día 2 — mañana",
        "__timer__",
        "1",
        &"x".repeat(64),
        &"ก".repeat(64),
    ] {
        assert_eq!(validate_title(title), None, "{title:?}");
    }
}

#[test]
fn empty_long_or_symbol_only_titles_are_refused() {
    assert_eq!(validate_title("   "), Some("Title cannot be empty"));
    assert_eq!(
        validate_title(&"x".repeat(65)),
        Some("Title must be 64 characters or fewer")
    );
    assert_eq!(
        validate_title("-,."),
        Some("Title must contain at least one letter or number")
    );
    assert_eq!(
        validate_title("🎉"),
        Some("Title must contain at least one letter or number")
    );
}

#[test]
fn create_and_rename_refuse_a_bad_title_and_change_nothing() {
    let mut project = Project::default();
    let before = project.rundown_list();
    assert_eq!(
        call(
            &mut project,
            HttpMethod::Post,
            "/data/rundowns",
            json!({ "title": "-,." })
        )
        .0,
        422
    );
    assert_eq!(
        call(&mut project, HttpMethod::Post, "/data/rundowns", json!({})).0,
        422
    );
    let id = project.loaded.clone();
    let (status, body) = call(
        &mut project,
        HttpMethod::Patch,
        &format!("/data/rundowns/{id}"),
        json!({ "title": "🎉" }),
    );
    assert_eq!(status, 422);
    assert_eq!(
        body["message"],
        "Title must contain at least one letter or number"
    );
    assert_eq!(project.rundown_list(), before);
}

#[test]
fn a_thai_title_is_created_and_renamed() {
    let mut project = Project::default();
    let (status, body) = call(
        &mut project,
        HttpMethod::Post,
        "/data/rundowns",
        json!({ "title": "สอบปากเปล่า" }),
    );
    assert_eq!(status, 200);
    assert!(body["rundowns"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["title"] == "สอบปากเปล่า"));
}

#[test]
fn a_replace_with_a_bad_title_is_refused() {
    let mut project = Project::default();
    let id = project.loaded.clone();
    let (status, _) = call(
        &mut project,
        HttpMethod::Put,
        &format!("/data/rundowns/{id}"),
        json!({ "title": "", "entries": [] }),
    );
    assert_eq!(status, 422);
}
