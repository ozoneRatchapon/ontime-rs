//! What one integration call costs in the core, by rundown size (plan 014).
//! `cargo run --release --example integration_bench`
//!
//! Each figure is the median of 9 batches, so a stray scheduler hiccup does
//! not move it. The host's own work (a Durable Object request, storage, the
//! broadcast) is not included; it costs milliseconds, which is the scale these
//! microseconds are compared against.
use std::hint::black_box;
use std::time::{Duration, Instant};

use ontime_core::integration::{dispatch, http_call, parse, HttpCall};
use ontime_core::project::Project;
use ontime_core::rundown::{Insert, Rundown};
use serde_json::{json, Value};

const T0: i64 = 1_700_000_000_000;

fn project_of(events: usize) -> (Project, Vec<String>) {
    let mut project = Project::default();
    let mut rundown = Rundown::new(project.loaded.clone(), "Bench");
    let mut ids = Vec::with_capacity(events);
    for i in 0..events {
        let entry = rundown.add_entry(
            &json!({ "type": "event", "cue": format!("{i}"), "title": format!("Item {i}"), "duration": 300_000 }),
            Insert::End,
        );
        ids.push(entry["id"].as_str().unwrap_or_default().to_string());
    }
    rundown.recompute();
    project.rundowns.insert(project.loaded.clone(), rundown);
    (project, ids)
}

/// Median time per call over 9 batches of `runs`.
fn time(runs: u32, mut call: impl FnMut()) -> Duration {
    for _ in 0..runs.min(50) {
        call();
    }
    let mut batches: Vec<Duration> = (0..9)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..runs {
                call();
            }
            start.elapsed() / runs
        })
        .collect();
    batches.sort();
    batches[4]
}

fn parsed(path: &str) -> (String, Option<Value>) {
    let (path, query) = path
        .split_once('?')
        .map_or((path, None), |(p, q)| (p, Some(q)));
    match http_call(path, query) {
        Ok(HttpCall::Action { tag, payload }) => (tag, payload),
        other => panic!("{path}: {other:?}"),
    }
}

fn main() {
    println!("| call | 10 events | 100 events | 1,000 events |");
    println!("| --- | --- | --- | --- |");

    let row = |name: &str, measure: &dyn Fn(usize) -> Duration| {
        let cells: Vec<String> = [10, 100, 1000]
            .iter()
            .map(|&n| format!("{:?}", measure(n)))
            .collect();
        println!("| {name} | {} |", cells.join(" | "));
    };

    row("`http_call` + `parse`, `/start/index/3`", &|_| {
        time(20_000, || {
            let (tag, payload) = parsed("/start/index/3");
            black_box(parse(&tag, payload.as_ref()).ok());
        })
    });
    row(
        "`http_call` + `parse`, `/change/<id>?title=My+Title`",
        &|_| {
            time(20_000, || {
                let (tag, payload) =
                    parsed("/change/0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b?title=My+Title");
                black_box(parse(&tag, payload.as_ref()).ok());
            })
        },
    );

    let dispatched = |tag: &'static str, payload: fn(&[String], usize) -> Value, runs: u32| {
        move |n: usize| {
            let (mut project, ids) = project_of(n);
            let payload = payload(&ids, n);
            let mut now = T0;
            time(runs, || {
                now += 1;
                let action = parse(tag, Some(&payload)).expect("parses");
                black_box(dispatch(&mut project, action, now).ok());
            })
        }
    };
    row(
        "dispatch `start` by index (the last event)",
        &dispatched("start", |_, n| json!({ "index": n }), 2_000),
    );
    row(
        "dispatch `load` by cue (the last event)",
        &dispatched("load", |_, n| json!({ "cue": format!("{}", n - 1) }), 2_000),
    );
    row(
        "dispatch `load` next",
        &dispatched("load", |_, _| json!("next"), 2_000),
    );
    row(
        "dispatch `addtime`",
        &dispatched("addtime", |_, _| json!(1000), 2_000),
    );
    row(
        "dispatch `change` title (re-times the rundown)",
        &dispatched(
            "change",
            |ids, n| json!({ ids[n / 2].clone(): { "title": "Renamed" } }),
            500,
        ),
    );
    row(
        "dispatch `poll` (the whole runtime payload)",
        &dispatched("poll", |_, _| Value::Null, 2_000),
    );

    // the alarm while an event plays: the most frequent thing a host does
    row("`on_alarm` while an event plays", &|n| {
        let (mut project, _) = project_of(n);
        let start = parse("start", None).expect("parses");
        dispatch(&mut project, start, T0).ok();
        let mut now = T0;
        time(2_000, || {
            now += 1;
            black_box(ontime_core::api::on_alarm(&mut project, now));
        })
    });
    // what every command and every alarm paid before benchmarks/003
    row("copying the rundown (before 003)", &|n| {
        let (project, _) = project_of(n);
        time(2_000, || {
            black_box(project.current().cloned());
        })
    });
}
