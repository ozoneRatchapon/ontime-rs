//! Project files: what the Ontime app saves and opens (plan 020), so a show
//! moves between the app and a room both ways.
//!
//! - [`import`] reads a file as upstream's `parseDatabaseModel` does: each
//!   section is taken field by field, what is invalid is dropped or defaulted,
//!   and every rundown is rebuilt the way `parseRundown` rebuilds it.
//! - [`export`] writes the room as a `DatabaseModel`, the file the app opens.
//!
//! Pinned against upstream by `tests/project_file_tests.rs` over
//! `tests/fixtures/project_file.json` (scripts/upstream-oracle).
//!
//! One deliberate difference: a file made by Ontime v3 is refused with a
//! message, where upstream migrates it. v3 is three years old; the app it came
//! from opens it and saves it as v4.

use std::fmt;

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};

use crate::api::restore_into;
use crate::aux_timer::sanitise_aux_timer_names;
use crate::entry_edits::event_times;
use crate::project::{Project, ProjectData, Settings, ViewSettings};
use crate::rundown::Rundown;
use crate::teleprompter::settings::{parse_settings as parse_teleprompter, TeleprompterSettings};

/// Why a file was not taken. Nothing in the room changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    NotJson,
    /// No `settings.version`: upstream's test for a file being its own.
    NotAProjectFile,
    FromVersion3,
    /// A rundown without its `order` or `entries`, which upstream fails on.
    BrokenRundown(String),
}

impl ImportError {
    pub fn message(&self) -> String {
        match self {
            Self::NotJson => "This file is not JSON, so it is not an Ontime project file".into(),
            Self::NotAProjectFile => {
                "This is not an Ontime project file: it has no settings version".into()
            }
            Self::FromVersion3 => {
                "This project file is from Ontime v3. Open it in the Ontime app and save it, then import it here".into()
            }
            Self::BrokenRundown(id) => {
                format!("Rundown \"{id}\" in this file has no order or entries")
            }
        }
    }
}

/// What a file held, and what of it was dropped or defaulted.
#[derive(Debug, Clone)]
pub struct Imported {
    pub project: Project,
    pub warnings: Vec<String>,
}

/// Reads a project file, given as its text so the order of its rundowns is
/// known: the app loads the first one, and so does the room.
pub fn import(text: &str) -> Result<Imported, ImportError> {
    let data: Value = serde_json::from_str(text).map_err(|_| ImportError::NotJson)?;
    if !data.is_object() {
        return Err(ImportError::NotAProjectFile);
    }
    let rundown_keys = serde_json::from_str::<RundownKeys>(text)
        .map(|keys| keys.rundowns.0)
        .unwrap_or_default();
    let mut warnings = Vec::new();

    let settings = parse_project_settings(&data)?;
    let custom_fields = parse_custom_fields(data.get("customFields"), &mut warnings);
    let (rundowns, first) = parse_rundowns(&data, &rundown_keys, &custom_fields, &mut warnings)?;

    let mut project = Project {
        project_id: data
            .get("projectId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map_or_else(make_project_id, str::to_string),
        loaded: first,
        project: parse_project_data(data.get("project"), &mut warnings),
        settings,
        view_settings: parse_view_settings(data.get("viewSettings")),
        url_presets: parse_url_presets(data.get("urlPresets"), &mut warnings),
        custom_fields: Value::Object(custom_fields),
        automation: parse_automation(data.get("automation")),
        teleprompter: parse_teleprompter(
            data.get("teleprompter").unwrap_or(&Value::Null),
            &TeleprompterSettings::default(),
        ),
        ..Project::default()
    };
    project.rundowns = rundowns;
    // each kind of problem once, in the order it was met
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));
    Ok(Imported { project, warnings })
}

/// The room as a project file the Ontime app opens. Only what a file carries:
/// no playback, report, password or activity log.
pub fn export(project: &Project) -> Value {
    let project_id = match project.project_id.as_str() {
        "" => make_project_id(),
        id => id.to_string(),
    };
    json!({
        "projectId": project_id,
        "rundowns": project.rundowns,
        "project": project.project,
        "settings": project.settings,
        "viewSettings": project.view_settings,
        "urlPresets": project.url_presets,
        "customFields": project.custom_fields,
        "automation": project.automation,
        "teleprompter": project.teleprompter,
    })
}

