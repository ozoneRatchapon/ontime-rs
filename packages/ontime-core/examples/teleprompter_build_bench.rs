//! How long building the teleprompter script takes for a large rundown.
//! `cargo run --release --example teleprompter_build_bench`
use std::time::Instant;

use ontime_core::rundown::{Insert, Rundown};
use ontime_core::teleprompter::script::Script;
use ontime_core::teleprompter::settings::TeleprompterSettings;
use serde_json::json;

fn main() {
    let note =
        "Good evening and welcome to the 2026 awards, our first guest is a well-known face. "
            .repeat(6);
    let thai = "สวัสดีครับ ยินดีต้อนรับทุกท่านเข้าสู่งานประกาศรางวัล ".repeat(8);
    let mut rundown = Rundown::new("show", "Show");
    for i in 0..100 {
        let text = if i % 2 == 0 { &note } else { &thai };
        rundown.add_entry(&json!({ "id": format!("e{i}"), "type": "event", "cue": format!("{i}"), "title": "Item", "note": text }), Insert::End);
    }
    rundown.recompute();
    let settings = TeleprompterSettings::default();
    let fields = json!({});
    for _ in 0..20 {
        Script::build(&rundown, &fields, &settings);
    }
    let runs = 200;
    let start = Instant::now();
    let mut lines = 0;
    for _ in 0..runs {
        lines = Script::build(&rundown, &fields, &settings)
            .events
            .iter()
            .map(|e| e.lines.len())
            .sum::<usize>();
    }
    let per = start.elapsed() / runs;
    println!("100 events, {lines} lines: {per:?} per build");

    // what the room pays per runtime payload or wake check once cached
    let mut project = ontime_core::project::Project::default();
    project.rundowns.insert("show".into(), rundown);
    project.loaded = "show".into();
    ontime_core::teleprompter::room::script(&project);
    let start = Instant::now();
    for _ in 0..runs {
        ontime_core::teleprompter::room::script(&project);
    }
    println!("cached: {:?} per lookup", start.elapsed() / runs);
}
