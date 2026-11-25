//! The generated fixtures still exercise what each case is for.
//!
//! A contract test compares ours with upstream case by case, so it cannot see
//! a case that has stopped testing anything: a regenerated fixture whose tick
//! no longer reaches the end of the event, or whose "fails" case now succeeds,
//! still agrees on both sides. These checks read each fixture on its own and
//! fail when a case no longer does what its name says. (The pattern is borrowed
//! from katgpt-rs's pinned test vectors, which check their own coverage.)

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

fn fixture(raw: &str) -> Value {
    serde_json::from_str::<Value>(raw).unwrap()["cases"].clone()
}

fn report(what: &str, problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{what}: cases that stopped testing what they are for:\n{}",
        problems.join("\n")
    );
}

fn has_step(case: &Value, action: &str) -> bool {
    case["steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|step| step["do"] == action)
}

fn entries(rundown: &Value) -> Vec<&Value> {
    rundown["entries"]
        .as_object()
        .map(|m| m.values().collect())
        .unwrap_or_default()
}

#[test]
fn runtime_cases_still_drive_what_they_are_named_for() {
    let cases = fixture(include_str!("fixtures/runtime_offset.json"));
    // a word in the name, and the operator action the case must contain
    let named: &[(&str, &str)] = &[
        ("roll", "roll"),
        ("pause", "pause"),
        ("resumed", "resume"),
        ("added time", "addtime"),
        ("removed time", "addtime"),
        ("reload", "reload"),
        // these lead the name: "start next while…" is a `startnext` case
        ("^next while", "next"),
        ("^next from", "next"),
        ("^previous at", "previous"),
        ("start next", "startnext"),
        ("start previous", "startprevious"),
        ("stop", "stop"),
        ("relative mode", "mode"),
    ];
    let mut problems = Vec::new();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        for (word, action) in named {
            let named_for = match word.strip_prefix('^') {
                Some(lead) => name.starts_with(lead),
                None => name.contains(word),
            };
            if named_for && !has_step(case, action) {
                problems.push(format!(
                    "{name}: named for `{word}` but has no `{action}` step"
                ));
            }
        }
        // an end action case must reach the end of the event and run the action
        for action in ["play-next", "load-next"] {
            if !name.starts_with(action) {
                continue;
            }
            let uses = entries(&case["rundown"])
                .iter()
                .any(|e| e["endAction"] == action);
            if !uses {
                problems.push(format!("{name}: no event ends with {action}"));
            }
            let playback = case["expected"]["timer"]["playback"]
                .as_str()
                .unwrap_or_default();
            let at_last = name.contains("last event");
            let fired = match (action, at_last) {
                // at the last event upstream wraps to the first, which is what is pinned
                (_, true) => case["expected"]["eventNow"] == "a",
                ("play-next", false) => playback == "play" && case["expected"]["eventNow"] != "a",
                (_, false) => playback == "armed",
            };
            if !fired {
                problems.push(format!(
                    "{name}: the end action did not run upstream (playback {playback})"
                ));
            }
        }
        if name.starts_with("roll") && case["expected"]["timer"]["playback"] != "roll" {
            problems.push(format!("{name}: upstream is not rolling at the end"));
        }
    }
    report("runtime_offset.json", problems);
}

#[test]
fn integration_cases_still_reach_both_outcomes() {
    let cases = fixture(include_str!("fixtures/integration.json"));
    let mut problems = Vec::new();
    let mut accepted = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let tag_of = |path: &str| {
        path.trim_start_matches("/api/")
            .split(['/', '?'])
            .next()
            .unwrap_or_default()
            .to_lowercase()
    };
    for case in cases["http"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let status = case["status"].as_u64().unwrap_or(0);
        let tag = tag_of(case["path"].as_str().unwrap());
        match status {
            202 => accepted.insert(tag.clone()),
            200 => false,
            _ => refused.insert(tag.clone()),
        };
        // "fails" means the service was asked and said no: the failure path
        if name.ends_with(" fails") {
            let asked = case["calls"].as_array().is_some_and(|c| !c.is_empty());
            if case["ok"] != false || status != 500 || !asked {
                problems.push(format!("{name}: no longer reaches a service that refuses"));
            }
        }
    }
    for case in cases["ws"].as_array().unwrap() {
        let tag = case["tag"].as_str().unwrap().to_lowercase();
        match case.get("error") {
            Some(_) => refused.insert(tag),
            None => accepted.insert(tag),
        };
    }
    // every action that can refuse has a case where it does
    let cannot_refuse = ["version", "poll", "pause", "stop", "reload", "roll"];
    for tag in &accepted {
        if !cannot_refuse.contains(&tag.as_str()) && !refused.contains(tag) {
            problems.push(format!("`{tag}`: accepted in some case, refused in none"));
        }
    }
    report("integration.json", problems);
}