/// Puts an imported file in place of the room's show. The room keeps what
/// belongs to the room rather than the show: its password and members, its
/// audience feed, its activity log and report archive, its CSS, and its clock
/// settings.
pub fn import_into(project: &mut Project, mut imported: Project, now: i64) {
    imported.members_only = project.members_only;
    imported.public_feed = project.public_feed.clone();
    imported.report_archive = std::mem::take(&mut project.report_archive);
    imported.css_override = std::mem::take(&mut project.css_override);
    imported.timezone_offset = project.timezone_offset;
    imported.heartbeat_ms = project.heartbeat_ms;
    restore_into(project, imported, now);
}

fn make_project_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

// #region sections ===============================================

fn parse_project_settings(data: &Value) -> Result<Settings, ImportError> {
    let settings = data
        .get("settings")
        .filter(|s| is_truthy(s))
        .ok_or(ImportError::NotAProjectFile)?;
    let version = settings.get("version").filter(|v| !v.is_null());
    let version = version.ok_or(ImportError::NotAProjectFile)?;
    // upstream migrates v3 first (db.migration.v3 shouldUseThisMigration)
    if version
        .as_str()
        .is_some_and(|v| v.split('.').next() == Some("3"))
    {
        return Err(ImportError::FromVersion3);
    }
    let defaults = Settings::default();
    let text = |key: &str, fallback: &str| {
        settings
            .get(key)
            .and_then(Value::as_str)
            .map_or_else(|| fallback.to_string(), str::to_string)
    };
    let key = |key: &str| {
        settings
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    Ok(Settings {
        version: defaults.version.clone(),
        editor_key: key("editorKey"),
        operator_key: key("operatorKey"),
        time_format: text("timeFormat", &defaults.time_format),
        language: text("language", &defaults.language),
        aux_timer_names: sanitise_aux_timer_names(
            settings.get("auxTimerNames").unwrap_or(&Value::Null),
        ),
    })
}

/// upstream `sanitiseCustomFields`: a field is kept when its label is letters,
/// digits, spaces or underscores and its key is the label made into a key.
fn parse_custom_fields(fields: Option<&Value>, warnings: &mut Vec<String>) -> Map<String, Value> {
    let Some(fields) = fields.and_then(Value::as_object) else {
        warnings.push("Custom Fields: no data found to import".into());
        return Map::new();
    };
    let kept: Map<String, Value> = fields
        .iter()
        .filter_map(|(key, field)| {
            let label = field.get("label")?.as_str()?;
            let colour = field.get("colour")?.as_str()?;
            let kind = field
                .get("type")?
                .as_str()
                .filter(|t| matches!(*t, "text" | "image"))?;
            let valid = !label.is_empty()
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
                && *key == label.trim().replace(' ', "_");
            valid.then(|| {
                (
                    key.clone(),
                    json!({ "type": kind, "colour": colour, "label": label }),
                )
            })
        })
        .collect();
    if kept.len() != fields.len() {
        warnings.push("Custom Fields: skipped invalid custom fields".into());
    }
    kept
}

fn parse_project_data(data: Option<&Value>, warnings: &mut Vec<String>) -> ProjectData {
    let defaults = ProjectData::default();
    let Some(data) = data.filter(|d| is_truthy(d)) else {
        warnings.push("Project: no data found to import".into());
        return defaults;
    };
    let text = |key: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default()
    };
    let custom = match data.get("custom") {
        Some(Value::Array(list)) => {
            let parsed: Vec<Value> = list
                .iter()
                .filter_map(|entry| {
                    let title = entry.get("title")?.as_str()?;
                    let value = entry.get("value")?.as_str()?;
                    let url = match entry.get("url") {
                        None => "",
                        Some(url) => url.as_str()?,
                    };
                    Some(json!({ "title": title, "value": value, "url": url }))
                })
                .collect();
            if parsed.len() != list.len() {
                warnings
                    .push("Project: custom data contained invalid entries, skipping them".into());
            }
            parsed
        }
        None => defaults.custom.clone(),
        Some(_) => {
            warnings.push("Project: custom data is invalid, using defaults".into());
            defaults.custom.clone()
        }
    };
    ProjectData {
        title: text("title"),
        description: text("description"),
        url: text("url"),
        info: text("info"),
        logo: data.get("logo").and_then(Value::as_str).map(str::to_string),
        custom,
    }
}

