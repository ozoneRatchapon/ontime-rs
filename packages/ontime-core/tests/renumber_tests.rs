//! Renumbering cues, as the client asks for it: `{ ids, prefix, start, increment }`
//! with the numbers as decimal strings. Ported from upstream's `renumberEntries`
//! and `getIntegerAndFraction`; the earlier port read `startAt` and ignored
//! `ids`, so it renumbered the whole rundown from 1.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::cue::CueNumber;
use ontime_core::project::Project;
use ontime_core::rundown::{Insert, Rundown};
use serde_json::{json, Value};

fn room() -> (Project, Vec<String>) {
    let mut project = Project::default();
    let mut rundown = Rundown::new("show", "Show");
    let mut ids = Vec::new();
    for (i, kind) in ["event", "event", "delay", "event", "event"]
        .iter()
        .enumerate()
    {
        let entry = rundown.add_entry(
            &json!({ "type": kind, "cue": format!("old{i}") }),
            Insert::End,
        );
        ids.push(entry["id"].as_str().unwrap().to_string());
    }
    project.rundowns.insert("show".into(), rundown);
    project.loaded = "show".into();
    project.ensure_consistent();
    (project, ids)
}

fn renumber(project: &mut Project, body: Value) -> (u16, Value) {
    let req = ApiRequest {
        method: HttpMethod::Patch,
        path: "/data/rundowns/show/renumber".into(),
        body,
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    };
    let response = handle(project, &req, 0);
    (response.status, response.body.unwrap_or(Value::Null))
}

fn cue(project: &Project, id: &str) -> String {
    project.rundowns["show"].get(id).unwrap()["cue"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn parses_decimal_strings() {
    assert_eq!(
        CueNumber::parse("10"),
        Some(CueNumber {
            integer: 10,
            fraction: 0,
            precision: 0
        })
    );
    assert_eq!(
        CueNumber::parse("1.05"),
        Some(CueNumber {
            integer: 1,
            fraction: 5,
            precision: 2
        })
    );
    assert_eq!(
        CueNumber::parse("-2.5"),
        Some(CueNumber {
            integer: -2,
            fraction: 5,
            precision: 1
        })
    );
    for bad in [".5", "", "a", "1.", "1.2.3", "1e3", " 1"] {
        assert_eq!(CueNumber::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn renumbers_only_the_given_entries_in_the_given_order() {
    let (mut project, ids) = room();
    let (status, _) = renumber(
        &mut project,
        json!({ "ids": [ids[3], ids[0]], "prefix": "", "start": "10", "increment": "5" }),
    );
    assert_eq!(status, 200);
    assert_eq!(cue(&project, &ids[3]), "10");
    assert_eq!(cue(&project, &ids[0]), "15");
    assert_eq!(
        cue(&project, &ids[1]),
        "old1",
        "an entry not asked for keeps its cue"
    );
    assert_eq!(cue(&project, &ids[4]), "old4");
}

#[test]
fn a_prefix_gets_a_separator_unless_it_has_one() {
    let (mut project, ids) = room();
    renumber(
        &mut project,
        json!({ "ids": [ids[0]], "prefix": "A", "start": "1", "increment": "1" }),
    );
    assert_eq!(cue(&project, &ids[0]), "A 1");
    renumber(
        &mut project,
        json!({ "ids": [ids[0]], "prefix": "B-", "start": "1", "increment": "1" }),
    );
    assert_eq!(cue(&project, &ids[0]), "B-1");
    renumber(
        &mut project,
        json!({ "ids": [ids[0]], "prefix": "C ", "start": "1", "increment": "1" }),
    );
    assert_eq!(cue(&project, &ids[0]), "C 1");
}

#[test]
fn fractions_share_the_longer_precision() {
    let (mut project, ids) = room();
    renumber(
        &mut project,
        json!({ "ids": [ids[0], ids[1], ids[3]], "prefix": "", "start": "1.5", "increment": "0.25" }),
    );
    // as upstream: 1.50, 1.75, 1.100 — the fraction does not roll into the integer
    assert_eq!(
        [
            cue(&project, &ids[0]),
            cue(&project, &ids[1]),
            cue(&project, &ids[3])
        ],
        ["1.50".to_string(), "1.75".to_string(), "1.100".to_string()]
    );
}

#[test]
fn a_number_that_is_not_one_is_refused_and_changes_nothing() {
    let (mut project, ids) = room();
    let (status, body) = renumber(
        &mut project,
        json!({ "ids": [ids[0]], "prefix": "", "start": ".5", "increment": "1" }),
    );
    assert_eq!(status, 422);
    assert_eq!(body["message"], "input can not be converted to a number");
    assert_eq!(cue(&project, &ids[0]), "old0");
}

#[test]
fn an_entry_that_is_not_an_event_is_refused_and_changes_nothing() {
    let (mut project, ids) = room();
    let (status, body) = renumber(
        &mut project,
        json!({ "ids": [ids[0], ids[2]], "prefix": "", "start": "1", "increment": "1" }),
    );
    assert_eq!(status, 422);
    assert_eq!(body["message"], "A given id was not an event");
    assert_eq!(
        cue(&project, &ids[0]),
        "old0",
        "checked before anything is changed"
    );
}

#[test]
fn a_malformed_body_is_refused() {
    let (mut project, ids) = room();
    for body in [
        json!({}),
        json!({ "ids": [], "prefix": "", "start": "1", "increment": "1" }),
        json!({ "ids": [ids[0]], "prefix": "", "start": 1, "increment": "1" }),
        json!({ "ids": [ids[0]], "start": "1", "increment": "1" }),
    ] {
        assert_eq!(renumber(&mut project, body.clone()).0, 422, "{body}");
    }
    assert_eq!(cue(&project, &ids[0]), "old0");
}
