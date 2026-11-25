//! `(tag, payload)` into a typed [`Action`], with upstream's validation and
//! messages (`integration.controller.ts`, `message.utils.ts`,
//! `coerceType.ts`). After this, nothing looks at the JSON shape again.

use serde_json::{Map, Value};

use super::js;
use super::types::{ClientCommand, IntegrationError};
use crate::aux_timer::{AuxCommand, SimpleDirection};
use crate::command::Command;
use crate::message::{MessagePatch, TimerMessagePatch};
use crate::offset::OffsetMode;
use crate::teleprompter::transport::{parse_command, Command as PrompterCommand};

/// Upstream's MILLIS_PER_HOUR: the most one `addtime` may add or remove.
const MAX_ADD_TIME: f64 = 3_600_000.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Version,
    /// The whole `runtime-data` payload.
    Poll,
    /// Edit one entry. The fields are checked against the entry itself, which
    /// only dispatch can see.
    Change {
        id: String,
        fields: Vec<(String, Value)>,
    },
    Message(MessagePatch),
    /// `failure` is the message for a command that has to take effect and did
    /// not, worded for what was asked ("Unable to start CUE: 3").
    Playback {
        command: Command,
        failure: Option<String>,
    },
    AuxTimer {
        index: usize,
        command: AuxCommand,
    },
    Client(ClientCommand),
    Teleprompter(PrompterCommand),
    /// Accepted and does nothing, as `addtime 0`.
    Nothing,
}

type Parsed = Result<Action, IntegrationError>;

fn fail<T>(message: impl Into<String>) -> Result<T, IntegrationError> {
    Err(IntegrationError::new(message))
}

/// Upstream dispatchFromAdapter: the tag is matched case-insensitively.
pub fn parse(tag: &str, payload: Option<&Value>) -> Parsed {
    match tag.to_lowercase().as_str() {
        "version" => Ok(Action::Version),
        "poll" => Ok(Action::Poll),
        "change" => change(payload),
        "message" => message(payload),
        "start" => start(payload),
        "load" => load(payload),
        "pause" => Ok(playback(Command::Pause)),
        "stop" => Ok(playback(Command::Stop)),
        "reload" => Ok(playback(Command::Reload)),
        "roll" => Ok(playback(Command::Roll)),
        "addtime" => add_time(payload),
        "auxtimer" => aux_timer(payload),
        "client" => client(payload),
        "offsetmode" => offset_mode(payload),
        "teleprompter" => teleprompter(payload),
        _ => fail(format!("Unhandled message {tag}")),
    }
}

fn playback(command: Command) -> Action {
    Action::Playback {
        command,
        failure: None,
    }
}

fn must(command: Command, failure: impl Into<String>) -> Parsed {
    Ok(Action::Playback {
        command,
        failure: Some(failure.into()),
    })
}

// #region coercion (upstream assert.ts and coerceType.ts) ======

/// `assert.isObject`: a plain object, not an array and not null.
fn object(payload: Option<&Value>) -> Result<&Map<String, Value>, IntegrationError> {
    match payload {
        Some(Value::Object(map)) => Ok(map),
        other => fail(unexpected(other)),
    }
}

fn unexpected(value: Option<&Value>) -> String {
    format!(
        "Unexpected payload type: {}",
        js::string_or_undefined(value)
    )
}

/// `numberOrError`.
fn number(value: Option<&Value>) -> Result<f64, IntegrationError> {
    value
        .and_then(js::number)
        .ok_or_else(|| IntegrationError::new("Payload is not a valid number"))
}

/// `coerceString`.
pub(crate) fn coerce_string(value: &Value) -> Result<String, IntegrationError> {
    match value {
        Value::Null | Value::Object(_) | Value::Array(_) => fail("Invalid value received"),
        other => Ok(js::string(other)),
    }
}

/// `coerceBoolean`.
pub(crate) fn coerce_boolean(value: &Value) -> Result<bool, IntegrationError> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(text) => match text.to_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" | "" => Ok(false),
            _ => fail("Invalid value received"),
        },
        Value::Number(n) if n.as_f64() == Some(0.0) => Ok(false),
        Value::Number(n) if n.as_f64() == Some(1.0) => Ok(true),
        _ => fail("Invalid value received"),
    }
}

/// `coerceNumber`: `null` is refused, unlike `Number(null)`.
pub(crate) fn coerce_number(value: &Value) -> Result<f64, IntegrationError> {
    match value {
        Value::Null => fail("Invalid value received"),
        other => js::number(other).ok_or_else(|| IntegrationError::new("Invalid value received")),
    }
}