fn parse_view_settings(data: Option<&Value>) -> ViewSettings {
    let defaults = ViewSettings::default();
    let Some(data) = data.filter(|d| is_truthy(d)) else {
        return defaults;
    };
    let colour = |key: &str, fallback: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .map_or_else(|| fallback.to_string(), str::to_string)
    };
    ViewSettings {
        override_styles: data
            .get("overrideStyles")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.override_styles),
        normal_color: colour("normalColor", &defaults.normal_color),
        warning_color: colour("warningColor", &defaults.warning_color),
        danger_color: colour("dangerColor", &defaults.danger_color),
    }
}

fn parse_url_presets(data: Option<&Value>, warnings: &mut Vec<String>) -> Vec<Value> {
    let Some(presets) = data.and_then(Value::as_array) else {
        return Vec::new();
    };
    presets
        .iter()
        .filter_map(|preset| {
            let alias = preset.get("alias").filter(|a| is_truthy(a));
            let target = preset.get("target").filter(|t| is_truthy(t));
            let (Some(alias), Some(target)) = (alias, target) else {
                warnings.push(format!("URL presets: invalid preset {preset}"));
                return None;
            };
            let mut kept = json!({
                "enabled": or(preset.get("enabled"), json!(false)),
                "alias": alias,
                "target": target,
                "search": or(preset.get("search"), json!("")),
                "displayInNav": or(preset.get("displayInNav"), json!(false)),
            });
            if let Some(options) = preset.get("options") {
                kept["options"] = options.clone();
            }
            Some(kept)
        })
        .collect()
}

fn parse_automation(data: Option<&Value>) -> Value {
    let defaults = Project::default().automation;
    let Some(data) = data.filter(|d| is_truthy(d)) else {
        return defaults;
    };
    let field = |key: &str| or(data.get(key), defaults[key].clone());
    json!({
        "enabledAutomations": field("enabledAutomations"),
        "enabledOscIn": field("enabledOscIn"),
        "oscPortIn": field("oscPortIn"),
        "triggers": data.get("triggers").filter(|t| t.is_array()).cloned().unwrap_or(json!([])),
        "automations": data
            .get("automations")
            .filter(|a| a.is_object() || a.is_array())
            .cloned()
            .unwrap_or(json!({})),
    })
}

// #endregion sections ============================================

// #region rundowns ===============================================

/// upstream `parseRundowns`: every rundown rebuilt, or one empty rundown made
/// for a file without any. Returns them and the id of the first.
fn parse_rundowns(
    data: &Value,
    keys: &[String],
    custom_fields: &Map<String, Value>,
    warnings: &mut Vec<String>,
) -> Result<(std::collections::BTreeMap<String, Rundown>, String), ImportError> {
    let mut rundowns = std::collections::BTreeMap::new();
    let given = data
        .get("rundowns")
        .and_then(Value::as_object)
        .filter(|r| !r.is_empty());
    let Some(given) = given else {
        warnings.push("Rundowns: no data found to import".into());
        let rundown = Rundown {
            revision: 0,
            ..Rundown::new(crate::rundown::new_id(), "Default")
        };
        let id = rundown.id.clone();
        rundowns.insert(id.clone(), rundown);
        return Ok((rundowns, id));
    };
    let mut first = None;
    // the file's order, with any key the reader missed after it
    let ordered = keys
        .iter()
        .filter(|key| given.contains_key(*key))
        .chain(given.keys().filter(|key| !keys.contains(key)));
    for key in ordered {
        let rundown = parse_rundown(key, &given[key], custom_fields, warnings)?;
        first.get_or_insert_with(|| rundown.id.clone());
        rundowns.insert(rundown.id.clone(), rundown);
    }
    let first = first.unwrap_or_default();
    Ok((rundowns, first))
}

