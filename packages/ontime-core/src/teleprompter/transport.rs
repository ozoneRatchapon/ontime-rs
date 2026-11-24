//! The shared transport: the reading position over time, like the timer.
//! `position = row(anchor) + speed × (now − at)`, held at the edge playback
//! runs to. Times are the room's time of day in milliseconds, the clock every
//! client shares. Ported from upstream's `teleprompterTransport.ts` and the
//! integration command parser.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::position::{reanchor, start_of_event, Anchor, Layout};
use super::script::ScriptEvent;
use crate::rundown::MS_PER_DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Playback {
    Play,
    Pause,
}

/// How playback reads: stopping at the end of each event and following the
/// loaded event, or reading the whole script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Event,
    Script,
}

/// Where playback stopped of its own accord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stop {
    Event,
    Script,
}

/// Following screens show nothing while playback carries on underneath.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Blackout {
    On,
    Off,
}

pub const DEFAULT_SPEED: f64 = 25.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transport {
    pub playback: Playback,
    pub mode: Mode,
    /// Lines per minute, 1 to 100 either way; negative reads back up.
    pub speed: f64,
    /// Where the reader was when the transport last changed; `None` while the script is empty.
    pub anchor: Option<Anchor>,
    /// When the anchor was true, as the room's time of day in milliseconds.
    pub at: f64,
    pub ended: Option<Stop>,
    pub blackout: Blackout,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playback: Playback::Pause,
            mode: Mode::Event,
            speed: DEFAULT_SPEED,
            anchor: None,
            at: 0.0,
            ended: None,
            blackout: Blackout::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingEvent {
    pub id: String,
    pub cue: String,
    pub title: String,
}

/// The teleprompter as people and integrations read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub playback: Playback,
    pub mode: Mode,
    pub speed: f64,
    pub event: Option<ReadingEvent>,
    pub ended: Option<Stop>,
    pub blackout: Blackout,
}

/// What following screens count from: `row + speed × (now − at)`, held at
/// `until` while playing. It changes only on commands, at boundaries and with
/// the script, so a screen joining late calculates the same position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sync {
    /// The script revision these rows belong to.
    pub revision: u64,
    pub playback: Playback,
    pub speed: f64,
    pub row: f64,
    pub until: f64,
    pub at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeCommand {
    Event,
    Script,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlackoutCommand {
    On,
    Off,
    Toggle,
}

/// A command on the transport, with any event already resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TransportCommand {
    Play,
    Pause,
    Toggle,
    Speed { value: f64 },
    SpeedBy { value: f64 },
    Scroll { lines: f64 },
    Next,
    Previous,
    Goto {
        #[serde(rename = "eventId")]
        event_id: String,
    },
    Top,
    Mode { mode: ModeCommand },
    Blackout { blackout: BlackoutCommand },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoadedTarget {
    Loaded,
}

/// Which event a `goto` names. An index counts the script's events from 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GotoTarget {
    Cue { cue: String },
    Id { id: String },
    Index { index: u64 },
    Loaded(LoadedTarget),
}

/// A command as it arrives from a screen or an integration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Speed { value: f64 },
    SpeedBy { value: f64 },
    Scroll { lines: f64 },
    Next,
    Previous,
    Goto { target: GotoTarget },
    Top,
    Mode { mode: ModeCommand },
    Blackout { blackout: BlackoutCommand },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    Invalid,
    EventNotFound,
}

impl CommandError {
    pub fn message(self) -> &'static str {
        match self {
            CommandError::Invalid => "Invalid teleprompter command",
            CommandError::EventNotFound => "Event not found",
        }
    }
}

/// A client's copy of the clock can run behind; playback is capped at a day
/// less this before it reads as no time passed.
const CLOCK_LAG_TOLERANCE: f64 = 60_000.0;
/// Positions closer than this are the same place.
const BOUNDARY_EPSILON: f64 = 1e-6;
/// Previous restarts the event once the reader is this far into its text.
const PREVIOUS_RESTART_LINES: f64 = 1.0;
const MS_PER_MINUTE: f64 = 60_000.0;

fn sign(value: f64) -> f64 {
    match value {
        v if v > 0.0 => 1.0,
        v if v < 0.0 => -1.0,
        _ => 0.0,
    }
}

/// Between 1 and 100 lines per minute either way. Playback always moves, so 0
/// is the slowest speed in the current direction.
fn clamp_speed(speed: f64, current: f64) -> f64 {
    let direction = [sign(speed), sign(current), 1.0].into_iter().find(|d| *d != 0.0).unwrap_or(1.0);
    direction * speed.abs().clamp(1.0, 100.0)
}

