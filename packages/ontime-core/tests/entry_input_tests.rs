//! Upstream 9abf9d00 and its integration test: input that does not apply is
//! refused with a 4xx and leaves the rundown untouched. Plus the placement of a
//! new entry inside a group, which the client asks for with `parent`.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use ontime_core::rundown::{Insert, Rundown};
use serde_json::{json, Value};

/// Rundown `show`: event `a`, group `g` holding event `in`, event `b`, delay `d`.
fn room() -> Project {
    let mut project = Project::default();
    let mut rundown = Rundown::new("show", "Show");
    for payload in [
        json!({ "id": "a", "type": "event" }),
        json!({ "id": "g", "type": "group" }),
        json!({ "id": "in", "type": "event" }),
        json!({ "id": "b", "type": "event" }),
        json!({ "id": "d", "type": "delay", "duration": 60000 }),
    ] {
        rundown.add_entry(&payload, Insert::End);
    }
    assert!(rundown.reorder("in", "g", "insert"));
    assert_eq!(rundown.order, ["a", "g", "b", "d"]);
    project.rundowns.insert("show".into(), rundown);
    project.loaded = "show".into();
    project.ensure_consistent();
    project
}

fn call(project: &mut Project, method: HttpMethod, path: &str, body: Value) -> (u16, String) {
    let req = ApiRequest {
        method,
        path: format!("/data/rundowns/show{path}"),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    };
    let response = handle(project, &req, 0);
    let message = response
        .body
        .as_ref()
        .and_then(|b| b.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (response.status, message)
}

fn group_entries(project: &Project) -> Value {
    project.rundowns["show"].get("g").unwrap()["entries"].clone()
}

// --- additions -------------------------------------------------------------

#[test]
fn refuses_invalid_additions_and_leaves_the_rundown_untouched() {
    let mut project = room();
    let before = serde_json::to_value(&project.rundowns["show"]).unwrap();

    let results = [
        call(
            &mut project,
            HttpMethod::Post,
            "/entry",
            json!({ "type": "event", "id": "a", "title": "again" }),
        ),
        call(
            &mut project,
            HttpMethod::Post,
            "/entry",
            json!({ "type": "event", "title": "orphan", "parent": "a" }),
        ),
        call(
            &mut project,
            HttpMethod::Post,
            "/entry",
            json!({ "type": "event", "title": "lost", "parent": "missing" }),
        ),
        call(
            &mut project,
            HttpMethod::Post,
            "/entry",
            json!({ "type": "group", "title": "nested", "parent": "g" }),
        ),
    ];
    assert_eq!(
        results,
        [
            (409, "Entry with ID a already exists".to_string()),
            (422, "Invalid parent event with ID a".to_string()),
            (422, "Invalid parent event with ID missing".to_string()),
            (422, "Cannot add a group to another group".to_string()),
        ]
    );
    assert_eq!(
        serde_json::to_value(&project.rundowns["show"]).unwrap(),
        before
    );
}

#[test]
fn a_parent_without_an_anchor_puts_the_entry_at_the_end_of_that_group() {
    let mut project = room();
    let (status, _) = call(
        &mut project,
        HttpMethod::Post,
        "/entry",
        json!({ "id": "new", "type": "event", "parent": "g" }),
    );
    assert_eq!(status, 201);
    assert_eq!(group_entries(&project), json!(["in", "new"]));
    let show = &project.rundowns["show"];
    assert_eq!(show.get("new").unwrap()["parent"], "g");
    assert!(
        !show.order.contains(&"new".to_string()),
        "not also at the top level"
    );
}

#[test]
fn a_parent_with_before_true_puts_the_entry_first_in_that_group() {
    let mut project = room();
    call(
        &mut project,
        HttpMethod::Post,
        "/entry",
        json!({ "id": "new", "type": "event", "parent": "g", "before": true }),
    );
    assert_eq!(group_entries(&project), json!(["new", "in"]));
}

#[test]
fn an_anchor_inside_the_group_places_it_next_to_the_anchor() {
    let mut project = room();
    call(
        &mut project,
        HttpMethod::Post,
        "/entry",
        json!({ "id": "new", "type": "event", "parent": "g", "before": "in" }),
    );
    assert_eq!(group_entries(&project), json!(["new", "in"]));
}

#[test]
fn a_top_level_entry_never_claims_a_parent() {
    let mut project = room();
    call(
        &mut project,
        HttpMethod::Post,
        "/entry",
        json!({ "id": "new", "type": "event", "parent": null, "after": "a" }),
    );
    let show = &project.rundowns["show"];
    assert_eq!(show.get("new").unwrap()["parent"], Value::Null);
    assert_eq!(show.order[..2], ["a".to_string(), "new".to_string()]);
}

// --- operations that do not apply ------------------------------------------

#[test]
fn refuses_operations_which_do_not_apply_leaving_the_rundown_untouched() {
    let mut project = room();
    let before = serde_json::to_value(&project.rundowns["show"]).unwrap();

    let results = [
        call(
            &mut project,
            HttpMethod::Put,
            "/entry",
            json!({ "id": "missing", "title": "ghost" }),
        ),
        call(
            &mut project,
            HttpMethod::Put,
            "/entry",
            json!({ "id": "a", "type": "delay" }),
        ),
        call(
            &mut project,
            HttpMethod::Patch,
            "/reorder",
            json!({ "entryId": "a", "destinationId": "missing", "order": "after" }),
        ),
        call(
            &mut project,
            HttpMethod::Patch,
            "/applydelay/a",
            Value::Null,
        ),
        call(
            &mut project,
            HttpMethod::Patch,
            "/applydelay/missing",
            Value::Null,
        ),
        call(
            &mut project,
            HttpMethod::Post,
            "/ungroup/missing",
            Value::Null,
        ),
        call(&mut project, HttpMethod::Post, "/ungroup/a", Value::Null),
    ];
    assert_eq!(
        results.clone().map(|(status, _)| status),
        [404, 422, 404, 422, 404, 404, 422],
        "{results:?}"
    );
    assert_eq!(
        serde_json::to_value(&project.rundowns["show"]).unwrap(),
        before
    );
}

#[test]
fn an_edit_that_repeats_the_type_is_fine() {
    let mut project = room();
    let (status, _) = call(
        &mut project,
        HttpMethod::Put,
        "/entry",
        json!({ "id": "a", "type": "event", "title": "A" }),
    );
    assert_eq!(status, 200);
}
