//! What a room shows its audience (plan 023): the agenda and where the show is,
//! read without any token, for attendees' phones and for other sites such as
//! BeThere's event page.
//!
//! A room's token lets its holder change the room, so it must never travel in
//! a link meant for attendees. This feed is the safe alternative: it is off
//! until the room turns it on, and it carries only what is built here.
//!
//! - In: the project title, the loaded rundown's title, and its events (cue,
//!   title, times, colour, group), milestones and group titles, in order.
//! - In, only when the room names them: custom fields (`fields`, by key), such
//!   as a presenter.
//! - In: where the show is: playing or not, which event now and next, the
//!   timer, how far ahead or behind, the planned and expected end.
//! - Out, always: notes, skipped events, delays, other custom fields, other
//!   rundowns, the report, messages, the teleprompter, tokens, the activity log.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::project::Project;
use crate::rundown::entry_type;

/// The page attendees open: the agenda, live, from this feed.
pub const AGENDA_PAGE: &str = include_str!("public_agenda.html");

/// Sockets a room keeps open for its audience, so a crowd cannot exhaust it.
pub const MAX_LISTENERS: usize = 1_000;

/// The room's choice, stored with the project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicFeed {
    #[serde(default)]
    pub enabled: bool,
    /// Custom field keys shown to the audience, e.g. a presenter.
    #[serde(default)]
    pub fields: Vec<String>,
}

/// Why a change to the feed was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    NotABoolean,
    NotAList,
    UnknownField(String),
}

impl FeedError {
    pub fn message(&self) -> String {
        match self {
            Self::NotABoolean => "\"enabled\" must be true or false".into(),
            Self::NotAList => "\"fields\" must be a list of custom field keys".into(),
            Self::UnknownField(key) => format!("\"{key}\" is not a custom field of this room"),
        }
    }
}

/// Applies `{ enabled?, fields? }` to the room's feed. Fields must be the
/// room's own custom fields; nothing changes when anything is refused.
pub fn update(project: &mut Project, body: &Value) -> Result<(), FeedError> {
    let mut next = project.public_feed.clone();
    if let Some(enabled) = body.get("enabled") {
        next.enabled = enabled.as_bool().ok_or(FeedError::NotABoolean)?;
    }
    if let Some(fields) = body.get("fields") {
        let list = fields.as_array().ok_or(FeedError::NotAList)?;
        let mut keys = Vec::new();
        for key in list {
            let key = key.as_str().ok_or(FeedError::NotAList)?;
            if project.custom_fields.get(key).is_none() {
                return Err(FeedError::UnknownField(key.to_string()));
            }
            if !keys.iter().any(|k| k == key) {
                keys.push(key.to_string());
            }
        }
        next.fields = keys;
    }
    project.public_feed = next;
    Ok(())
}

/// The agenda: what is planned, in order. Changes only when the show is edited.
pub fn agenda(project: &Project) -> Value {
    let Some(rundown) = project.current() else {
        return json!({ "title": project.project.title, "rundown": "", "entries": [] });
    };
    let shown = shown_fields(project);
    let entries: Vec<Value> = rundown
        .flat_order
        .iter()
        .filter_map(|id| rundown.get(id))
        .filter_map(|entry| public_entry(entry, &shown))
        .collect();
    json!({
        "title": project.project.title,
        "rundown": rundown.title,
        "fields": shown.iter().map(|(_, label)| label).collect::<Vec<_>>(),
        "entries": entries,
    })
}

/// Where the show is now. `clock` and the timer's `current` change every
/// moment; the rest only when the operator acts or an event ends.
pub fn live(project: &Project, now: i64) -> Value {
    let Some(rundown) = project.current() else {
        return json!({ "playback": "stop", "clock": null });
    };
    let snapshot = project
        .runtime
        .snapshot(rundown, now, project.timezone_offset);
    let id_of = |key: &str| {
        snapshot
            .get(key)
            .and_then(|event| event.get("id"))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let timer = &snapshot["timer"];
    let offset = &snapshot["offset"];
    let run = &snapshot["rundown"];
    json!({
        "playback": timer["playback"],
        "now": id_of("eventNow"),
        "next": id_of("eventNext"),
        "clock": snapshot["clock"],
        "timer": {
            "current": timer["current"],
            "duration": timer["duration"],
            "addedTime": timer["addedTime"],
            "expectedFinish": timer["expectedFinish"],
            "phase": timer["phase"],
        },
        "offset": {
            "mode": offset["mode"],
            "absolute": offset["absolute"],
            "relative": offset["relative"],
            "expectedEnd": offset["expectedRundownEnd"],
        },
        "plannedStart": run["plannedStart"],
        "plannedEnd": run["plannedEnd"],
        "actualStart": run["actualStart"],
    })
}

/// `live` without what changes every moment, to tell a change from a tick.
/// (The offset still moves during a pause or an overrun, and is sent then.)
pub fn live_key(live: &Value) -> String {
    let mut steady = live.clone();
    if let Some(object) = steady.as_object_mut() {
        object.remove("clock");
    }
    if let Some(timer) = steady.get_mut("timer").and_then(Value::as_object_mut) {
        timer.remove("current");
    }
    steady.to_string()
}

/// The custom fields shown, as (key, label), in the room's order of choice.
fn shown_fields(project: &Project) -> Vec<(String, String)> {
    project
        .public_feed
        .fields
        .iter()
        .filter_map(|key| {
            let label = project.custom_fields.get(key)?.get("label")?.as_str()?;
            Some((key.clone(), label.to_string()))
        })
        .collect()
}

fn public_entry(entry: &Value, shown: &[(String, String)]) -> Option<Value> {
    let text = |key: &str| entry.get(key).cloned().unwrap_or(Value::Null);
    match entry_type(entry) {
        "event" if entry.get("skip").and_then(Value::as_bool) != Some(true) => {
            let custom = entry.get("custom");
            let fields: Map<String, Value> = shown
                .iter()
                .filter_map(|(key, label)| {
                    let value = custom?.get(key)?.as_str().filter(|v| !v.is_empty())?;
                    Some((label.clone(), json!(value)))
                })
                .collect();
            Some(json!({
                "type": "event",
                "id": text("id"),
                "cue": text("cue"),
                "title": text("title"),
                "timeStart": text("timeStart"),
                "timeEnd": text("timeEnd"),
                "duration": text("duration"),
                "dayOffset": text("dayOffset"),
                "colour": text("colour"),
                "group": text("parent"),
                "fields": fields,
            }))
        }
        "milestone" => Some(json!({
            "type": "milestone",
            "id": text("id"),
            "cue": text("cue"),
            "title": text("title"),
            "colour": text("colour"),
            "group": text("parent"),
        })),
        "group" => Some(json!({
            "type": "group",
            "id": text("id"),
            "title": text("title"),
            "colour": text("colour"),
        })),
        _ => None,
    }
}
