//! Plan 008, T0: the wrap contract. Every case was produced by running
//! upstream's `wrapText`, so passing means byte-identical lines and positions.

use ontime_core::teleprompter::wrap::{wrap_text, WrappedLine};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    text: String,
    chars_per_line: f64,
    lines: Vec<WrappedLine>,
}

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[test]
fn wrapping_matches_upstream_case_by_case() {
    let fixture: Fixture = serde_json::from_str(include_str!("fixtures/teleprompter_wrap.json")).unwrap();
    assert!(fixture.cases.len() > 50);
    let failures: Vec<String> = fixture
        .cases
        .iter()
        .filter_map(|case| {
            let actual = wrap_text(&case.text, case.chars_per_line);
            let name = &case.name;
            (actual != case.lines).then(|| format!("{name}:\n  want {:?}\n  got  {actual:?}", case.lines))
        })
        .collect();
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), fixture.cases.len(), failures.join("\n"));
}