/// A step across 0 stops at the slowest speed; only from there does it turn around.
fn step_speed(current: f64, step: f64) -> f64 {
    let speed = current + step;
    if sign(speed) == sign(current) {
        return clamp_speed(speed, current);
    }
    match current.abs() == 1.0 {
        true => -current,
        false => sign(current),
    }
}

/// Times are times of day: one before the change was taken after midnight,
/// unless it is just before, a clock running a little behind.
fn elapsed_since(at: f64, now: f64) -> f64 {
    let day = MS_PER_DAY as f64;
    let elapsed = match now < at {
        true => now - at + day,
        false => now - at,
    };
    match elapsed > day - CLOCK_LAG_TOLERANCE {
        true => 0.0,
        false => elapsed,
    }
}

fn travel(from: f64, speed: f64, elapsed: f64, bound: f64) -> f64 {
    let row = from + (speed * elapsed) / MS_PER_MINUTE;
    match speed > 0.0 {
        true => row.min(bound),
        false => row.max(bound),
    }
}

/// Event by event, the edge of the event being read (its end reading on, its
/// start reading back); otherwise the edge of the script.
fn playback_bound(layout: &Layout, from: f64, mode: Mode, speed: f64) -> (f64, Stop) {
    let (Some(first), Some(last)) = (layout.events.first(), layout.events.last()) else {
        return (from, Stop::Script);
    };
    if speed > 0.0 {
        let next = match mode {
            Mode::Event => layout.events.iter().find(|event| event.end_row as f64 > from + BOUNDARY_EPSILON),
            Mode::Script => None,
        };
        return match next {
            Some(next) if next.id != last.id => (next.end_row as f64, Stop::Event),
            _ => (from.max(last.end_row as f64), Stop::Script),
        };
    }
    let reading = match mode {
        Mode::Event => layout.events.iter().rev().find(|event| (event.start_row as f64) < from - BOUNDARY_EPSILON),
        Mode::Script => None,
    };
    match reading {
        Some(reading) if reading.id != first.id => (reading.start_row as f64, Stop::Event),
        _ => (from.min(first.start_row as f64), Stop::Script),
    }
}

/// The position, calculated from the transport and the clock. `None` while the
/// anchor is not in the script, as between an edit and its refetch.
pub fn position_at(transport: &Transport, layout: &Layout, now: f64) -> Option<f64> {
    let Some(anchor) = &transport.anchor else {
        return Some(0.0);
    };
    let from = layout.row_of(anchor)?;
    if transport.playback != Playback::Play {
        return Some(from);
    }
    let (bound, _) = playback_bound(layout, from, transport.mode, transport.speed);
    Some(travel(from, transport.speed, elapsed_since(transport.at, now), bound))
}

/// Brings the transport up to a moment: playback which reached its end has
/// stopped there. An empty script has nothing to play.
pub fn settle(transport: &Transport, layout: &Layout, now: f64) -> Transport {
    if transport.playback != Playback::Play {
        return transport.clone();
    }
    let Some(anchor) = &transport.anchor else {
        return Transport { playback: Playback::Pause, at: now, ..transport.clone() };
    };
    let Some(from) = layout.row_of(anchor) else {
        return transport.clone();
    };
    let (bound, stop) = playback_bound(layout, from, transport.mode, transport.speed);
    let ms_to_bound = ((bound - from) / transport.speed) * MS_PER_MINUTE;
    if elapsed_since(transport.at, now) < ms_to_bound {
        return transport.clone();
    }
    Transport {
        playback: Playback::Pause,
        anchor: layout.anchor_at(bound),
        at: now,
        ended: Some(stop),
        ..transport.clone()
    }
}

/// When playback next reaches its bound, in milliseconds from `now`, so the
/// host can wake then instead of ticking. `None` unless playing.
pub fn ms_until_settled(transport: &Transport, layout: &Layout, now: f64) -> Option<f64> {
    if transport.playback != Playback::Play {
        return None;
    }
    let from = layout.row_of(transport.anchor.as_ref()?)?;
    let (bound, _) = playback_bound(layout, from, transport.mode, transport.speed);
    let ms_to_bound = ((bound - from) / transport.speed) * MS_PER_MINUTE;
    Some((ms_to_bound - elapsed_since(transport.at, now)).max(0.0))
}