/// `decodeURI(coerceString(value))`, as the overlay texts are read.
fn decoded_text(value: &Value) -> Result<String, IntegrationError> {
    js::decode_uri(&coerce_string(value)?).map_err(|_| IntegrationError::new("URI malformed"))
}

/// `extractCue`: a number reads as its digits.
fn cue(value: &Value) -> Result<String, IntegrationError> {
    match value {
        Value::Number(_) | Value::String(_) => Ok(js::string(value)),
        _ => fail("Payload is not a valid string or number"),
    }
}

// #endregion ===================================================
// #region actions ==============================================

fn change(payload: Option<&Value>) -> Parsed {
    let payload = object(payload)?;
    let Some((id, data)) = payload.iter().next() else {
        return fail("Payload is empty");
    };
    if id.is_empty() {
        return fail("Missing Event ID");
    }
    Ok(Action::Change {
        id: id.clone(),
        fields: object_entries(data)?,
    })
}

/// `Object.entries(value)`: a string's characters by position, an array's
/// items, an object's fields, nothing for a number or a boolean.
fn object_entries(value: &Value) -> Result<Vec<(String, Value)>, IntegrationError> {
    Ok(match value {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        Value::String(text) => text
            .encode_utf16()
            .enumerate()
            .map(|(i, unit)| {
                let character = String::from_utf16_lossy(&[unit]);
                (i.to_string(), Value::String(character))
            })
            .collect(),
        Value::Null => return fail("Cannot convert undefined or null to object"),
        Value::Bool(_) | Value::Number(_) => Vec::new(),
    })
}

fn message(payload: Option<&Value>) -> Parsed {
    let payload = object(payload)?;
    let timer = payload.get("timer").map(timer_message).transpose()?;
    let secondary = payload.get("secondary").map(decoded_text).transpose()?;
    Ok(Action::Message(MessagePatch { timer, secondary }))
}

/// `validateTimerMessage`.
fn timer_message(value: &Value) -> Result<TimerMessagePatch, IntegrationError> {
    let fields = object(Some(value))?;
    let flag = |key: &str| fields.get(key).map(coerce_boolean).transpose();
    Ok(TimerMessagePatch {
        text: fields.get("text").map(decoded_text).transpose()?,
        visible: flag("visible")?,
        blink: flag("blink")?,
        blackout: flag("blackout")?,
        // anything but a known source clears it, as upstream coerceSecondary
        secondary_source: fields.get("secondarySource").map(|source| {
            source
                .as_str()
                .filter(|s| matches!(*s, "aux1" | "aux2" | "aux3" | "secondary"))
                .map(str::to_string)
        }),
    })
}

/// Which event `start` or `load` asks for, before it is looked up.
enum Target {
    Index(usize, String),
    Id(String),
    Cue(String),
    Next,
    Previous,
}

/// The shared half of `start` and `load`. `None` for a payload that names no
/// target, which the two answer differently.
fn target(payload: Option<&Value>) -> Result<Option<Target>, IntegrationError> {
    match payload {
        Some(Value::Object(fields)) => {
            if fields.contains_key("index") {
                let index = number(fields.get("index"))?;
                let shown = js::number_to_string(index);
                if index <= 0.0 {
                    return fail(format!("Event index out of range {shown}"));
                }
                // the operator counts from 1, and only whole positions exist
                let not_found = format!("Event index not recognised or out of range {shown}");
                if index.fract() != 0.0 || index > usize::MAX as f64 {
                    return fail(not_found);
                }
                return Ok(Some(Target::Index(index as usize - 1, shown)));
            }
            if let Some(id) = fields.get("id") {
                return match id {
                    Value::String(id) => Ok(Some(Target::Id(id.clone()))),
                    other => fail(unexpected(Some(other))),
                };
            }
            if let Some(value) = fields.get("cue") {
                return Ok(Some(Target::Cue(cue(value)?)));
            }
            Ok(None)
        }
        Some(Value::String(word)) if word == "next" => Ok(Some(Target::Next)),
        Some(Value::String(word)) if word == "previous" => Ok(Some(Target::Previous)),
        _ => Ok(None),
    }
}

fn start(payload: Option<&Value>) -> Parsed {
    let Some(payload) = payload else {
        return must(Command::Start, "Unable to start");
    };
    match target(Some(payload))? {
        Some(Target::Index(index, shown)) => must(
            Command::StartIndex(index),
            format!("Event index not recognised or out of range {shown}"),
        ),
        Some(Target::Id(id)) => must(
            Command::StartId(id.clone()),
            format!("Unable to start ID: {id}"),
        ),
        Some(Target::Cue(cue)) => must(
            Command::StartCue(cue.clone()),
            format!("Unable to start CUE: {cue}"),
        ),
        Some(Target::Next) => must(Command::StartNext, "Unable to start next event"),
        Some(Target::Previous) => must(Command::StartPrevious, "Unable to start previous event"),
        None => fail("No matching start function"),
    }
}