#[test]
fn rundown_edit_cases_still_succeed_or_fail_as_named() {
    let cases = fixture(include_str!("fixtures/rundown_edits.json"));
    let refusals = [
        "id that exists",
        "group into a group",
        "does not exist",
        "not a group",
    ];
    let mut problems = Vec::new();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let errors: Vec<bool> = case["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.get("error").is_some())
            .collect();
        let should_refuse = refusals.iter().any(|word| name.contains(word));
        match should_refuse {
            true if errors != [true] => {
                problems.push(format!("{name}: upstream no longer refuses it"))
            }
            false if errors.iter().any(|e| *e) => {
                problems.push(format!("{name}: upstream now refuses a step"))
            }
            _ => {}
        }
    }
    report("rundown_edits.json", problems);
}

#[test]
fn selection_rundowns_still_hold_what_they_are_named_for() {
    let cases = fixture(include_str!("fixtures/selection.json"));
    let mut problems = Vec::new();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let events: Vec<&Value> = entries(&case["rundown"])
            .into_iter()
            .filter(|e| e["type"] == "event")
            .collect();
        let mut cues: BTreeMap<&str, usize> = BTreeMap::new();
        for event in &events {
            *cues
                .entry(event["cue"].as_str().unwrap_or_default())
                .or_default() += 1;
        }
        let ok = match name {
            "skipped events" | "only skipped" => events.iter().any(|e| e["skip"] == true),
            "shared cues" => cues.values().any(|n| *n > 1),
            "zero-length events" => events.iter().any(|e| e["duration"] == 0),
            "groups, delays and milestones" => {
                let kinds: BTreeSet<&str> = entries(&case["rundown"])
                    .iter()
                    .filter_map(|e| e["type"].as_str())
                    .collect();
                ["group", "delay", "milestone"]
                    .iter()
                    .all(|k| kinds.contains(k))
            }
            "empty" => events.is_empty(),
            _ => true,
        };
        if !ok {
            problems.push(format!(
                "{name}: the rundown no longer holds what the name says"
            ));
        }
    }
    report("selection.json", problems);
}

#[test]
fn the_js_corpus_still_reaches_every_branch() {
    let cases = fixture(include_str!("fixtures/js_semantics.json"));
    let cases = cases.as_array().unwrap();
    let count = |what: &dyn Fn(&Value) -> bool| cases.iter().filter(|c| what(c)).count();
    let needs: Vec<(&str, usize)> = vec![
        (
            "Number() is NaN",
            count(&|c| c["number"]["string"] == "NaN"),
        ),
        (
            "Number() is a finite number",
            count(&|c| {
                c["number"]["string"] != "NaN"
                    && !c["number"]["string"].as_str().unwrap().contains("Infinity")
            }),
        ),
        (
            "Number() is -0",
            count(&|c| c["number"]["negativeZero"] == true),
        ),
        (
            "Number() is Infinity",
            count(&|c| c["number"]["string"].as_str().unwrap().contains("Infinity")),
        ),
        (
            "String() uses an exponent",
            count(&|c| c["number"]["string"].as_str().unwrap().contains('e')),
        ),
        ("decodeURI() throws", count(&|c| c["decodeURI"].is_null())),
        (
            "decodeURI() keeps a reserved escape",
            count(&|c| {
                c["decodeURI"]
                    .as_str()
                    .is_some_and(|s| s.contains("%2F") || s.contains("%23"))
            }),
        ),
        (
            "the query decodes lossily",
            count(&|c| c["query"].as_str().is_some_and(|s| s.contains('\u{fffd}'))),
        ),
        (
            "the query is kept raw",
            count(&|c| c["query"].as_str().is_some_and(|s| s.contains('%'))),
        ),
    ];
    let problems = needs
        .into_iter()
        .filter(|(_, n)| *n == 0)
        .map(|(what, _)| format!("no input where {what}"))
        .collect();
    report("js_semantics.json", problems);
}

#[test]
fn import_cases_still_cover_each_kind_of_cue() {
    let cases = fixture(include_str!("fixtures/import_cues.json"));
    let mut seen = BTreeSet::new();
    for case in cases.as_array().unwrap() {
        for entry in entries(&case["rundown"]) {
            match (&entry["type"], entry.get("cue")) {
                (t, None) if t == "event" => seen.insert("an event without a cue"),
                (t, Some(Value::String(s))) if t == "event" && s.is_empty() => {
                    seen.insert("an empty cue")
                }
                (t, Some(Value::Number(_))) if t == "event" => seen.insert("a numeric cue"),
                (t, _) if t == "group" => seen.insert("a group"),
                _ => false,
            };
        }
    }
    let problems = [
        "an event without a cue",
        "an empty cue",
        "a numeric cue",
        "a group",
    ]
    .into_iter()
    .filter(|kind| !seen.contains(kind))
    .map(|kind| format!("no case with {kind}"))
    .collect();
    report("import_cues.json", problems);
}
