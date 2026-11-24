//! Socket commands, the alarm, when to wake and the `runtime-data` payload.

use serde_json::{json, Value};

use crate::project::Project;
use crate::teleprompter::room as prompter;

/// How long until the host next has to do something, in milliseconds.
///
/// `None` means nothing is moving and the host may sleep indefinitely — on the
/// Worker that is the difference between an idle room costing nothing and costing
/// a request a second, and on a native server it is the difference between an
/// idle process being quiet and being busy.
///
/// `has_clients` says whether anyone is connected to hear a broadcast. With
/// nobody there the heartbeat is pointless, and a room whose timer was left
/// running would otherwise wake for ever at somebody's expense. The end-of-event
/// wake is kept regardless, because that one advances the rundown and there is
/// one of it per event rather than one a second.
///
/// This lives here rather than in either host because it is policy, not
/// transport: the two answering it differently is exactly how they would drift.
pub fn next_wake_in(project: &Project, now: i64, has_clients: bool) -> Option<i64> {
    // the teleprompter wakes once, when playback reaches its end, never per tick
    let prompter = prompter::wake_in(project, now).map(|ms| ms.max(50));
    match (timer_wake_in(project, now, has_clients), prompter) {
        (Some(timer), Some(prompter)) => Some(timer.min(prompter)),
        (timer, prompter) => timer.or(prompter),
    }
}

fn timer_wake_in(project: &Project, now: i64, has_clients: bool) -> Option<i64> {
    let runtime = &project.runtime;
    let heartbeat = if has_clients {
        project.heartbeat_ms.max(0)
    } else {
        0
    };

    if !runtime.is_running() {
        // A paused show is still losing time against the schedule and the
        // operator is watching that figure grow, so the tick continues. An aux
        // timer runs independently of the rundown too: without this a stopwatch
        // started while playback is stopped would never update.
        let needs_tick =
            runtime.playback == crate::runtime::Playback::Pause || project.aux_timers.any_running();

        return (needs_tick && heartbeat > 0).then_some(heartbeat);
    }

    let rundown = project.current()?;
    let event = runtime.selected_id.as_ref().and_then(|id| rundown.get(id));
    let inputs = runtime.inputs_for(event, project.timezone_offset);

    // In roll mode the host is the only thing that will start the event, so it
    // wakes at the exact moment rather than waiting for a heartbeat.
    if let Some(target) = runtime.secondary_target {
        let clock = (now + project.timezone_offset).rem_euclid(crate::rundown::MS_PER_DAY);
        let until_due = (target - clock).max(0);
        let delay = if heartbeat > 0 {
            until_due.min(heartbeat)
        } else {
            until_due
        };
        return Some(delay.max(50));
    }

    // roll hands over to the next event on its own, so it needs the wake even
    // without an end action
    let has_end_action = runtime.playback == crate::runtime::Playback::Roll
        || event
            .and_then(|e| e.get("endAction"))
            .and_then(Value::as_str)
            .is_some_and(|action| action != "none");

    let until_end = match (has_end_action, inputs.ends_at(now)) {
        (true, Some(ends_at)) => Some((ends_at - now).max(0)),
        _ => None,
    };

    // a heartbeat of zero disables the safety net entirely, leaving only the
    // end-of-event wake — the cheapest a host can run
    let delay = match (until_end, heartbeat > 0) {
        (Some(until_end), true) => until_end.min(heartbeat),
        (Some(until_end), false) => until_end,
        (None, true) => heartbeat,
        (None, false) => return None,
    };

    // a floor stops a rundown of already-expired events spinning the wake
    Some(delay.max(50))
}

/// Applies a websocket command to the project.
///
/// Playback, the overlay, the aux timers and the run report all move through
/// here, so a host cannot implement three of the four and quietly lose the
/// fourth — which is how the report went missing from the native server.
///
/// Returns true when clients should be told and the project written down.
pub fn apply_socket_command(
    project: &mut Project,
    tag: &str,
    payload: Option<&Value>,
    now: i64,
) -> bool {
    match tag {
        "teleprompter" => {
            // a refused command changes nothing; the screen keeps what it shows
            return payload.is_some_and(|payload| prompter::command(project, payload, now).is_ok());
        }
        "message" => return payload.map(|p| project.message.apply(p)).unwrap_or(false),
        "auxtimer" => {
            return payload
                .map(|p| project.aux_timers.apply(p, now))
                .unwrap_or(false)
        }
        _ => {}
    }

    let Some(command) = crate::runtime::Command::parse(tag, payload) else {
        return false;
    };
    let Some(rundown) = project.current().cloned() else {
        return false;
    };

    let was_playing = project.runtime.playback == crate::runtime::Playback::Play;
    let previous_entry = project.runtime.selected_id.clone();
    let tz = project.timezone_offset;

    if !project.runtime.apply(command, &rundown, now, tz) {
        return false;
    }

    // event by event, the teleprompter follows the event Ontime loads
    if project.runtime.selected_id != previous_entry {
        prompter::follow_loaded(project, now);
    }
    record_report(project, was_playing, previous_entry, now);
    true
}

/// The alarm: the timer's end action and the teleprompter reaching its end.
/// `true` when anything changed that has to be stored.
pub fn on_alarm(project: &mut Project, now: i64) -> bool {
    let previous_entry = project.runtime.selected_id.clone();
    let tz = project.timezone_offset;
    let timer = match project.current().cloned() {
        Some(rundown) => project.runtime.on_tick(&rundown, now, tz),
        None => false,
    };
    // an end action may load the next event
    let followed = project.runtime.selected_id != previous_entry && prompter::follow_loaded(project, now);
    let settled = prompter::tick(project, now);
    timer || followed || settled
}

/// The whole `runtime-data` payload: playback, the aux timers, the overlay and
/// the teleprompter all live in the same client store. One place, for every
/// host, because the copies drifting apart is how the aux timers went missing.
pub fn runtime_payload(project: &Project, now: i64) -> Option<Value> {
    let rundown = project.current()?;
    let mut payload = project.runtime.snapshot(rundown, now, project.timezone_offset);
    if let Some(object) = payload.as_object_mut() {
        if let Some(aux) = project.aux_timers.snapshot(now, &project.settings.aux_timer_names).as_object() {
            object.extend(aux.clone());
        }
        object.insert("message".into(), json!(project.message));
        object.extend(prompter::runtime_fields(project, now).map(|(key, value)| (key.to_string(), value)));
    }
    Some(payload)
}

/// Notes when an event actually started or stopped.
///
/// The report is what lets an operator compare the run against the plan
/// afterwards, so it records wall-clock time of day rather than epoch.
fn record_report(
    project: &mut Project,
    was_playing: bool,
    previous_entry: Option<String>,
    now: i64,
) {
    let clock = (now + project.timezone_offset).rem_euclid(crate::rundown::MS_PER_DAY);
    let is_playing = project.runtime.playback == crate::runtime::Playback::Play;
    let current_entry = project.runtime.selected_id.clone();

    // whatever was running has stopped if playback ended or moved on
    if was_playing {
        if let Some(previous) = previous_entry.as_deref() {
            if !is_playing || current_entry.as_deref() != Some(previous) {
                project.report.on_stop(previous, clock);
            }
        }
    }

    // and a new one has begun if playback just started, or moved to another entry
    if is_playing {
        if let Some(current) = current_entry.as_deref() {
            if !was_playing || previous_entry.as_deref() != Some(current) {
                project.report.on_start(current, clock);
            }
        }
    }
}
