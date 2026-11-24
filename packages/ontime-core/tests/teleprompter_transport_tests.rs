//! Plan 008, T0: the transport contract. Each scenario was replayed step by
//! step against upstream's teleprompterTransport.ts; here the same steps are
//! replayed against the Rust core and every outcome compared.

use std::collections::HashMap;

use ontime_core::teleprompter::position::Layout;
use ontime_core::teleprompter::script::ScriptEvent;
use ontime_core::teleprompter::transport::{
    apply_command, describe, describe_sync, parse_command, position_at, reanchor_transport, settle,
    sync_position_at, Command, State, Sync, Transport, TransportCommand,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Step {
    Command { cmd: TransportCommand, now: f64 },
    Settle { settle: f64 },
    Position { position: f64 },
    Describe { describe: f64 },
    Sync { sync: u64 },
    SyncAt {
        #[serde(rename = "syncAt")]
        sync_at: f64,
    },
    Reanchor { reanchor: String, now: f64 },
}

#[derive(Deserialize, Debug, Default)]
struct Outcome {
    transport: Option<Transport>,
    error: Option<String>,
    // present and null means "no position"
    #[serde(default, deserialize_with = "present")]
    position: Option<Option<f64>>,
    state: Option<State>,
    sync: Option<Sync>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    script: String,
    initial: Transport,
    steps: Vec<Step>,
    results: Vec<Outcome>,
}

#[derive(Deserialize)]
struct Parse {
    payload: Value,
    command: Option<Command>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Cases {
    scripts: HashMap<String, Vec<ScriptEvent>>,
    scenarios: Vec<Scenario>,
    parses: Vec<Parse>,
}

#[derive(Deserialize)]
struct Fixture {
    cases: Cases,
}

fn fixture() -> Cases {
    serde_json::from_str::<Fixture>(include_str!("fixtures/teleprompter_transport.json")).unwrap().cases
}

#[test]
fn every_scenario_step_matches_upstream() {
    let cases = fixture();
    let mut steps = 0;
    for scenario in &cases.scenarios {
        let mut events = cases.scripts[&scenario.script].clone();
        let mut layout = Layout::new(&events);
        let mut transport = scenario.initial.clone();
        for (index, (step, want)) in scenario.steps.iter().zip(&scenario.results).enumerate() {
            let label = format!("{} / step {index}: {step:?}", scenario.name);
            match step {
                Step::Command { cmd, now } => match apply_command(&transport, cmd, &layout, *now) {
                    Ok(next) => {
                        assert_eq!(want.error, None, "{label}: upstream refused");
                        transport = next;
                    }
                    Err(error) => assert_eq!(want.error.as_deref(), Some(error.message()), "{label}"),
                },
                Step::Settle { settle: now } => transport = settle(&transport, &layout, *now),
                Step::Position { position: now } => {
                    assert_eq!(Some(position_at(&transport, &layout, *now)), want.position, "{label}")
                }
                Step::Describe { describe: now } => {
                    assert_eq!(Some(describe(&transport, &layout, *now)), want.state, "{label}")
                }
                Step::Sync { sync } => assert_eq!(Some(describe_sync(&transport, &layout, *sync)), want.sync, "{label}"),
                Step::SyncAt { sync_at } => {
                    let at = sync_position_at(&describe_sync(&transport, &layout, 1), *sync_at);
                    assert_eq!(Some(Some(at)), want.position, "{label}");
                }
                Step::Reanchor { reanchor, now } => {
                    let next = cases.scripts[reanchor].clone();
                    let next_layout = Layout::new(&next);
                    transport = reanchor_transport(&transport, (&events, &layout), (&next, &next_layout), *now);
                    events = next;
                    layout = next_layout;
                }
            }
            if let Some(want_transport) = &want.transport {
                assert_eq!(&transport, want_transport, "{label}: transport");
            }
            steps += 1;
        }
    }
    assert!(steps > 100, "{steps}");
}

#[test]
fn commands_parse_like_upstream() {
    for case in fixture().parses {
        // the generator wrote Infinity as a string, which parses to the same refusal
        match (parse_command(&case.payload), &case.command, &case.error) {
            (Ok(command), Some(want), None) => assert_eq!(&command, want, "{}", case.payload),
            (Err(error), None, Some(want)) => assert_eq!(error.message(), want, "{}", case.payload),
            (got, want, error) => panic!("{}: got {got:?}, upstream {want:?} / {error:?}", case.payload),
        }
    }
}
