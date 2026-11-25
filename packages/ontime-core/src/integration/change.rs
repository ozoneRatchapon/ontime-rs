//! `change`: edit a field of an entry from outside, as upstream's
//! `isValidChangeProperty` and `parseProperty` (integration.utils.ts) allow.

use serde_json::{json, Map, Value};

use super::action::{coerce_boolean, coerce_number, coerce_string};
use super::colour::{is_colour_hex, is_css_colour};
use super::types::{Change, IntegrationError, Reply};
use crate::project::Project;

/// Upstream's maxDuration: a day less a second.
const MAX_DURATION: f64 = 86_399_000.0;

/// How each field that may be changed is read from what was sent.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Flag,
    Colour,
    Time,
    OneOf(&'static [&'static str]),
}

const END_ACTIONS: &[&str] = &["load-next", "none", "play-next"];
const TIMER_TYPES: &[&str] = &["count-down", "count-up", "clock", "none"];
const TIME_STRATEGIES: &[&str] = &["lock-end", "lock-duration"];

/// Upstream's `propertyConversion`, less `custom` on its own (see `kind_of`).
fn kind_of(property: &str) -> Option<Kind> {
    Some(match property {
        "title" | "note" | "cue" => Kind::Text,
        "skip" | "flag" | "countToEnd" | "linkStart" => Kind::Flag,
        "colour" => Kind::Colour,
        "timeWarning" | "timeDanger" | "targetDuration" | "duration" | "timeStart" | "timeEnd" => {
            Kind::Time
        }
        "endAction" => Kind::OneOf(END_ACTIONS),
        "timerType" => Kind::OneOf(TIMER_TYPES),
        "timeStrategy" => Kind::OneOf(TIME_STRATEGIES),
        // Upstream also lists a bare `custom`, which writes a string over the
        // entry's whole map of custom values. Left out on purpose: a field is
        // changed as `custom:<key>`.
        _ => return None,
    })
}

fn read(kind: Kind, value: &Value) -> Result<Value, IntegrationError> {
    Ok(match kind {
        Kind::Text => Value::String(coerce_string(value)?),
        Kind::Flag => Value::Bool(coerce_boolean(value)?),
        Kind::Colour => Value::String(colour(value)?),
        Kind::Time => {
            let millis = coerce_number(value)?;
            if !(0.0..=MAX_DURATION).contains(&millis) {
                return Err(IntegrationError::new("Times should be from 0 to 23:59:59"));
            }
            whole_or_fraction(millis)
        }
        Kind::OneOf(allowed) => match value.as_str() {
            Some(text) if allowed.contains(&text) => Value::String(text.to_string()),
            _ => return Err(IntegrationError::new("Invalid value received")),
        },
    })
}

fn whole_or_fraction(number: f64) -> Value {
    match number.fract() == 0.0 {
        true => json!(number as i64),
        false => json!(number),
    }
}

/// `coerceColour`: a hex colour or a CSS colour name, lowercased; empty for none.
fn colour(value: &Value) -> Result<String, IntegrationError> {
    let Some(text) = value.as_str() else {
        return Err(IntegrationError::new("Invalid colour value received"));
    };
    let lower = text.to_lowercase();
    match lower.as_str() {
        hex if hex.starts_with('#') => match is_colour_hex(hex) {
            true => Ok(lower),
            false => Err(IntegrationError::new("Invalid hex colour received")),
        },
        "" => Ok(lower),
        name if is_css_colour(name) => Ok(lower),
        _ => Err(IntegrationError::new("Invalid colour name received")),
    }
}

/// `isValidChangeProperty`: the entry has the field, or it is a custom field
/// the project defines and the entry can hold custom values.
fn is_valid(entry: &Value, property: &str, custom_fields: &Value) -> bool {
    let has = |key: &str| entry.get(key).is_some();
    match property.strip_prefix("custom:") {
        Some(key) if has("custom") => !key.is_empty() && custom_fields.get(key).is_some(),
        _ => has(property),
    }
}

/// The patch `fields` make of entry `id` in the loaded rundown, checked as
/// upstream checks it, without changing anything.
pub fn patch_for(
    project: &Project,
    id: &str,
    fields: &[(String, Value)],
) -> Result<Map<String, Value>, IntegrationError> {
    let Some(entry) = project.current().and_then(|rundown| rundown.get(id)) else {
        return Err(IntegrationError::new("ID do not exits in rundown"));
    };

    let mut patch = Map::new();
    let mut custom = Map::new();
    for (property, value) in fields {
        if !is_valid(entry, property, &project.custom_fields) {
            return Err(IntegrationError::new("Invalid property or value"));
        }
        if property.starts_with("custom:") {
            // upstream keys the value by what lies between the first and a
            // second colon, if there is one
            let key = property.split(':').nth(1).unwrap_or_default();
            custom.insert(key.to_string(), Value::String(coerce_string(value)?));
            continue;
        }
        let Some(kind) = kind_of(property) else {
            return Err(IntegrationError::new(format!(
                "Property {property} not permitted"
            )));
        };
        patch.insert(property.clone(), read(kind, value)?);
    }
    if !custom.is_empty() {
        patch.insert("custom".to_string(), Value::Object(custom));
    }
    Ok(patch)
}

/// Applies `fields` to entry `id` of the loaded rundown.
pub fn apply(
    project: &mut Project,
    id: &str,
    fields: &[(String, Value)],
) -> Result<Reply, IntegrationError> {
    let patch = patch_for(project, id, fields)?;
    let rundown_id = project.loaded.clone();
    if let Some(rundown) = project.get_mut(&rundown_id) {
        rundown.edit_entry(id, &Value::Object(patch));
    }
    // an edit can take the loaded event out of play, as a skip does
    project.reconcile_runtime();
    Ok(Reply::success(Change::Rundown { rundown_id }))
}
