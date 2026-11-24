//! Upstream 39c6dcb5 and the custom field service: an entry only holds values
//! for fields the project has, and renaming or deleting a field carries its
//! values along everywhere.

use ontime_core::api::{handle, ApiRequest, Effect, HttpMethod};
use ontime_core::project::Project;
use ontime_core::rundown::{Insert, Rundown};
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

fn call(
    project: &mut Project,
    method: HttpMethod,
    path: &str,
    body: Value,
) -> (u16, Value, Vec<Effect>) {
    let req = ApiRequest {
        method,
        path: path.into(),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    };
    let response = handle(project, &req, NOW);
    (
        response.status,
        response.body.unwrap_or(Value::Null),
        response.effects,
    )
}

/// Fields `Mic` and `Camera`; rundowns `show` (loaded, entry `a`) and `other` (entry `b`),
/// both with values for each field.
fn room() -> Project {
    let mut project = Project {
        custom_fields: json!({
            "Mic": { "label": "Mic", "type": "text", "colour": "" },
            "Camera": { "label": "Camera", "type": "text", "colour": "" },
        }),
        ..Project::default()
    };
    for (rundown_id, entry_id) in [("show", "a"), ("other", "b")] {
        let mut rundown = Rundown::new(rundown_id, rundown_id);
        rundown.add_entry(
            &json!({ "id": entry_id, "type": "event", "custom": { "Mic": "1", "Camera": "2" } }),
            Insert::End,
        );
        project.rundowns.insert(rundown_id.into(), rundown);
    }
    project.loaded = "show".into();
    project.ensure_consistent();
    project
}

fn custom(project: &Project, rundown: &str, entry: &str) -> Value {
    project.rundowns[rundown].get(entry).unwrap()["custom"].clone()
}

fn refetched_rundowns(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Refetch { target, rundown_id } if target == "rundown" => rundown_id.clone(),
            _ => None,
        })
        .collect()
}

// --- values on entries ---------------------------------------------------------

#[test]
fn an_edit_drops_values_for_unknown_fields() {
    let mut project = room();
    let (status, body, _) = call(
        &mut project,
        HttpMethod::Put,
        "/data/rundowns/show/entry",
        json!({ "id": "a", "custom": { "Mic": "3", "Ghost": "x" } }),
    );
    assert_eq!(status, 200);
    assert_eq!(
        body["custom"],
        json!({ "Mic": "3" }),
        "the response shows what was stored"
    );
    assert_eq!(custom(&project, "show", "a"), json!({ "Mic": "3" }));
}

#[test]
fn a_batch_edit_drops_values_for_unknown_fields() {
    let mut project = room();
    call(
        &mut project,
        HttpMethod::Put,
        "/data/rundowns/show/batch",
        json!({ "ids": ["a"], "data": { "custom": { "Ghost": "x", "Camera": "9" } } }),
    );
    assert_eq!(custom(&project, "show", "a"), json!({ "Camera": "9" }));
}

#[test]
fn an_edit_without_custom_leaves_values_alone() {
    let mut project = room();
    // a value that predates the rule stays until that entry's custom values are edited
    project
        .rundowns
        .get_mut("show")
        .unwrap()
        .entries
        .get_mut("a")
        .unwrap()["custom"]["Old"] = json!("kept");
    call(
        &mut project,
        HttpMethod::Put,
        "/data/rundowns/show/entry",
        json!({ "id": "a", "title": "T" }),
    );
    assert_eq!(custom(&project, "show", "a")["Old"], "kept");
}

#[test]
fn a_new_entry_drops_values_for_unknown_fields() {
    let mut project = room();
    let (status, body, _) = call(
        &mut project,
        HttpMethod::Post,
        "/data/rundowns/show/entry",
        json!({ "type": "event", "custom": { "Mic": "1", "Ghost": "x" } }),
    );
    assert_eq!(status, 201);
    assert_eq!(body["custom"], json!({ "Mic": "1" }));
}

#[test]
fn a_replaced_rundown_drops_values_for_unknown_fields() {
    let mut project = room();
    call(
        &mut project,
        HttpMethod::Put,
        "/data/rundowns/show",
        json!({ "entries": [{ "id": "a", "type": "event", "custom": { "Ghost": "x", "Mic": "5" } }] }),
    );
    assert_eq!(custom(&project, "show", "a"), json!({ "Mic": "5" }));
}

