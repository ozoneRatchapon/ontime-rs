//! What the room's teleprompter reads and how it lays it out. Stored with the
//! project. Ported from upstream's `teleprompterSettings.ts`,
//! `parseTeleprompterSettings` and the route validation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CHARS_PER_LINE_MIN: u32 = 10;
pub const CHARS_PER_LINE_MAX: u32 = 80;

/// Where a script source names a custom field: `custom-{key}`.
const CUSTOM_FIELD_PREFIX: &str = "custom-";

/// What each event's heading line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Heading {
    Title,
    Cue,
    Both,
    None,
}

impl Heading {
    fn parse(value: &Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeleprompterSettings {
    /// `note`, `title` or `custom-{key}` for a text custom field.
    pub script: String,
    pub chars_per_line: u32,
    pub heading: Heading,
}

impl Default for TeleprompterSettings {
    fn default() -> Self {
        Self {
            script: "note".into(),
            chars_per_line: 32,
            heading: Heading::Title,
        }
    }
}

pub fn script_source_of_custom_field(key: &str) -> String {
    format!("{CUSTOM_FIELD_PREFIX}{key}")
}

pub fn custom_field_of_script_source(source: &str) -> Option<&str> {
    source.strip_prefix(CUSTOM_FIELD_PREFIX)
}

/// A source follows its custom field when the field's key changes, and falls
/// back to the note when the field is deleted. Anything else is unchanged.
pub fn rename_script_source(source: &str, old_key: &str, new_key: Option<&str>) -> String {
    if source != script_source_of_custom_field(old_key) {
        return source.to_string();
    }
    match new_key {
        Some(key) => script_source_of_custom_field(key),
        None => TeleprompterSettings::default().script,
    }
}

/// Whether a source names something with text: the note, the title, or a
/// custom field of type `text` that exists.
pub fn is_readable_script_source(script: &str, custom_fields: &Value) -> bool {
    match script {
        "note" | "title" => true,
        _ => {
            custom_field_of_script_source(script)
                .and_then(|key| custom_fields.get(key))
                .and_then(|field| field.get("type"))
                .and_then(Value::as_str)
                == Some("text")
        }
    }
}

/// Settings from untrusted data, each missing or invalid value taken from the
/// fallback. A line length is rounded and kept within bounds.
pub fn parse_settings(data: &Value, fallback: &TeleprompterSettings) -> TeleprompterSettings {
    let Some(input) = data.as_object().filter(|_| !data.is_array()) else {
        return fallback.clone();
    };
    TeleprompterSettings {
        script: input
            .get("script")
            .and_then(Value::as_str)
            .map_or_else(|| fallback.script.clone(), str::to_string),
        chars_per_line: parse_chars_per_line(input.get("charsPerLine"), fallback.chars_per_line),
        heading: input
            .get("heading")
            .and_then(Heading::parse)
            .unwrap_or(fallback.heading),
    }
}

fn parse_chars_per_line(value: Option<&Value>, fallback: u32) -> u32 {
    let Some(number) = value.and_then(Value::as_f64).filter(|n| n.is_finite()) else {
        return fallback;
    };
    // JavaScript's Math.round: halves go up
    let rounded = (number + 0.5).floor();
    rounded.clamp(f64::from(CHARS_PER_LINE_MIN), f64::from(CHARS_PER_LINE_MAX)) as u32
}

/// Why a settings patch is refused. The route answers 422 and changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    NotAnObject,
    UnreadableScript,
    InvalidCharsPerLine,
    InvalidHeading,
}

impl SettingsError {
    pub fn message(self) -> &'static str {
        match self {
            SettingsError::NotAnObject => "Teleprompter settings have to be an object",
            SettingsError::UnreadableScript => {
                "The script has to be the note, the title or a text custom field"
            }
            SettingsError::InvalidCharsPerLine => {
                "Characters per line has to be a whole number from 10 to 80"
            }
            SettingsError::InvalidHeading => "The heading has to be title, cue, both or none",
        }
    }
}

/// The route's checks, stricter than [`parse_settings`]: nothing is coerced,
/// and a present `null` is a wrong value, not a missing one.
pub fn validate_patch(body: &Value, custom_fields: &Value) -> Result<(), SettingsError> {
    let Some(input) = body.as_object() else {
        return Err(SettingsError::NotAnObject);
    };
    if let Some(script) = input.get("script") {
        let readable = script
            .as_str()
            .is_some_and(|script| is_readable_script_source(script, custom_fields));
        if !readable {
            return Err(SettingsError::UnreadableScript);
        }
    }
    if let Some(chars) = input.get("charsPerLine") {
        let whole = chars
            .as_f64()
            .filter(|n| n.fract() == 0.0)
            .is_some_and(|n| {
                (f64::from(CHARS_PER_LINE_MIN)..=f64::from(CHARS_PER_LINE_MAX)).contains(&n)
            });
        if !whole {
            return Err(SettingsError::InvalidCharsPerLine);
        }
    }
    if let Some(heading) = input.get("heading") {
        if Heading::parse(heading).is_none() {
            return Err(SettingsError::InvalidHeading);
        }
    }
    Ok(())
}