/// upstream `parseRundown`.
fn parse_rundown(
    key: &str,
    rundown: &Value,
    custom_fields: &Map<String, Value>,
    warnings: &mut Vec<String>,
) -> Result<Rundown, ImportError> {
    let broken = || ImportError::BrokenRundown(key.to_string());
    let order = rundown
        .get("order")
        .and_then(Value::as_array)
        .ok_or_else(broken)?;
    let entries = rundown
        .get("entries")
        .and_then(Value::as_object)
        .ok_or_else(broken)?;
    let id = rundown
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(crate::rundown::new_id, str::to_string);
    let mut parsed = Rundown {
        title: rundown
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        revision: rundown.get("revision").and_then(Value::as_u64).unwrap_or(1),
        ..Rundown::new(id, "")
    };
    let mut warn = |message: &str| warnings.push(format!("Rundowns: {message}"));
    let mut event_index = 0usize;

    for entry_id in order {
        let entry_id = crate::integration::js::string(entry_id);
        let Some(entry) = entries.get(&entry_id).filter(|e| is_truthy(e)) else {
            warn("could not find referenced event, skipping");
            continue;
        };
        let own_id = entry.get("id").map(crate::integration::js::string);
        if own_id.is_some_and(|own| parsed.order.contains(&own)) {
            warn("ID collision on event import, skipping");
            continue;
        }
        let mut nested_ids = Vec::new();
        let made = match kind(entry) {
            "event" => {
                let event = create_event(entry, event_index, custom_fields);
                event_index += 1;
                event
            }
            "delay" => create_delay(entry, &entry_id),
            "milestone" => create_milestone(entry, &entry_id, custom_fields),
            "group" | "block" => {
                let group_id = entry.get("id").cloned().unwrap_or(Value::Null);
                let listed = entry.get("entries").and_then(Value::as_array);
                for nested_id in listed.into_iter().flatten() {
                    let nested_id = crate::integration::js::string(nested_id);
                    let Some(nested) = entries.get(&nested_id) else {
                        continue;
                    };
                    let mut made = match kind(nested) {
                        "event" => {
                            let event = create_event(nested, event_index, custom_fields);
                            event_index += 1;
                            event
                        }
                        "delay" => create_delay(nested, &nested_id),
                        "milestone" => create_milestone(nested, &nested_id, custom_fields),
                        _ => continue,
                    };
                    made["parent"] = group_id.clone();
                    nested_ids.push(nested_id.clone());
                    parsed.entries.insert(nested_id, made);
                }
                create_group(entry, &entry_id, entries, custom_fields)
            }
            _ => {
                warn("unknown event type, skipping");
                continue;
            }
        };
        parsed.entries.insert(entry_id.clone(), made);
        parsed.order.push(entry_id.clone());
        parsed.flat_order.push(entry_id);
        parsed.flat_order.extend(nested_ids);
    }
    Ok(parsed)
}

fn kind(entry: &Value) -> &str {
    entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// upstream `createEvent(event, index)`: the event definition with the file's
/// values over it, each one checked, numbered by its place if it has no cue.
fn create_event(event: &Value, index: usize, custom_fields: &Map<String, Value>) -> Value {
    let base = crate::rundown::default_entry(Some("event"), "");
    let id = event
        .get("id")
        .cloned()
        .unwrap_or_else(|| json!(crate::rundown::new_id()));
    let times = event_times(&base, event);
    let flag = |key: &str| event.get(key).and_then(Value::as_bool).unwrap_or(false);
    let cue = match event.get("cue").filter(|c| !c.is_null()) {
        None => (index + 1).to_string(),
        Some(cue) => make_string(Some(cue), &(index + 1).to_string()),
    };
    let one_of = |key: &str, allowed: &[&str], fallback: &str| {
        event
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| allowed.contains(v))
            .unwrap_or(fallback)
            .to_string()
    };
    let mut made = json!({
        "id": id,
        "type": "event",
        "flag": flag("flag"),
        "title": make_string(event.get("title"), ""),
        "linkStart": flag("linkStart"),
        "endAction": one_of("endAction", &["load-next", "none", "play-next"], "none"),
        "timerType": one_of("timerType", &["count-down", "count-up", "clock", "none"], "count-down"),
        "countToEnd": flag("countToEnd"),
        "skip": flag("skip"),
        "note": make_string(event.get("note"), ""),
        "colour": make_string(event.get("colour"), ""),
        "delay": 0,
        "dayOffset": 0,
        "gap": 0,
        "cue": cue,
        "parent": null,
        "revision": 0,
        "timeWarning": or(event.get("timeWarning"), json!(120_000)),
        "timeDanger": or(event.get("timeDanger"), json!(60_000)),
        "custom": clean_custom(event.get("custom"), custom_fields),
        "triggers": or(event.get("triggers"), json!([])),
    });
    if let Some(object) = made.as_object_mut() {
        object.extend(times);
    }
    made
}

