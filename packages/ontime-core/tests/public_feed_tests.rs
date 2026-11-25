//! The audience feed (plan 023): off until turned on, and only ever what is
//! built for an audience — never a note, a skipped event, an unnamed custom
//! field, another rundown or anything that opens the room.

use ontime_core::api::{handle, ApiRequest, HttpMethod};
use ontime_core::project::Project;
use ontime_core::project_file;
use ontime_core::public_feed::{self, agenda, live, live_key, FeedError};
use ontime_core::runtime::Command;
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

fn event(id: &str, cue: &str, title: &str, start: i64, extra: Value) -> Value {
    let mut entry = json!({
        "id": id, "type": "event", "cue": cue, "title": title,
        "timeStart": start, "duration": 1_800_000, "timeEnd": start + 1_800_000,
        "note": format!("secret note {id}"),
        "custom": { "presenter": format!("Speaker {id}"), "backstage": "crew only" },
    });
    if let (Some(entry), Some(extra)) = (entry.as_object_mut(), extra.as_object()) {
        entry.extend(extra.clone());
    }
    entry
}

/// A room with a show, imported as the app would hand it over.
fn room() -> Project {
    let file = json!({
        "settings": { "version": "4.2.0" },
        "project": { "title": "Road to Mainnet" },
        "customFields": {
            "presenter": { "type": "text", "colour": "#000", "label": "presenter" },
            "backstage": { "type": "text", "colour": "#000", "label": "backstage" },
        },
        "rundowns": {
            "show": {
                "id": "show", "title": "Day 1",
                "order": ["a", "d", "g", "m", "s"],
                "entries": {
                    "a": event("a", "1", "Opening", 36_000_000, json!({})),
                    "d": { "id": "d", "type": "delay", "duration": 300_000 },
                    "g": { "id": "g", "type": "group", "title": "Talks", "entries": ["b"] },
                    "b": event("b", "2", "Keynote", 37_800_000, json!({ "parent": "g" })),
                    "m": { "id": "m", "type": "milestone", "cue": "M", "title": "Lunch", "note": "caterer 0812345678" },
                    "s": event("s", "3", "Cancelled talk", 39_600_000, json!({ "skip": true })),
                },
            },
            "other": {
                "id": "other", "title": "Crew rundown", "order": ["x"],
                "entries": { "x": event("x", "1", "Load-in", 28_800_000, json!({})) },
            },
        },
    });
    let mut project = project_file::import(&file.to_string()).unwrap().project;
    project.loaded = "show".into();
    project.password_hash = Some("the-room-token".into());
    project
}

fn post(path: &str, body: Value, token: Option<&str>) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Post,
        path: path.into(),
        body,
        token: token.map(str::to_string),
        master_token: None,
        room_is_new: false,
        member: None,
    }
}

#[test]
fn off_until_the_room_turns_it_on() {
    let project = Project::default();
    assert!(!project.public_feed.enabled);
    assert!(project.public_feed.fields.is_empty());
}

#[test]
fn turned_on_by_someone_who_may_edit_the_room() {
    let mut project = room();
    let refused = handle(
        &mut project,
        &post("/data/public-feed", json!({ "enabled": true }), None),
        NOW,
    );
    assert_eq!(refused.status, 401);
    assert!(!project.public_feed.enabled);
    let body = json!({ "enabled": true, "fields": ["presenter", "presenter"] });
    let response = handle(
        &mut project,
        &post("/data/public-feed", body, Some("the-room-token")),
        NOW,
    );
    assert_eq!(response.status, 200);
    assert!(project.public_feed.enabled);
    assert_eq!(project.public_feed.fields, vec!["presenter".to_string()]);
}

#[test]
fn bad_changes_are_refused_and_change_nothing() {
    let mut project = room();
    for (body, error) in [
        (json!({ "enabled": "yes" }), FeedError::NotABoolean),
        (json!({ "fields": "presenter" }), FeedError::NotAList),
        (json!({ "fields": [1] }), FeedError::NotAList),
        (
            json!({ "enabled": true, "fields": ["nope"] }),
            FeedError::UnknownField("nope".into()),
        ),
    ] {
        let before = project.public_feed.clone();
        assert_eq!(public_feed::update(&mut project, &body), Err(error));
        assert_eq!(project.public_feed, before);
    }
}

#[test]
fn the_agenda_is_only_what_an_audience_should_see() {
    let mut project = room();
    public_feed::update(
        &mut project,
        &json!({ "enabled": true, "fields": ["presenter"] }),
    )
    .unwrap();
    let shown = agenda(&project);
    assert_eq!(shown["title"], "Road to Mainnet");
    assert_eq!(shown["rundown"], "Day 1");
    let kinds: Vec<(&str, &str)> = shown["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["type"].as_str().unwrap(), e["title"].as_str().unwrap()))
        .collect();
    // in order, the group with its talk, no delay, no skipped event
    assert_eq!(
        kinds,
        vec![
            ("event", "Opening"),
            ("group", "Talks"),
            ("event", "Keynote"),
            ("milestone", "Lunch")
        ]
    );
    let keynote = &shown["entries"][2];
    assert_eq!(keynote["group"], "g");
    assert_eq!(keynote["fields"], json!({ "presenter": "Speaker b" }));
    assert_eq!(keynote["timeStart"], 37_800_000);

    let text = shown.to_string();
    for private in [
        "secret note",
        "crew only",
        "0812345678",
        "Cancelled talk",
        "Load-in",
        "Crew rundown",
        "the-room-token",
    ] {
        assert!(!text.contains(private), "{private} leaked: {text}");
    }
}

#[test]
fn custom_fields_stay_out_unless_named() {
    let mut project = room();
    public_feed::update(&mut project, &json!({ "enabled": true })).unwrap();
    let text = agenda(&project).to_string();
    assert!(!text.contains("Speaker a"));
    assert_eq!(agenda(&project)["entries"][0]["fields"], json!({}));
}

#[test]
fn where_the_show_is() {
    let mut project = room();
    let stopped = live(&project, NOW);
    assert_eq!(stopped["playback"], "stop");
    assert_eq!(stopped["now"], Value::Null);

    let rundown = project.current().unwrap().clone();
    project
        .runtime
        .apply(Command::StartId("a".into()), &rundown, NOW, 0);
    let playing = live(&project, NOW + 1_000);
    assert_eq!(playing["playback"], "play", "{playing}");
    assert_eq!(playing["now"], "a");
    assert_eq!(playing["next"], "b");
    assert!(playing["timer"]["current"].as_i64().unwrap() <= 1_800_000);
    let text = playing.to_string();
    assert!(!text.contains("secret note") && !text.contains("the-room-token"));

    // time passing alone is not a change worth sending
    let later = live(&project, NOW + 5_000);
    assert_ne!(playing, later);
    assert_eq!(live_key(&playing), live_key(&later));
    assert_ne!(live_key(&playing), live_key(&stopped));
}

#[test]
fn an_import_keeps_the_rooms_feed() {
    let mut project = room();
    public_feed::update(
        &mut project,
        &json!({ "enabled": true, "fields": ["presenter"] }),
    )
    .unwrap();
    let file = json!({ "settings": { "version": "4.2.0" } }).to_string();
    let imported = project_file::import(&file).unwrap().project;
    project_file::import_into(&mut project, imported, NOW);
    assert!(project.public_feed.enabled);
}
