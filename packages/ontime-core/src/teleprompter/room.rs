//! The room's teleprompter: what a host calls, so no host holds any of its
//! rules. Ported from upstream's `teleprompter.service.ts`, with one change
//! of shape: upstream keeps the transport in memory, and a Durable Object is
//! evicted between messages, so here it is stored with the project
//! ([`Prompter`]) and the script is rebuilt from the project when needed.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::position::Layout;
use super::script::{revision_of, Script};
use super::settings::{custom_field_of_script_source, TeleprompterSettings};
use super::transport::{
    apply_command, describe, describe_sync, ms_until_settled, parse_command, reanchor_transport,
    settle, Command, CommandError, GotoTarget, Playback, State, Sync, Transport, TransportCommand,
};
use crate::project::Project;
use crate::rundown::MS_PER_DAY;

/// The room's shared teleprompter, stored with the project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prompter {
    pub transport: Transport,
    /// Something has read the script. Until then nothing follows the loaded
    /// event, so a room nobody prompts from never builds a script.
    pub built: bool,
}

/// The room's time of day in milliseconds: the clock the transport counts in,
/// and the one every client follows.
pub fn time_of_day(project: &Project, epoch_ms: i64) -> f64 {
    (epoch_ms + project.timezone_offset).rem_euclid(MS_PER_DAY) as f64
}

/// The shared script, cut from the loaded rundown with the room's settings.
///
/// Cached on the project by a fingerprint of exactly what the script reads, so
/// the runtime payload, the wake check and each command do not rewrap every
/// event: hashing the text is far cheaper than cutting it into lines.
pub fn script(project: &Project) -> Arc<Script> {
    let key = fingerprint(project);
    // a poisoned lock only means another thread panicked mid-update; the
    // fingerprint check below still decides whether its content is usable
    let mut cache = project
        .prompter_cache
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match cache.as_ref() {
        Some((cached, script)) if *cached == key => Arc::clone(script),
        _ => {
            let script = Arc::new(script_with(project, &project.teleprompter));
            *cache = Some((key, Arc::clone(&script)));
            script
        }
    }
}

/// Everything [`Script::build`] reads: the settings, the source field's type,
/// and per event of the loaded rundown its id, type, skip, cue, title and text.
fn fingerprint(project: &Project) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    let settings = &project.teleprompter;
    hasher.update(settings.script.as_bytes());
    hasher.update(&settings.chars_per_line.to_le_bytes());
    hasher.update(format!("{:?}", settings.heading).as_bytes());
    let field = custom_field_of_script_source(&settings.script)
        .and_then(|key| project.custom_fields.get(key));
    hasher.update(
        field
            .and_then(|field| field.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes(),
    );

    let Some(rundown) = project.current() else {
        return *hasher.finalize().as_bytes();
    };
    hasher.update(rundown.id.as_bytes());
    let custom_key = custom_field_of_script_source(&settings.script);
    for id in &rundown.flat_order {
        let Some(entry) = rundown.entries.get(id) else {
            continue;
        };
        hasher.update(id.as_bytes());
        let text = match custom_key {
            Some(key) => entry.get("custom").and_then(|custom| custom.get(key)),
            None => entry.get(settings.script.as_str()),
        };
        for value in [
            entry.get("type"),
            entry.get("skip"),
            entry.get("cue"),
            entry.get("title"),
            text,
        ] {
            hash_value(&mut hasher, value);
        }
    }
    *hasher.finalize().as_bytes()
}

/// Strings by their bytes, without the escaping `to_string` would do; the
/// tag keeps a missing value, a string and anything else apart.
fn hash_value(hasher: &mut blake3::Hasher, value: Option<&Value>) {
    match value {
        None => hasher.update(b"\0n"),
        Some(Value::String(text)) => hasher
            .update(b"\0s")
            .update(&(text.len() as u64).to_le_bytes())
            .update(text.as_bytes()),
        Some(other) => hasher.update(b"\0v").update(other.to_string().as_bytes()),
    };
}

/// The cached script, kept with the project but never stored: a restore,
/// which replaces the project, starts without one. A mutex rather than a
/// cell, because the native host shares rooms across threads.
#[derive(Debug, Default)]
pub struct ScriptCache(Mutex<Option<([u8; 32], Arc<Script>)>>);

