//! Carries out a parsed [`Action`] on the project (upstream dispatchFromAdapter's
//! handlers), and says what changed for the host to act on.

use serde_json::{json, Value};

use super::action::Action;
use super::change;
use super::types::{Change, IntegrationError, Reply};
use crate::api::{record_report, runtime_payload};
use crate::command::Command;
use crate::project::{Project, ONTIME_VERSION};
use crate::runtime::Playback;
use crate::teleprompter::room as prompter;

pub fn dispatch(
    project: &mut Project,
    action: Action,
    now: i64,
) -> Result<Reply, IntegrationError> {
    match action {
        Action::Version => Ok(Reply::new(json!(ONTIME_VERSION), Change::None)),
        Action::Poll => Ok(Reply::new(
            runtime_payload(project, now).unwrap_or(Value::Null),
            Change::None,
        )),
        Action::Change { id, fields } => change::apply(project, &id, &fields),
        Action::Message(patch) => {
            let changed = project.message.patch(patch);
            Ok(Reply::new(json!(project.message), runtime_change(changed)))
        }
        Action::Playback { command, failure } => playback(project, command, failure, now),
        Action::AuxTimer { index, command } => {
            let name = project.settings.aux_timer_names[index - 1].clone();
            let state = project
                .aux_timers
                .run(index, command, now, &name)
                .ok_or_else(|| IntegrationError::new("Invalid auxtimer index"))?;
            Ok(Reply::new(state, Change::Runtime))
        }
        // the sockets are the host's; it answers for a target it does not know
        Action::Client(command) => Ok(Reply::success(Change::Client(command))),
        Action::Teleprompter(command) => prompter::run(project, command, now)
            .map(|state| Reply::new(json!(state), Change::Runtime))
            .map_err(|error| IntegrationError::new(error.message())),
        Action::Nothing => Ok(Reply::success(Change::None)),
    }
}

fn runtime_change(changed: bool) -> Change {
    match changed {
        true => Change::Runtime,
        false => Change::None,
    }
}

/// A playback command, with the teleprompter following the loaded event and
/// the run report noting what started and stopped.
fn playback(
    project: &mut Project,
    command: Command,
    failure: Option<String>,
    now: i64,
) -> Result<Reply, IntegrationError> {
    let refused = |failure: Option<String>| match failure {
        Some(message) => Err(IntegrationError::new(message)),
        None => Ok(Reply::success(Change::None)),
    };
    let Some(rundown) = project.current().cloned() else {
        return refused(failure);
    };

    let was_playing = project.runtime.playback == Playback::Play;
    let previous_entry = project.runtime.selected_id.clone();
    let tz = project.timezone_offset;

    if !project.runtime.apply(command, &rundown, now, tz) {
        return refused(failure);
    }

    // event by event, the teleprompter follows the event Ontime loads
    if project.runtime.selected_id != previous_entry {
        prompter::follow_loaded(project, now);
    }
    record_report(project, was_playing, previous_entry, now);
    Ok(Reply::success(Change::Runtime))
}
