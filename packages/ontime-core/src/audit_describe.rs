//! Turns a room request into an audit [`Note`], and decides its [`Actor`].
//!
//! Describing happens **before** the change is applied, while what a request
//! deletes or renames still has its old title. The host records the note
//! only if the change then succeeds and has an effect.

use serde_json::Value;

use crate::api::{self, route_segments, ApiRequest, HttpMethod};
use crate::audit::{Action, Actor, Note, Subject};
use crate::command::Command;
use crate::project::{token_matches, Project};
use crate::registry::Member;

/// Who a request counts as. A person's session comes first: when it is
/// present, it is what let them in.
pub fn actor(project: &Project, req: &ApiRequest, member: Option<&Member>) -> Actor {
    if let Some(member) = member {
        return Actor::Person {
            id: member.person,
            name: member.name.clone(),
        };
    }
    if api::is_admin(req) {
        return Actor::Admin;
    }
    let room = match (project.password_hash.as_deref(), req.token.as_deref()) {
        (Some(hash), Some(presented)) => token_matches(hash, presented),
        _ => false,
    };
    match room {
        true => Actor::RoomPassword,
        false => Actor::Anyone,
    }
}

/// What an HTTP request would change, or `None` for one that is not recorded:
/// reads, share links, downloads and anything the log leaves out.
pub fn http_note(project: &Project, req: &ApiRequest) -> Option<Note> {
    use HttpMethod::*;
    if req.method == Get {
        return None;
    }
    let body = &req.body;
    let segments = route_segments(&req.path);
    match (req.method, segments.as_slice()) {
        (_, ["data", "rundowns", rest @ ..]) => rundown_note(project, req.method, rest, body),
        (_, ["data", "project", ..]) => {
            Some(Note::new(Action::ProjectData).with_fields(keys(body)))
        }
        (_, ["data", "settings", ..]) => Some(Note::new(Action::Settings).with_fields(keys(body))),
        (_, ["data", "view-settings"]) => {
            Some(Note::new(Action::ViewSettings).with_fields(keys(body)))
        }
        (_, ["data", "custom-fields", rest @ ..]) => {
            let name = text(body, "label")
                .or(rest.first().copied())
                .map(Subject::titled);
            Some(Note::new(Action::CustomFields).about(name))
        }
        (_, ["data", "url-presets", ..]) => Some(Note::new(Action::UrlPresets)),
        (Post, ["data", "report", "archive"]) => Some(Note::new(Action::ReportArchive)),
        (Delete, ["data", "report", ..]) => Some(Note::new(Action::ReportClear)),
        (_, ["data", "members-only"]) => match body.get("on").and_then(Value::as_bool) {
            Some(true) => Some(Note::new(Action::MembersOnlyOn)),
            Some(false) => Some(Note::new(Action::MembersOnlyOff)),
            None => None,
        },
        (_, ["data", "session", "password"]) => match body.get("password") {
            Some(Value::String(password)) if !password.is_empty() => {
                Some(Note::new(Action::PasswordSet))
            }
            Some(Value::Null) => Some(Note::new(Action::PasswordCleared)),
            _ => None,
        },
        (_, ["data", "assets", "css", ..]) => Some(Note::new(Action::Css)),
        (Post, ["data", "db", "restore"]) => Some(Note::new(Action::Restore)),
        (Post, ["data", "db", "import"]) => Some(Note::new(Action::ProjectImport)),
        (Post, ["data", "public-feed"]) => Some(Note::new(Action::PublicFeed)),
        _ => None,
    }
}

fn rundown_note(
    project: &Project,
    method: HttpMethod,
    rest: &[&str],
    body: &Value,
) -> Option<Note> {
    use HttpMethod::*;
    let titled = |id: &str| {
        project
            .rundowns
            .get(id)
            .map(|rundown| Subject::titled(rundown.title.clone()))
    };
    let entry = |rundown: &str, id: &str| entry_subject(project, rundown, id);
    let entries = |rundown: &str, ids: &[String]| {
        let first = ids.first().and_then(|id| entry(rundown, id));
        (first, ids.len().saturating_sub(1))
    };

    let note = match (method, rest) {
        (Post, []) => {
            Note::new(Action::RundownCreate).about(text(body, "title").map(Subject::titled))
        }
        (Post, ["import"]) => {
            Note::new(Action::RundownImport).about(text(body, "title").map(Subject::titled))
        }
        (Patch, [_]) => {
            Note::new(Action::RundownRename).about(text(body, "title").map(Subject::titled))
        }
        (Put, [id]) => Note::new(Action::RundownReplace).about(titled(id)),
        (Delete, [id]) => Note::new(Action::RundownDelete).about(titled(id)),
        (Post, [id, "load"]) => Note::new(Action::RundownLoad).about(titled(id)),
        (Post, [id, "duplicate"]) => Note::new(Action::RundownDuplicate).about(titled(id)),
        (Post, [_, "entry"]) => Note::new(Action::EntryAdd).about(Some(subject_of(body))),
        (Put, [id, "entry"]) => {
            let subject = text(body, "id").and_then(|entry_id| entry(id, entry_id));
            Note::new(Action::EntryEdit)
                .about(subject)
                .with_fields(field_names(body))
        }
        (Put, [id, "batch"]) => {
            let (subject, more) = entries(id, &strings(body.get("ids")));
            let data = body.get("data").unwrap_or(&Value::Null);
            Note::new(Action::EntryEdit)
                .about(subject)
                .with_more(more)
                .with_fields(field_names(data))
        }
        (Delete, [id, "entries"]) => {
            let (subject, more) = entries(id, &strings(body.get("ids")));
            Note::new(Action::EntryDelete)
                .about(subject)
                .with_more(more)
        }
        (Delete, [id, "all"]) => Note::new(Action::EntryDeleteAll).about(titled(id)),
        (Patch, [id, "reorder"]) => Note::new(Action::EntryMove)
            .about(text(body, "entryId").and_then(|entry_id| entry(id, entry_id))),
        (Patch, [id, "swap"]) => Note::new(Action::EntrySwap)
            .about(text(body, "from").and_then(|entry_id| entry(id, entry_id))),
        (Patch, [id, "renumber"]) => {
            let (subject, more) = entries(id, &strings(body.get("ids")));
            Note::new(Action::EntryRenumber)
                .about(subject)
                .with_more(more)
        }
        (Patch, [id, "applydelay", delay_id]) => {
            Note::new(Action::DelayApply).about(entry(id, delay_id))
        }
        (Post, [id, "clone", entry_id]) => Note::new(Action::EntryClone).about(entry(id, entry_id)),
        (Post, [id, "group"]) => {
            let (subject, more) = entries(id, &strings(body.get("ids")));
            Note::new(Action::EntryGroup).about(subject).with_more(more)
        }
        (Post, [id, "ungroup", group_id]) => {
            Note::new(Action::EntryUngroup).about(entry(id, group_id))
        }
        _ => return None,
    };
    Some(note)
}

