use crate::types::{EventItem, PlaybackState, RundownState, WsAction};
use chrono::Utc;

pub struct RundownEngine;

impl RundownEngine {
    pub fn apply_action(state: &mut RundownState, action: WsAction) {
        match action {
            WsAction::SyncState(new_state) => {
                *state = new_state;
            }
            WsAction::StartTimer => match state.playback_state {
                PlaybackState::Running => return,
                _ => {
                    state.playback_state = PlaybackState::Running;
                    state.started_at_utc = Some(Utc::now().timestamp_millis() as u64);
                }
            },
            WsAction::PauseTimer => match state.playback_state {
                PlaybackState::Running => {
                    state.playback_state = PlaybackState::Paused;
                }
                _ => return,
            },
            WsAction::ResetTimer => {
                state.playback_state = PlaybackState::Stopped;
                state.elapsed_millis = 0;
                state.started_at_utc = None;
            }
            WsAction::NextEvent => {
                if state.current_event_index + 1 < state.events.len() {
                    state.current_event_index += 1;
                    state.elapsed_millis = 0;
                }
            }
            WsAction::PreviousEvent => {
                if state.current_event_index > 0 {
                    state.current_event_index -= 1;
                    state.elapsed_millis = 0;
                }
            }
            WsAction::AddEvent {
                cue,
                title,
                duration,
            } => {
                let event = EventItem::new(cue, title, duration);
                state.events.push(event);
            }
            WsAction::EditEvent {
                id,
                cue,
                title,
                duration,
                subtitle,
                presenter,
                note,
            } => {
                let Some(event) = state.events.iter_mut().find(|e| e.id == id) else {
                    return;
                };
                if let Some(c) = cue {
                    event.cue = c;
                }
                if let Some(t) = title {
                    event.title = t;
                }
                if let Some(d) = duration {
                    event.duration = d * 1000;
                }
                if subtitle.is_some() {
                    event.subtitle = subtitle;
                }
                if presenter.is_some() {
                    event.presenter = presenter;
                }
                if note.is_some() {
                    event.note = note;
                }
            }
            WsAction::DeleteEvent { id } => {
                state.events.retain(|e| e.id != id);
                if state.current_event_index >= state.events.len() && !state.events.is_empty() {
                    state.current_event_index = state.events.len() - 1;
                }
            }
            WsAction::ReorderEvents {
                from_index,
                to_index,
            } => {
                if from_index >= state.events.len() || to_index >= state.events.len() {
                    return;
                }
                let item = state.events.remove(from_index);
                state.events.insert(to_index, item);
            }
            WsAction::UpdateProjectTitle { title } => {
                state.project.title = title;
            }
        }

        state.updated_at = Utc::now().timestamp_millis() as u64;
        state.checksum = state.compute_checksum();
    }
}