// --- editing a field -------------------------------------------------------------

#[test]
fn renaming_a_field_moves_its_values_in_every_rundown() {
    let mut project = room();
    let (status, fields, effects) = call(
        &mut project,
        HttpMethod::Put,
        "/data/custom-fields/Mic",
        json!({ "label": "Mic 2" }),
    );
    assert_eq!(status, 200);
    assert!(fields.get("Mic").is_none());
    assert_eq!(fields["Mic_2"]["label"], "Mic 2");
    assert_eq!(
        fields["Mic_2"]["type"], "text",
        "the rest of the field is kept"
    );
    for (rundown, entry) in [("show", "a"), ("other", "b")] {
        assert_eq!(
            custom(&project, rundown, entry),
            json!({ "Mic_2": "1", "Camera": "2" })
        );
    }
    let mut changed = refetched_rundowns(&effects);
    changed.sort();
    assert_eq!(changed, vec!["other", "show"]);
}

#[test]
fn editing_only_the_colour_keeps_the_key_and_values() {
    let mut project = room();
    let (status, fields, effects) = call(
        &mut project,
        HttpMethod::Put,
        "/data/custom-fields/Mic",
        json!({ "colour": "#f00" }),
    );
    assert_eq!(status, 200);
    assert_eq!(
        fields["Mic"],
        json!({ "label": "Mic", "type": "text", "colour": "#f00" })
    );
    assert_eq!(custom(&project, "show", "a")["Mic"], "1");
    assert!(refetched_rundowns(&effects).is_empty());
}

#[test]
fn editing_an_unknown_field_is_refused() {
    let mut project = room();
    let (status, _, _) = call(
        &mut project,
        HttpMethod::Put,
        "/data/custom-fields/Ghost",
        json!({ "label": "Ghost" }),
    );
    assert_eq!(status, 404);
    assert!(project.custom_fields.get("Ghost").is_none());
}

#[test]
fn a_field_cannot_change_type() {
    let mut project = room();
    let (status, _, _) = call(
        &mut project,
        HttpMethod::Put,
        "/data/custom-fields/Mic",
        json!({ "type": "image" }),
    );
    assert_eq!(status, 400);
    assert_eq!(project.custom_fields["Mic"]["type"], "text");
}

#[test]
fn a_rename_onto_another_field_is_refused() {
    let mut project = room();
    let (status, _, _) = call(
        &mut project,
        HttpMethod::Put,
        "/data/custom-fields/Mic",
        json!({ "label": "Camera" }),
    );
    assert_eq!(status, 400);
    assert_eq!(
        custom(&project, "show", "a"),
        json!({ "Mic": "1", "Camera": "2" })
    );
}

#[test]
fn a_rename_to_an_invalid_label_is_refused() {
    let mut project = room();
    let (status, _, _) = call(
        &mut project,
        HttpMethod::Put,
        "/data/custom-fields/Mic",
        json!({ "label": "Mic/2" }),
    );
    assert_eq!(status, 400);
}

// --- deleting a field ------------------------------------------------------------

#[test]
fn deleting_a_field_removes_its_values_everywhere() {
    let mut project = room();
    let (status, fields, effects) = call(
        &mut project,
        HttpMethod::Delete,
        "/data/custom-fields/Mic",
        Value::Null,
    );
    assert_eq!(status, 200);
    assert!(fields.get("Mic").is_none());
    for (rundown, entry) in [("show", "a"), ("other", "b")] {
        assert_eq!(custom(&project, rundown, entry), json!({ "Camera": "2" }));
    }
    assert_eq!(refetched_rundowns(&effects).len(), 2);
}

#[test]
fn deleting_an_unknown_field_changes_nothing() {
    let mut project = room();
    let (status, _, effects) = call(
        &mut project,
        HttpMethod::Delete,
        "/data/custom-fields/Ghost",
        Value::Null,
    );
    assert_eq!(status, 200);
    assert!(refetched_rundowns(&effects).is_empty());
}