/// Applies a command. Moving the reader keeps the play state, and only
/// playing or moving the reader clears where playback ended. Every command
/// sets the time, so a repeated command is still a change.
pub fn apply_command(
    transport: &Transport,
    command: &TransportCommand,
    layout: &Layout,
    now: f64,
) -> Result<Transport, CommandError> {
    let current = settle(transport, layout, now);
    let is_playing = current.playback == Playback::Play;
    let position = position_at(&current, layout, now).unwrap_or(0.0);
    // only playback moves the reader off their anchor, which a row would round to the start of its line
    let anchor = match is_playing {
        true => layout.anchor_at(position),
        false => current.anchor.clone(),
    };
    let rebased = Transport { anchor, at: now, ..current.clone() };

    let move_to = |row: f64| {
        let last_row = layout.events.last().map_or(0, |event| event.end_row) as f64;
        let target = row.max(0.0).min(last_row);
        Transport { anchor: layout.anchor_at(target), ended: None, ..rebased.clone() }
    };
    let move_to_event = |event_id: Option<&str>| match event_id {
        Some(id) => Transport { anchor: Some(start_of_event(id)), ended: None, ..rebased.clone() },
        None => rebased.clone(),
    };
    let reader = layout.event_at(position);
    let reader_index = reader.and_then(|reader| layout.events.iter().position(|event| event.id == reader.id));
    let event_id_at = |index: Option<usize>| index.and_then(|i| layout.events.get(i)).map(|event| event.id.as_str());

    let next = match command {
        TransportCommand::Play => Transport { playback: Playback::Play, ended: None, ..rebased },
        TransportCommand::Pause => Transport { playback: Playback::Pause, ..rebased },
        TransportCommand::Toggle => match is_playing {
            true => Transport { playback: Playback::Pause, ..rebased },
            false => Transport { playback: Playback::Play, ended: None, ..rebased },
        },
        TransportCommand::Speed { value } => Transport { speed: clamp_speed(*value, current.speed), ..rebased },
        TransportCommand::SpeedBy { value } => Transport { speed: step_speed(current.speed, *value), ..rebased },
        TransportCommand::Scroll { lines } => move_to(position + lines),
        TransportCommand::Next => move_to_event(event_id_at(Some(reader_index.map_or(0, |i| i + 1)))),
        TransportCommand::Previous => {
            let is_past_start = reader.is_some_and(|reader| position >= reader.start_row as f64 + PREVIOUS_RESTART_LINES);
            match is_past_start {
                true => move_to_event(reader.map(|reader| reader.id.as_str())),
                false => move_to_event(event_id_at(reader_index.and_then(|i| i.checked_sub(1)))),
            }
        }
        TransportCommand::Goto { event_id } => match layout.event(event_id) {
            Some(_) => move_to_event(Some(event_id)),
            None => return Err(CommandError::EventNotFound),
        },
        TransportCommand::Top => move_to_event(event_id_at(Some(0))),
        TransportCommand::Mode { mode } => Transport {
            mode: match (mode, current.mode) {
                (ModeCommand::Event, _) | (ModeCommand::Toggle, Mode::Script) => Mode::Event,
                (ModeCommand::Script, _) | (ModeCommand::Toggle, Mode::Event) => Mode::Script,
            },
            ..rebased
        },
        TransportCommand::Blackout { blackout } => Transport {
            blackout: match (blackout, current.blackout) {
                (BlackoutCommand::On, _) | (BlackoutCommand::Toggle, Blackout::Off) => Blackout::On,
                (BlackoutCommand::Off, _) | (BlackoutCommand::Toggle, Blackout::On) => Blackout::Off,
            },
            ..rebased
        },
    };
    // playing from where playback has nowhere to go stops it straight away
    Ok(settle(&next, layout, now))
}

/// The reader keeps their place in the text when the script changes, and
/// playback carries on from there without a jump.
pub fn reanchor_transport(
    transport: &Transport,
    previous: (&[ScriptEvent], &Layout),
    next: (&[ScriptEvent], &Layout),
    now: f64,
) -> Transport {
    let current = settle(transport, previous.1, now);
    let is_playing = current.playback == Playback::Play;
    let position = match is_playing {
        true => position_at(&current, previous.1, now),
        false => None,
    };
    let place = match position {
        Some(row) => previous.1.anchor_at(row),
        None => current.anchor.clone(),
    };
    let anchor = reanchor(previous.0, next.0, place.as_ref());
    if !is_playing && anchor == current.anchor {
        return current;
    }
    settle(&Transport { anchor, at: now, ..current }, next.1, now)
}

/// What every screen and the host show. Time only changes it when playback
/// stops at an end or the reader reaches another event.
pub fn describe(transport: &Transport, layout: &Layout, now: f64) -> State {
    let current = settle(transport, layout, now);
    let event = current
        .anchor
        .as_ref()
        .and_then(|_| position_at(&current, layout, now))
        .and_then(|position| layout.event_at(position))
        .map(|reader| ReadingEvent { id: reader.id.clone(), cue: reader.cue.clone(), title: reader.title.clone() });
    State {
        playback: current.playback,
        mode: current.mode,
        speed: current.speed,
        event,
        ended: current.ended,
        blackout: current.blackout,
    }
}