/// What a playback command did, described **after** it was applied, so that
/// "start" names the event that actually started. `before` is the event that
/// was loaded until then, which pause, stop and add-time are about.
///
/// An integration `change` is recorded as an entry edit. The message overlay,
/// the aux timers and the offset mode are not recorded.
pub fn socket_note(
    project: &Project,
    tag: &str,
    payload: Option<&Value>,
    before: Option<&str>,
) -> Option<Note> {
    if tag == "teleprompter" {
        return prompter_note(project, payload?);
    }
    let command = match crate::integration::parse(tag, payload).ok()? {
        crate::integration::Action::Playback { command, .. } => command,
        // an entry edited from outside reads as the same edit made in the editor
        crate::integration::Action::Change { id, fields } => {
            let names = fields
                .iter()
                .map(|(name, _)| match name.strip_prefix("custom:") {
                    Some(custom) => format!("custom.{custom}"),
                    None => name.clone(),
                })
                .collect();
            return Some(
                Note::new(Action::EntryEdit)
                    .about(entry_subject(project, &project.loaded, &id))
                    .with_fields(names),
            );
        }
        _ => return None,
    };
    let loaded = project.runtime.selected_id.as_deref();
    let (action, about, amount) = match command {
        Command::Start
        | Command::StartNext
        | Command::StartPrevious
        | Command::StartId(_)
        | Command::StartIndex(_)
        | Command::StartCue(_) => (Action::Start, loaded, None),
        Command::LoadNext
        | Command::LoadPrevious
        | Command::LoadId(_)
        | Command::LoadIndex(_)
        | Command::LoadCue(_) => (Action::Load, loaded, None),
        Command::Roll => (Action::Roll, loaded, None),
        Command::Pause => (Action::Pause, before, None),
        Command::Stop => (Action::Stop, before, None),
        Command::Reload => (Action::Reload, before, None),
        Command::AddTime(ms) => (Action::AddTime, before, Some(ms)),
        // a display preference of the room, like the message overlay
        Command::SetOffsetMode(_) => return None,
    };
    let subject = about.and_then(|id| entry_subject(project, &project.loaded, id));
    let note = Note::new(action).about(subject);
    Some(match amount {
        Some(ms) => note.with_amount(ms),
        None => note,
    })
}

/// A teleprompter command, described after it was applied. Its subject is the
/// event the reader is in, which every command re-anchors to as it runs.
fn prompter_note(project: &Project, payload: &Value) -> Option<Note> {
    use crate::teleprompter::transport::{parse_command, Command as Prompt, Playback};
    let transport = &project.prompter.transport;
    let action = match parse_command(payload).ok()? {
        Prompt::Play => Action::PrompterPlay,
        Prompt::Pause => Action::PrompterPause,
        Prompt::Toggle => match transport.playback {
            Playback::Play => Action::PrompterPlay,
            Playback::Pause => Action::PrompterPause,
        },
        Prompt::Speed { .. } | Prompt::SpeedBy { .. } => Action::PrompterSpeed,
        Prompt::Scroll { .. }
        | Prompt::Next
        | Prompt::Previous
        | Prompt::Top
        | Prompt::Goto { .. } => Action::PrompterMove,
        Prompt::Mode { .. } => Action::PrompterMode,
        Prompt::Blackout { .. } => Action::PrompterBlackout,
    };
    let subject = transport
        .anchor
        .as_ref()
        .and_then(|anchor| entry_subject(project, &project.loaded, &anchor.event_id));
    Some(Note::new(action).about(subject))
}

fn entry_subject(project: &Project, rundown: &str, id: &str) -> Option<Subject> {
    project
        .rundowns
        .get(rundown)?
        .entries
        .get(id)
        .map(subject_of)
}

fn subject_of(entry: &Value) -> Subject {
    Subject {
        cue: text(entry, "cue").unwrap_or_default().to_string(),
        title: text(entry, "title").unwrap_or_default().to_string(),
    }
}

/// The names of the fields an entry patch sets, never their values. Custom
/// fields are named `custom.<key>`. `id` only says which entry, so it is left out.
fn field_names(patch: &Value) -> Vec<String> {
    let Some(object) = patch.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .filter(|(key, _)| key.as_str() != "id")
        .flat_map(|(key, value)| match (key.as_str(), value.as_object()) {
            ("custom", Some(custom)) => {
                custom.keys().map(|name| format!("custom.{name}")).collect()
            }
            _ => vec![key.clone()],
        })
        .collect()
}

fn keys(body: &Value) -> Vec<String> {
    body.as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
