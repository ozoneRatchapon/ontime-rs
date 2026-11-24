//! The script: each event's text, wrapped into lines under a heading. Ported
//! from upstream's `buildScriptEvents`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::settings::{custom_field_of_script_source, is_readable_script_source, Heading, TeleprompterSettings};
use super::wrap::wrap_text;
use crate::rundown::Rundown;

/// One line of the script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Line {
    Heading {
        text: String,
    },
    Text {
        text: String,
        /// Position in the event's normalised text, in UTF-16 units.
        start: usize,
        /// Present only when the line is indented.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indent: Option<usize>,
    },
    /// The gap after an event.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptEvent {
    pub id: String,
    pub cue: String,
    pub title: String,
    pub lines: Vec<Line>,
}

/// The whole script as clients fetch it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    /// Changes whenever the lines do. See [`revision_of`].
    pub revision: u64,
    pub chars_per_line: u32,
    pub events: Vec<ScriptEvent>,
}

impl Script {
    pub fn build(rundown: &Rundown, custom_fields: &Value, settings: &TeleprompterSettings) -> Self {
        let events = build_script_events(rundown, custom_fields, settings);
        Self { revision: revision_of(&events, settings.chars_per_line), chars_per_line: settings.chars_per_line, events }
    }
}

/// A revision made from the content, so the same lines always give the same
/// number and nothing has to be stored. 48 bits, which a JavaScript number
/// holds exactly. Upstream counts up instead; clients only compare for equality.
pub fn revision_of(events: &[ScriptEvent], chars_per_line: u32) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&chars_per_line.to_le_bytes());
    hasher.update(serde_json::to_string(events).unwrap_or_default().as_bytes());
    let bytes = hasher.finalize();
    let mut wide = [0u8; 8];
    wide[..6].copy_from_slice(&bytes.as_bytes()[..6]);
    u64::from_le_bytes(wide)
}

/// The events of the rundown with text to read, in running order. Skipped
/// events, entries which are not events, and events whose text is empty are
/// left out.
pub fn build_script_events(rundown: &Rundown, custom_fields: &Value, settings: &TeleprompterSettings) -> Vec<ScriptEvent> {
    if !is_readable_script_source(&settings.script, custom_fields) {
        return Vec::new();
    }
    rundown
        .flat_order
        .iter()
        .filter_map(|id| {
            let entry = rundown.entries.get(id)?;
            let is_event = entry.get("type").and_then(Value::as_str) == Some("event");
            let skipped = entry.get("skip").and_then(Value::as_bool).unwrap_or(false);
            if !is_event || skipped {
                return None;
            }
            let text = read_text(entry, &settings.script)?;
            let wrapped = wrap_text(text, f64::from(settings.chars_per_line));
            if wrapped.is_empty() {
                return None;
            }
            let cue = text_of(entry, "cue");
            let title = text_of(entry, "title");

            let mut lines = Vec::with_capacity(wrapped.len() + 2);
            let heading = make_heading(settings.heading, &cue, &title);
            if !heading.is_empty() {
                lines.push(Line::Heading { text: heading });
            }
            lines.extend(wrapped.into_iter().map(|line| Line::Text {
                text: line.text,
                start: line.start,
                indent: (line.indent > 0).then_some(line.indent),
            }));
            lines.push(Line::Blank);
            Some(ScriptEvent { id: id.clone(), cue, title, lines })
        })
        .collect()
}

/// The event's text for a readable source, or `None` when it holds no string.
fn read_text<'a>(entry: &'a Value, script: &str) -> Option<&'a str> {
    match script {
        "note" | "title" => entry.get(script)?.as_str(),
        _ => {
            let key = custom_field_of_script_source(script)?;
            // an event without the field reads as empty
            match entry.get("custom").and_then(|custom| custom.get(key)) {
                Some(value) => value.as_str(),
                None => Some(""),
            }
        }
    }
}

fn text_of(entry: &Value, key: &str) -> String {
    entry.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn make_heading(heading: Heading, cue: &str, title: &str) -> String {
    match heading {
        Heading::Title => title.to_string(),
        Heading::Cue => cue.to_string(),
        Heading::Both => [cue, title].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · "),
        Heading::None => String::new(),
    }
}
