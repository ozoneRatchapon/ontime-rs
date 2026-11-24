//! The room's teleprompter: what a host calls, so no host holds any of its
//! rules. Ported from upstream's `teleprompter.service.ts`, with one change
//! of shape: upstream keeps the transport in memory, and a Durable Object is
//! evicted between messages, so here it is stored with the project
//! ([`Prompter`]) and the script is rebuilt from the project when needed.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::position::Layout;
use super::script::{revision_of, Script};
use super::settings::TeleprompterSettings;
use super::transport::{
    apply_command, describe, describe_sync, ms_until_settled, parse_command, reanchor_transport, settle, Command,
    CommandError, GotoTarget, Playback, State, Sync, Transport, TransportCommand,
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
pub fn script(project: &Project) -> Script {
    script_with(project, &project.teleprompter)
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
pub fn script_for_source(project: &mut Project, source: Option<&str>, epoch_ms: i64) -> Script {
    match source {
        Some(source) if source != project.teleprompter.script => {
            let settings = TeleprompterSettings { script: source.to_string(), ..project.teleprompter.clone() };
            Script { revision: 0, ..script_with(project, &settings) }
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
    go_to_loaded(project, script, epoch_ms);
}

/// Event by event, the reader goes to the start of the event Ontime loads,
/// and playback carries on from there.
fn go_to_loaded(project: &mut Project, script: &Script, epoch_ms: i64) -> bool {
    let transport = &project.prompter.transport;
    let Some(loaded) = project.runtime.selected_id.clone() else {
        return false;
    };
    if transport.mode != super::transport::Mode::Event || !script.events.iter().any(|event| event.id == loaded) {
        return false;
    }
    let layout = Layout::new(&script.events);
    let now = time_of_day(project, epoch_ms);
    match apply_command(transport, &TransportCommand::Goto { event_id: loaded }, &layout, now) {
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
pub fn command(project: &mut Project, payload: &Value, epoch_ms: i64) -> Result<State, CommandError> {
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
        Command::Goto { target } => TransportCommand::Goto { event_id: find_event(project, &script, &target)? },
    };
    let layout = Layout::new(&script.events);
    let now = time_of_day(project, epoch_ms);
    project.prompter.transport = apply_command(&project.prompter.transport, &command, &layout, now)?;
    Ok(describe(&project.prompter.transport, &layout, now))
}

/// An event which is not in the script is passed on by id, for the transport to refuse.
fn find_event(project: &Project, script: &Script, target: &GotoTarget) -> Result<String, CommandError> {
    let found = match target {
        GotoTarget::Loaded(_) => project.runtime.selected_id.clone(),
        GotoTarget::Id { id } => Some(id.clone()),
        GotoTarget::Cue { cue } => script.events.iter().find(|event| &event.cue == cue).map(|event| event.id.clone()),
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
pub fn before_change(project: &Project) -> Option<Script> {
    project.prompter.built.then(|| script(project))
}

/// Playback which reached its end stops there. `true` when it did.
pub fn tick(project: &mut Project, epoch_ms: i64) -> bool {
    if !project.prompter.built || project.prompter.transport.playback != Playback::Play {
        return false;
    }
    let script = script(project);
    let layout = Layout::new(&script.events);
    let settled = settle(&project.prompter.transport, &layout, time_of_day(project, epoch_ms));
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
    ms_until_settled(&project.prompter.transport, &layout, time_of_day(project, epoch_ms)).map(|ms| ms.ceil() as i64)
}

/// A restored backup pauses shared playback: it never carries into another
/// project. Paused where playback had reached, as a pause command would.
pub fn pause_for_new_project(project: &mut Project, epoch_ms: i64) {
    if project.prompter.transport.playback != Playback::Play {
        return;
    }
    let layout = Layout::new(&script(project).events);
    let now = time_of_day(project, epoch_ms);
    if let Ok(paused) = apply_command(&project.prompter.transport, &TransportCommand::Pause, &layout, now) {
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
            (describe(&project.prompter.transport, &layout, now), describe_sync(&project.prompter.transport, &layout, script.revision))
        }
        false => {
            let layout = Layout::new(&[]);
            (describe(&project.prompter.transport, &layout, now), describe_sync(&project.prompter.transport, &layout, 0))
        }
    };
    [("teleprompter", json!(state)), ("teleprompterSync", json!(sync))]
}