impl Clone for ScriptCache {
    fn clone(&self) -> Self {
        let held = self.0.lock().map(|cache| cache.clone()).unwrap_or_default();
        Self(Mutex::new(held))
    }
}

fn script_with(project: &Project, settings: &TeleprompterSettings) -> Script {
    match project.current() {
        Some(rundown) => Script::build(rundown, &project.custom_fields, settings),
        None => Script {
            revision: revision_of(&[], settings.chars_per_line),
            chars_per_line: settings.chars_per_line,
            events: Vec::new(),
        },
    }
}

/// The script as `GET /data/teleprompter/script?source=` serves it. Another
/// source is built on each read, and has revision 0, as nothing follows it.
pub fn script_for_source(
    project: &mut Project,
    source: Option<&str>,
    epoch_ms: i64,
) -> Arc<Script> {
    match source {
        Some(source) if source != project.teleprompter.script => {
            let settings = TeleprompterSettings {
                script: source.to_string(),
                ..project.teleprompter.clone()
            };
            Arc::new(Script {
                revision: 0,
                ..script_with(project, &settings)
            })
        }
        _ => {
            let script = script(project);
            mark_built(project, &script, epoch_ms);
            script
        }
    }
}

/// The first read of the script follows the event loaded before anyone read it.
fn mark_built(project: &mut Project, script: &Script, epoch_ms: i64) {
    if project.prompter.built {
        return;
    }
    project.prompter.built = true;
    // as upstream's first build: the reader is moved from the empty script to
    // this one, which puts a reader with no place yet at the top
    let empty = Layout::new(&[]);
    let layout = Layout::new(&script.events);
    let now = time_of_day(project, epoch_ms);
    project.prompter.transport = reanchor_transport(
        &project.prompter.transport,
        (&[], &empty),
        (&script.events, &layout),
        now,
    );
    go_to_loaded(project, script, epoch_ms);
}

/// Event by event, the reader goes to the start of the event Ontime loads,
/// and playback carries on from there.
fn go_to_loaded(project: &mut Project, script: &Script, epoch_ms: i64) -> bool {
    let transport = &project.prompter.transport;
    let Some(loaded) = project.runtime.selected_id.clone() else {
        return false;
    };
    if transport.mode != super::transport::Mode::Event
        || !script.events.iter().any(|event| event.id == loaded)
    {
        return false;
    }
    let layout = Layout::new(&script.events);
    let now = time_of_day(project, epoch_ms);
    match apply_command(
        transport,
        &TransportCommand::Goto { event_id: loaded },
        &layout,
        now,
    ) {
        Ok(next) => {
            project.prompter.transport = next;
            true
        }
        Err(_) => false,
    }
}

/// Called after Ontime loaded another event. `true` when the reader moved.
pub fn follow_loaded(project: &mut Project, epoch_ms: i64) -> bool {
    if !project.prompter.built {
        return false;
    }
    let script = script(project);
    go_to_loaded(project, &script, epoch_ms)
}

/// A command from a screen or an integration (websocket tag `teleprompter`).
pub fn command(
    project: &mut Project,
    payload: &Value,
    epoch_ms: i64,
) -> Result<State, CommandError> {
    let request = parse_command(payload)?;
    let script = script(project);
    mark_built(project, &script, epoch_ms);
    let command = match request {
        Command::Play => TransportCommand::Play,
        Command::Pause => TransportCommand::Pause,
        Command::Toggle => TransportCommand::Toggle,
        Command::Speed { value } => TransportCommand::Speed { value },
        Command::SpeedBy { value } => TransportCommand::SpeedBy { value },
        Command::Scroll { lines } => TransportCommand::Scroll { lines },
        Command::Next => TransportCommand::Next,
        Command::Previous => TransportCommand::Previous,
        Command::Top => TransportCommand::Top,
        Command::Mode { mode } => TransportCommand::Mode { mode },
        Command::Blackout { blackout } => TransportCommand::Blackout { blackout },
        Command::Goto { target } => TransportCommand::Goto {
            event_id: find_event(project, &script, &target)?,
        },
    };
    let layout = Layout::new(&script.events);
    let now = time_of_day(project, epoch_ms);
    project.prompter.transport =
        apply_command(&project.prompter.transport, &command, &layout, now)?;
    Ok(describe(&project.prompter.transport, &layout, now))
}

