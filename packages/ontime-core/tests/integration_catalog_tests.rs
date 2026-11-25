//! The integration catalogue and its OpenAPI document cannot drift from the
//! parser, or from upstream (plan 013).

use std::collections::BTreeSet;

use ontime_core::integration::catalog::{openapi, tag_of, CALLS};
use ontime_core::integration::{http_call, parse, HttpCall};
use serde_json::Value;

const DOCUMENT: &str = include_str!("../../../.docs/integration_api.openapi.json");

#[test]
fn every_documented_example_parses() {
    let mut failures = Vec::new();
    for call in CALLS {
        let (path, query) = match call.example.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (call.example, None),
        };
        let parsed = http_call(path, query).and_then(|call| match call {
            HttpCall::Action { tag, payload } => parse(&tag, payload.as_ref()),
            HttpCall::Hello => unreachable!("every example names an action"),
        });
        if let Err(error) = parsed {
            failures.push(format!("{}: {}", call.example, error.message));
        }
    }
    assert!(
        failures.is_empty(),
        "examples that do not parse:\n{}",
        failures.join("\n")
    );
}

#[test]
fn every_action_upstream_has_is_documented() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/integration.json")).unwrap();
    let documented: BTreeSet<&str> = CALLS.iter().map(|call| tag_of(call.path)).collect();
    // every action upstream's own dispatcher accepted in the contract
    let accepted: BTreeSet<String> = fixture["cases"]["ws"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case.get("error").is_none())
        .filter_map(|case| case["tag"].as_str().map(str::to_lowercase))
        .chain(
            fixture["cases"]["http"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|case| case["status"] == 202)
                .filter_map(|case| case["path"].as_str())
                .map(|path| tag_of(path.trim_start_matches("/api")).to_lowercase()),
        )
        .collect();
    let missing: Vec<_> = accepted
        .iter()
        .filter(|tag| !documented.contains(tag.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "upstream actions missing from the catalogue: {missing:?}"
    );
}

#[test]
fn the_committed_document_is_what_the_catalogue_generates() {
    let committed: Value = serde_json::from_str(DOCUMENT).unwrap();
    assert!(
        committed == openapi(),
        "the OpenAPI document is stale; regenerate it with\n  cargo run -q -p ontime-core --example integration-openapi > .docs/integration_api.openapi.json"
    );
}
