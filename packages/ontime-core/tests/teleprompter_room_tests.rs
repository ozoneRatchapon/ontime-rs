//! Plan 008, T2: the room's teleprompter as hosts drive it through `api`.

use ontime_core::api::{apply_socket_command, handle, next_wake_in, on_alarm, runtime_payload, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use ontime_core::rundown::{Insert, Rundown};
use ontime_core::teleprompter::transport::{Playback, Stop};
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

fn room() -> Project {
    let mut project = Project::default();
    let mut rundown = Rundown::new("show", "Show");
    for (id, cue, note) in [("a", "1", "one two three four"), ("b", "2", "five six seven eight"), ("c", "3", "nine ten")] {
        rundown.add_entry(&json!({ "id": id, "type": "event", "cue": cue, "title": id, "note": note, "duration": 60000 }), Insert::End);
    }
    project.rundowns.insert("show".into(), rundown);
    project.loaded = "show".into();
    project.ensure_consistent();
    project.teleprompter.chars_per_line = 10;
    project
}

fn call(project: &mut Project, method: HttpMethod, path: &str, body: Value) -> (u16, Value) {
    let req = ApiRequest { method, path: path.into(), body, token: None, master_token: None, room_is_new: false, member: None };
    let response = handle(project, &req, NOW);
    (response.status, response.body.unwrap_or(Value::Null))
}

fn prompt(project: &mut Project, payload: Value, now: i64) -> bool {
    apply_socket_command(project, "teleprompter", Some(&payload), now)
}

fn reading(project: &Project) -> Option<String> {
    runtime_payload(project, NOW)?["teleprompter"]["event"]["id"].as_str().map(str::to_string)
}

#[test]
fn nothing_follows_until_the_script_is_read() {
    let mut project = room();
    apply_socket_command(&mut project, "load", Some(&json!({ "id": "b" })), NOW);
    assert!(!project.prompter.built);
    assert_eq!(reading(&project), None);

    // the first read follows the event loaded before it
    let (status, script) = call(&mut project, HttpMethod::Get, "/data/teleprompter/script", Value::Null);
    assert_eq!(status, 200);
    assert_eq!(script["events"].as_array().map(Vec::len), Some(3));
    assert!(project.prompter.built);
    assert_eq!(reading(&project).as_deref(), Some("b"));
}

#[test]
fn the_first_command_in_a_room_with_nothing_loaded_plays_from_the_top() {
    let mut project = room();
    assert_eq!(project.runtime.selected_id, None);
    assert!(prompt(&mut project, json!("toggle"), NOW));
    assert_eq!(project.prompter.transport.playback, Playback::Play);
    assert_eq!(reading(&project).as_deref(), Some("a"));
}

#[test]
fn loading_an_event_moves_the_reader_in_event_mode_only() {
    let mut project = room();
    assert!(prompt(&mut project, json!("top"), NOW));
    apply_socket_command(&mut project, "load", Some(&json!({ "id": "c" })), NOW);
    assert_eq!(reading(&project).as_deref(), Some("c"));

    prompt(&mut project, json!({ "mode": "script" }), NOW);
    apply_socket_command(&mut project, "load", Some(&json!({ "id": "a" })), NOW);
    assert_eq!(reading(&project).as_deref(), Some("c"));
}

#[test]
fn an_edit_keeps_the_reader_in_their_event() {
    let mut project = room();
    prompt(&mut project, json!({ "goto": { "cue": "2" } }), NOW);
    let (status, _) = call(
        &mut project,
        HttpMethod::Put,
        "/data/rundowns/show/entry",
        json!({ "id": "a", "note": "a much longer note which now takes many more lines" }),
    );
    assert_eq!(status, 200);
    assert_eq!(reading(&project).as_deref(), Some("b"));
}

#[test]
fn the_alarm_wakes_at_the_end_of_the_event_and_stops_playback_there() {
    let mut project = room();
    prompt(&mut project, json!({ "goto": { "id": "a" } }), NOW);
    prompt(&mut project, json!({ "speed": 60 }), NOW);
    prompt(&mut project, json!("play"), NOW);
    // 2 lines at 60 a minute
    let wake = next_wake_in(&project, NOW, false).unwrap();
    assert!((1_990..=2_010).contains(&wake), "{wake}");

    assert!(!on_alarm(&mut project, NOW + 1_000), "still reading");
    assert!(on_alarm(&mut project, NOW + wake));
    let transport = &project.prompter.transport;
    assert_eq!((transport.playback, transport.ended), (Playback::Pause, Some(Stop::Event)));
    assert_eq!(next_wake_in(&project, NOW + wake, false), None, "nothing more to wake for");
}

#[test]
fn a_restore_pauses_shared_playback() {
    let mut project = room();
    prompt(&mut project, json!("play"), NOW);
    let (_, backup) = call(&mut project, HttpMethod::Post, "/data/db/download", Value::Null);
    let (status, _) = call(&mut project, HttpMethod::Post, "/data/db/restore", backup);
    assert_eq!(status, 200);
    assert_eq!(project.prompter.transport.playback, Playback::Pause);
}

#[test]
fn settings_refuse_and_accept_like_upstream() {
    let mut project = room();
    assert_eq!(call(&mut project, HttpMethod::Post, "/data/teleprompter/settings", json!({ "heading": "banner" })).0, 422);
    let (status, settings) = call(&mut project, HttpMethod::Post, "/data/teleprompter/settings", json!({ "heading": "both" }));
    assert_eq!((status, settings["heading"].as_str()), (200, Some("both")));
    assert_eq!(settings["charsPerLine"], 10);
}

#[test]
fn the_script_source_follows_its_custom_field() {
    let mut project = room();
    call(&mut project, HttpMethod::Post, "/data/custom-fields", json!({ "label": "Lines", "type": "text", "colour": "" }));
    assert_eq!(call(&mut project, HttpMethod::Post, "/data/teleprompter/settings", json!({ "script": "custom-Lines" })).0, 200);
    call(&mut project, HttpMethod::Put, "/data/custom-fields/Lines", json!({ "label": "Speech" }));
    assert_eq!(project.teleprompter.script, "custom-Speech");
    call(&mut project, HttpMethod::Delete, "/data/custom-fields/Speech", Value::Null);
    assert_eq!(project.teleprompter.script, "note");
}

#[test]
fn refused_commands_change_nothing() {
    let mut project = room();
    prompt(&mut project, json!("top"), NOW);
    let before = project.prompter.clone();
    assert!(!prompt(&mut project, json!({ "goto": { "cue": "99" } }), NOW + 1));
    assert!(!prompt(&mut project, json!("dance"), NOW + 1));
    assert_eq!(project.prompter, before);
}

#[test]
fn the_sync_is_the_same_for_every_screen_at_any_moment() {
    let mut project = room();
    prompt(&mut project, json!("play"), NOW);
    let early = runtime_payload(&project, NOW + 100).unwrap()["teleprompterSync"].clone();
    let later = runtime_payload(&project, NOW + 900).unwrap()["teleprompterSync"].clone();
    assert_eq!(early, later, "the sync changes with commands, not with time");
}
