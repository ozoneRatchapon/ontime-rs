//! The schedule-math contract: the same fixture file is run by
//! `packages/utils/src/date-utils/scheduleMath.contract.test.ts`, so the Rust
//! and TypeScript copies cannot drift apart without a red test.

use ontime_core::offset::{
    expected_end, expected_start, ExpectedEndInputs, ExpectedStartInputs, OffsetMode,
};
use serde::Deserialize;

const FIXTURE: &str = include_str!("fixtures/schedule_math.json");

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Absolute,
    Relative,
}

impl From<Mode> for OffsetMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Absolute => OffsetMode::Absolute,
            Mode::Relative => OffsetMode::Relative,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartEvent {
    time_start: i64,
    day_offset: i64,
    delay: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartState {
    current_day: i64,
    total_gap: i64,
    is_linked_to_loaded: bool,
    offset: i64,
    mode: Mode,
    actual_start: Option<i64>,
    planned_start: Option<i64>,
}

#[derive(Deserialize)]
struct StartCase {
    name: String,
    event: StartEvent,
    state: StartState,
    expected: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndEvent {
    time_start: i64,
    day_offset: i64,
    duration: i64,
    count_to_end: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndCase {
    name: String,
    event: EndEvent,
    expected_start: i64,
    current_day: i64,
    expected: i64,
}

#[derive(Deserialize)]
struct Fixture {
    expected_start: Vec<StartCase>,
    expected_end: Vec<EndCase>,
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("schedule_math.json parses")
}

#[test]
fn expected_start_matches_the_contract() {
    let cases = fixture().expected_start;
    assert!(!cases.is_empty());
    let failures: Vec<String> = cases
        .into_iter()
        .filter_map(|case| {
            let actual = expected_start(&ExpectedStartInputs {
                time_start: case.event.time_start,
                day_offset: case.event.day_offset,
                delay: case.event.delay,
                current_day: case.state.current_day,
                total_gap: case.state.total_gap,
                is_linked_to_loaded: case.state.is_linked_to_loaded,
                offset: case.state.offset,
                mode: case.state.mode.into(),
                actual_start: case.state.actual_start,
                planned_start: case.state.planned_start,
            });
            let name = &case.name;
            let expected = case.expected;
            (actual != expected).then(|| format!("{name}: expected {expected}, got {actual}"))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn expected_end_matches_the_contract() {
    let cases = fixture().expected_end;
    assert!(!cases.is_empty());
    let failures: Vec<String> = cases
        .into_iter()
        .filter_map(|case| {
            let event = ExpectedEndInputs {
                time_start: case.event.time_start,
                day_offset: case.event.day_offset,
                duration: case.event.duration,
                count_to_end: case.event.count_to_end,
            };
            let actual = expected_end(&event, case.expected_start, case.current_day);
            let name = &case.name;
            let expected = case.expected;
            (actual != expected).then(|| format!("{name}: expected {expected}, got {actual}"))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}