/// Only the host which holds the transport resolves its anchor, so every
/// screen counts from the same row.
pub fn describe_sync(transport: &Transport, layout: &Layout, revision: u64) -> Sync {
    let row = transport.anchor.as_ref().and_then(|anchor| layout.row_of(anchor)).unwrap_or(0.0);
    let until = match transport.playback {
        Playback::Play => playback_bound(layout, row, transport.mode, transport.speed).0,
        Playback::Pause => row,
    };
    Sync { revision, playback: transport.playback, speed: transport.speed, row, until, at: transport.at }
}

pub fn sync_position_at(sync: &Sync, now: f64) -> f64 {
    match sync.playback {
        Playback::Play => travel(sync.row, sync.speed, elapsed_since(sync.at, now), sync.until),
        Playback::Pause => sync.row,
    }
}

/// Parses a command from a screen or an integration. Integrations send values
/// in paths, so numbers may arrive as strings.
pub fn parse_command(payload: &Value) -> Result<Command, CommandError> {
    let simple = match payload.as_str() {
        Some("play") => Some(Command::Play),
        Some("pause") => Some(Command::Pause),
        Some("toggle") => Some(Command::Toggle),
        Some("next") => Some(Command::Next),
        Some("previous") => Some(Command::Previous),
        Some("top") => Some(Command::Top),
        _ => None,
    };
    if let Some(command) = simple {
        return Ok(command);
    }
    let Some(object) = payload.as_object() else {
        return Err(CommandError::Invalid);
    };

    if let Some(value) = object.get("speed") {
        return Ok(Command::Speed { value: to_number(value)? });
    }
    if let Some(value) = object.get("speedBy") {
        return Ok(Command::SpeedBy { value: to_number(value)? });
    }
    if let Some(value) = object.get("scroll") {
        return Ok(Command::Scroll { lines: to_number(value)? });
    }
    if let Some(mode) = object.get("mode").and_then(|mode| serde_json::from_value::<ModeCommand>(mode.clone()).ok()) {
        return Ok(Command::Mode { mode });
    }
    if let Some(blackout) =
        object.get("blackout").and_then(|value| serde_json::from_value::<BlackoutCommand>(value.clone()).ok())
    {
        return Ok(Command::Blackout { blackout });
    }
    match object.get("goto") {
        Some(Value::String(target)) if target == "loaded" => {
            Ok(Command::Goto { target: GotoTarget::Loaded(LoadedTarget::Loaded) })
        }
        Some(Value::Object(goto)) => parse_goto(goto).map(|target| Command::Goto { target }),
        _ => Err(CommandError::Invalid),
    }
}

fn parse_goto(goto: &serde_json::Map<String, Value>) -> Result<GotoTarget, CommandError> {
    match goto.get("cue") {
        Some(Value::String(cue)) => return Ok(GotoTarget::Cue { cue: cue.clone() }),
        Some(Value::Number(cue)) => return Ok(GotoTarget::Cue { cue: cue.to_string() }),
        _ => {}
    }
    if let Some(Value::String(id)) = goto.get("id") {
        return Ok(GotoTarget::Id { id: id.clone() });
    }
    let index = goto.get("index").map(to_number).transpose()?.ok_or(CommandError::Invalid)?;
    match index.fract() == 0.0 && index > 0.0 {
        true => Ok(GotoTarget::Index { index: index as u64 }),
        false => Err(CommandError::Invalid),
    }
}

/// JavaScript's `Number(value)` for a non-empty string, the value itself for a
/// number; anything else, or anything not finite, is invalid.
fn to_number(value: &Value) -> Result<f64, CommandError> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) if !text.trim().is_empty() => js_number(text.trim()),
        _ => None,
    };
    number.filter(|n| n.is_finite()).ok_or(CommandError::Invalid)
}

fn js_number(text: &str) -> Option<f64> {
    let radix = |prefix: &str, radix: u32| {
        text.strip_prefix(prefix).and_then(|digits| u64::from_str_radix(digits, radix).ok()).map(|n| n as f64)
    };
    radix("0x", 16)
        .or_else(|| radix("0X", 16))
        .or_else(|| radix("0o", 8))
        .or_else(|| radix("0b", 2))
        // Rust also reads "inf" and "nan", which JavaScript does not; both end up not finite
        .or_else(|| text.parse::<f64>().ok())
}