/// An event which is not in the script is passed on by id, for the transport to refuse.
fn find_event(
    project: &Project,
    script: &Script,
    target: &GotoTarget,
) -> Result<String, CommandError> {
    let found = match target {
        GotoTarget::Loaded(_) => project.runtime.selected_id.clone(),
        GotoTarget::Id { id } => Some(id.clone()),
        GotoTarget::Cue { cue } => script
            .events
            .iter()
            .find(|event| &event.cue == cue)
            .map(|event| event.id.clone()),
        GotoTarget::Index { index } => usize::try_from(*index)
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| script.events.get(index))
            .map(|event| event.id.clone()),
    };
    found.ok_or(CommandError::EventNotFound)
}

/// Keeps the reader in their place in the text after a change to what the
/// script is built from. `previous` is the script from before the change.
/// `true` when the transport moved.
pub fn after_change(project: &mut Project, previous: &Script, epoch_ms: i64) -> bool {
    if !project.prompter.built {
        return false;
    }
    let next = script(project);
    if next.events == previous.events {
        return false;
    }
    let now = time_of_day(project, epoch_ms);
    let (previous_layout, next_layout) = (Layout::new(&previous.events), Layout::new(&next.events));
    let moved = reanchor_transport(
        &project.prompter.transport,
        (&previous.events, &previous_layout),
        (&next.events, &next_layout),
        now,
    );
    let changed = moved != project.prompter.transport;
    project.prompter.transport = moved;
    changed
}

/// The script before a change, when anything follows it; `None` otherwise,
/// which saves building it for rooms nobody prompts from.
pub fn before_change(project: &Project) -> Option<Arc<Script>> {
    project.prompter.built.then(|| script(project))
}

/// Playback which reached its end stops there. `true` when it did.
pub fn tick(project: &mut Project, epoch_ms: i64) -> bool {
    if !project.prompter.built || project.prompter.transport.playback != Playback::Play {
        return false;
    }
    let script = script(project);
    let layout = Layout::new(&script.events);
    let settled = settle(
        &project.prompter.transport,
        &layout,
        time_of_day(project, epoch_ms),
    );
    let changed = settled != project.prompter.transport;
    project.prompter.transport = settled;
    changed
}

/// How long until playback reaches its end, for the host's alarm. `None` unless playing.
pub fn wake_in(project: &Project, epoch_ms: i64) -> Option<i64> {
    if !project.prompter.built || project.prompter.transport.playback != Playback::Play {
        return None;
    }
    let layout = Layout::new(&script(project).events);
    ms_until_settled(
        &project.prompter.transport,
        &layout,
        time_of_day(project, epoch_ms),
    )
    .map(|ms| ms.ceil() as i64)
}

/// A restored backup pauses shared playback: it never carries into another
/// project. Paused where playback had reached, as a pause command would.
pub fn pause_for_new_project(project: &mut Project, epoch_ms: i64) {
    if project.prompter.transport.playback != Playback::Play {
        return;
    }
    let layout = Layout::new(&script(project).events);
    let now = time_of_day(project, epoch_ms);
    if let Ok(paused) = apply_command(
        &project.prompter.transport,
        &TransportCommand::Pause,
        &layout,
        now,
    ) {
        project.prompter.transport = paused;
    }
}

/// The `teleprompter` and `teleprompterSync` keys of `runtime-data`.
pub fn runtime_fields(project: &Project, epoch_ms: i64) -> [(&'static str, Value); 2] {
    let now = time_of_day(project, epoch_ms);
    let (state, sync): (State, Sync) = match project.prompter.built {
        true => {
            let script = script(project);
            let layout = Layout::new(&script.events);
            (
                describe(&project.prompter.transport, &layout, now),
                describe_sync(&project.prompter.transport, &layout, script.revision),
            )
        }
        false => {
            let layout = Layout::new(&[]);
            (
                describe(&project.prompter.transport, &layout, now),
                describe_sync(&project.prompter.transport, &layout, 0),
            )
        }
    };
    [
        ("teleprompter", json!(state)),
        ("teleprompterSync", json!(sync)),
    ]
}