/// upstream `createDelay({ duration, id })`.
fn create_delay(delay: &Value, id: &str) -> Value {
    json!({
        "type": "delay",
        "id": id,
        "duration": or(delay.get("duration"), json!(0)),
        "parent": null,
    })
}

/// upstream `createMilestone({ ...milestone, id })`.
fn create_milestone(milestone: &Value, id: &str, custom_fields: &Map<String, Value>) -> Value {
    json!({
        "id": id,
        "type": "milestone",
        "cue": or(milestone.get("cue"), json!("")),
        "title": or(milestone.get("title"), json!("")),
        "note": or(milestone.get("note"), json!("")),
        "colour": make_string(milestone.get("colour"), ""),
        "custom": clean_custom(milestone.get("custom"), custom_fields),
        "parent": or(milestone.get("parent"), Value::Null),
        "revision": 0,
    })
}

/// upstream `createGroup({ ...group, id })`, listing only entries the file has.
fn create_group(
    group: &Value,
    id: &str,
    entries: &Map<String, Value>,
    custom_fields: &Map<String, Value>,
) -> Value {
    let listed: Vec<Value> = match group.get("entries").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter(|e| entries.contains_key(&crate::integration::js::string(e)))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    json!({
        "id": id,
        "type": "group",
        "title": or(group.get("title"), json!("")),
        "note": or(group.get("note"), json!("")),
        "entries": listed,
        "targetDuration": or(group.get("targetDuration"), Value::Null),
        "colour": make_string(group.get("colour"), ""),
        "custom": clean_custom(group.get("custom"), custom_fields),
        "revision": 0,
        "timeStart": null,
        "timeEnd": null,
        "duration": 0,
        "isFirstLinked": false,
    })
}

/// upstream `cleanupCustomFields`: values only for the project's fields, and
/// none that are empty.
fn clean_custom(custom: Option<&Value>, custom_fields: &Map<String, Value>) -> Value {
    let Some(custom) = custom.and_then(Value::as_object) else {
        return json!({});
    };
    Value::Object(
        custom
            .iter()
            .filter(|(key, value)| custom_fields.contains_key(*key) && value.as_str() != Some(""))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

// #endregion rundowns ============================================

// #region helpers ================================================

/// JavaScript truthiness of a value that is there.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `value ?? fallback`.
fn or(value: Option<&Value>, fallback: Value) -> Value {
    value.filter(|v| !v.is_null()).cloned().unwrap_or(fallback)
}

/// upstream `makeString`: text trimmed, a plain object or nothing gives the
/// fallback, anything else its `toString()` trimmed.
fn make_string(value: Option<&Value>, fallback: &str) -> String {
    match value {
        None | Some(Value::Null | Value::Object(_)) => fallback.to_string(),
        Some(other) => crate::integration::js::string(other).trim().to_string(),
    }
}

/// The keys of `rundowns` in the order the file has them, which a
/// `serde_json::Value` does not keep.
#[derive(Deserialize)]
struct RundownKeys {
    #[serde(default)]
    rundowns: KeysInOrder,
}

#[derive(Default)]
struct KeysInOrder(Vec<String>);

impl<'de> Deserialize<'de> for KeysInOrder {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Keys;
        impl<'de> Visitor<'de> for Keys {
            type Value = KeysInOrder;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<KeysInOrder, A::Error> {
                let mut keys = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    map.next_value::<IgnoredAny>()?;
                    keys.push(key);
                }
                Ok(KeysInOrder(keys))
            }
        }
        deserializer.deserialize_map(Keys)
    }
}

// #endregion helpers =============================================