fn load(payload: Option<&Value>) -> Parsed {
    match target(payload)? {
        Some(Target::Index(index, shown)) => must(
            Command::LoadIndex(index),
            format!("Event index not recognised or out of range {shown}"),
        ),
        Some(Target::Id(id)) => must(
            Command::LoadId(id.clone()),
            format!("Unable to load ID: {id}"),
        ),
        Some(Target::Cue(cue)) => must(
            Command::LoadCue(cue.clone()),
            format!("Unable to load CUE: {cue}"),
        ),
        Some(Target::Next) => must(Command::LoadNext, "Unable to load next event"),
        Some(Target::Previous) => must(Command::LoadPrevious, "Unable to load previous event"),
        None => fail("No matching method provided"),
    }
}

fn add_time(payload: Option<&Value>) -> Parsed {
    let time = match payload {
        Some(Value::Object(fields)) if fields.contains_key("add") => number(fields.get("add"))?,
        Some(Value::Object(fields)) if fields.contains_key("remove") => {
            -number(fields.get("remove"))?
        }
        other => number(other)?,
    };
    if time == 0.0 {
        return Ok(Action::Nothing);
    }
    if time.abs() > MAX_ADD_TIME {
        return fail(format!("Payload too large: {}", js::number_to_string(time)));
    }
    // the runtime counts whole milliseconds
    Ok(playback(Command::AddTime(time.round() as i64)))
}

fn aux_timer(payload: Option<&Value>) -> Parsed {
    let fields = object(payload)?;
    let (index, command) = match fields.iter().next() {
        Some((key, command)) if matches!(key.as_str(), "1" | "2" | "3") => (key, command),
        _ => return fail("Invalid auxtimer index"),
    };
    let index: usize = index.parse().unwrap_or(1);
    let command = match command {
        Value::String(word) => match word.as_str() {
            "start" => AuxCommand::Start,
            "pause" => AuxCommand::Pause,
            "stop" => AuxCommand::Stop,
            _ => return fail("Invalid command"),
        },
        Value::Object(patch) if patch.contains_key("duration") => {
            AuxCommand::SetTime(number(patch.get("duration"))?.round() as i64)
        }
        Value::Object(patch) if patch.contains_key("addtime") => {
            AuxCommand::AddTime(number(patch.get("addtime"))?.round() as i64)
        }
        Value::Object(patch) if patch.contains_key("direction") => {
            match patch.get("direction").and_then(Value::as_str) {
                Some("count-up") => AuxCommand::Direction(SimpleDirection::CountUp),
                Some("count-down") => AuxCommand::Direction(SimpleDirection::CountDown),
                _ => return fail("Invalid direction payload"),
            }
        }
        _ => return fail("No matching method provided"),
    };
    Ok(Action::AuxTimer { index, command })
}

fn client(payload: Option<&Value>) -> Parsed {
    let fields = object(payload)?;
    let Some(target) = fields.get("target").and_then(Value::as_str) else {
        return fail("No or invalid client target");
    };
    let target = target.to_string();
    let command = match (
        fields.get("rename").and_then(Value::as_str),
        fields.get("redirect").and_then(Value::as_str),
        fields.get("identify").and_then(Value::as_bool),
    ) {
        (Some(name), _, _) => ClientCommand::Rename {
            target,
            name: name.to_string(),
        },
        (None, Some(path), _) => ClientCommand::Redirect {
            target,
            path: path.to_string(),
        },
        (None, None, Some(identify)) => ClientCommand::Identify { target, identify },
        _ => return fail("No matching method provided"),
    };
    Ok(Action::Client(command))
}

/// `coerceEnum(payload, OffsetMode)`.
fn offset_mode(payload: Option<&Value>) -> Parsed {
    let mode = match payload.and_then(Value::as_str) {
        Some("absolute") => OffsetMode::Absolute,
        Some("relative") => OffsetMode::Relative,
        _ => return fail("Invalid value received"),
    };
    Ok(playback(Command::SetOffsetMode(mode)))
}

fn teleprompter(payload: Option<&Value>) -> Parsed {
    parse_command(payload.unwrap_or(&Value::Null))
        .map(Action::Teleprompter)
        .map_err(|error| IntegrationError::new(error.message()))
}

// #endregion ===================================================
