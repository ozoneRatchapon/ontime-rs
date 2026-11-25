//! The project file contract (plan 020): a file imported into a room and
//! exported again is what upstream's own parseDatabaseModel makes of it, so a
//! show moves from the app to a room and back unchanged. The cases are run
//! through upstream by scripts/upstream-oracle/gen-project_file.ts.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use ontime_core::project_file;
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;
const MADE: &str = "<made>";

fn post(path: &str, body: Value) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Post,
        path: path.into(),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    }
}

fn cases() -> Vec<Value> {
    let text = include_str!("fixtures/project_file.json");
    let fixture: Value = serde_json::from_str(text).unwrap();
    fixture["cases"].as_array().unwrap().clone()
}

/// Where two values first differ, for a readable failure.
fn first_difference(path: &str, ours: &Value, theirs: &Value) -> Option<String> {
    match (ours, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().find_map(|key| {
                let (x, y) = (
                    a.get(key).unwrap_or(&Value::Null),
                    b.get(key).unwrap_or(&Value::Null),
                );
                match (a.contains_key(key), b.contains_key(key)) {
                    (true, true) => first_difference(&format!("{path}.{key}"), x, y),
                    (true, false) => Some(format!("{path}.{key}: ours has {x}, upstream has none")),
                    _ => Some(format!("{path}.{key}: upstream has {y}, ours has none")),
                }
            })
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(i, (x, y))| first_difference(&format!("{path}[{i}]"), x, y)),
        _ if ours == theirs => None,
        _ => Some(format!("{path}: ours {ours}, upstream {theirs}")),
    }
}

/// Our export laid out as the fixture records upstream's parse.
fn comparable(mut exported: Value, loaded: &str, upstream: &Value) -> Value {
    exported["settings"]["version"] = json!("<app>");
    if upstream["projectId"] == MADE {
        assert!(!exported["projectId"].as_str().unwrap().is_empty());
        exported["projectId"] = json!(MADE);
    }
    let mut first = json!(loaded);
    if upstream["rundowns"].get(MADE).is_some() {
        let rundowns = exported["rundowns"].as_object().unwrap();
        assert_eq!(rundowns.len(), 1, "one rundown made for a file without any");
        let mut rundown = rundowns.values().next().unwrap().clone();
        rundown["id"] = json!(MADE);
        exported["rundowns"] = json!({ MADE: rundown });
        first = json!(MADE);
    }
    exported["firstRundown"] = first;
    exported
}

#[test]
fn every_file_reads_as_upstream_reads_it() {
    let mut failures = Vec::new();
    for case in cases() {
        let name = case["name"].as_str().unwrap();
        let mut project = Project::default();
        let response = handle(
            &mut project,
            &post("/data/db/import", json!({ "file": case["file"] })),
            NOW,
        );
        let upstream = &case["data"];
        if upstream.is_null() {
            if response.status != 400 {
                failures.push(format!(
                    "{name}: upstream refuses it, ours answered {}",
                    response.status
                ));
            }
            continue;
        }
        if response.status != 200 {
            failures.push(format!(
                "{name}: ours answered {} {:?}",
                response.status, response.body
            ));
            continue;
        }
        let exported = handle(&mut project, &post("/data/db/export", Value::Null), NOW);
        let ours = comparable(exported.body.unwrap(), &project.loaded, upstream);
        if let Some(difference) = first_difference("", &ours, upstream) {
            failures.push(format!("{name}: {difference}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_file_exported_imports_back_the_same() {
    for case in cases().iter().filter(|c| !c["data"].is_null()) {
        let mut project = Project::default();
        handle(
            &mut project,
            &post("/data/db/import", json!({ "file": case["file"] })),
            NOW,
        );
        let once = handle(&mut project, &post("/data/db/export", Value::Null), NOW)
            .body
            .unwrap();
        let mut again = Project::default();
        let text = serde_json::to_string(&once).unwrap();
        let response = handle(
            &mut again,
            &post("/data/db/import", json!({ "file": text })),
            NOW,
        );
        assert_eq!(response.status, 200, "{}", case["name"]);
        let twice = handle(&mut again, &post("/data/db/export", Value::Null), NOW)
            .body
            .unwrap();
        assert_eq!(once, twice, "{}", case["name"]);
    }
}

#[test]
fn the_room_keeps_what_is_the_rooms() {
    let mut project = Project {
        password_hash: Some("token".into()),
        members_only: true,
        css_override: "body{}".into(),
        timezone_offset: 7,
        ..Project::default()
    };
    let file = json!({ "settings": { "version": "4.2.0" }, "project": { "title": "Gala" } });
    let imported = project_file::import(&file.to_string()).unwrap();
    project_file::import_into(&mut project, imported.project, NOW);
    assert_eq!(project.project.title, "Gala");
    assert_eq!(project.password_hash.as_deref(), Some("token"));
    assert!(project.members_only);
    assert_eq!(project.css_override, "body{}");
    assert_eq!(project.timezone_offset, 7);
    assert!(!project.project_id.is_empty());
    assert!(project.rundowns.contains_key(&project.loaded));
}

#[test]
fn files_that_are_not_project_files_are_refused_and_change_nothing() {
    for (file, says) in [
        ("not json", "not JSON"),
        ("[1, 2]", "not an Ontime project file"),
        (r#"{"settings": {"version": "3.9.0"}}"#, "from Ontime v3"),
        (
            r#"{"settings": {"version": "4.0.0"}, "rundowns": {"r": {"id": "r"}}}"#,
            "no order or entries",
        ),
    ] {
        let mut project = Project::default();
        let before = serde_json::to_value(&project).unwrap();
        let response = handle(
            &mut project,
            &post("/data/db/import", json!({ "file": file })),
            NOW,
        );
        assert_eq!(response.status, 400, "{file}");
        let message = response.body.unwrap()["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(message.contains(says), "{file}: {message}");
        assert_eq!(serde_json::to_value(&project).unwrap(), before);
    }
    let mut project = Project::default();
    let response = handle(
        &mut project,
        &post("/data/db/import", json!({ "project": {} })),
        NOW,
    );
    assert_eq!(response.status, 400);
}

#[test]
fn each_problem_is_reported_once() {
    let file = json!({
        "settings": { "version": "4.2.0" },
        "rundowns": { "r": { "id": "r", "order": ["gone", "lost"], "entries": {} } },
    });
    let imported = project_file::import(&file.to_string()).unwrap();
    let missing = imported
        .warnings
        .iter()
        .filter(|w| w.contains("could not find referenced event"))
        .count();
    assert_eq!(missing, 1, "{:?}", imported.warnings);
}
