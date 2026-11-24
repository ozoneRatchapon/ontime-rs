//! The registry's side of the audit log: people events, for the admin only.
//!
//! A request is described from its route and from the directory **before** it
//! ran, so a removed person is still named. The host records the line only if
//! the request succeeded and changed something.

use serde_json::Value;
use uuid::Uuid;

use crate::audit::{Action, Actor, Note, Subject};
use crate::directory::Directory;

/// Who made a registry request, and what it does. `None` for reads, for the
/// rooms' own reports and checks, and for anything not recorded.
///
/// `people/*` and `rooms/*` only reach the registry through the admin route,
/// which has already checked the master password.
pub fn note(
    before: &Directory,
    method: &str,
    segments: &[&str],
    body: &Value,
    now: i64,
) -> Option<(Actor, Note)> {
    let name_of = |id: &str| -> Option<Subject> {
        let id = Uuid::parse_str(id).ok()?;
        before
            .identity
            .person(&id)
            .map(|person| Subject::titled(person.name.clone()))
    };
    let caller = || {
        let token = body.get("token").and_then(Value::as_str)?;
        let id = before.identity.person_of(token, now)?;
        let name = before
            .identity
            .person(&id)
            .map(|person| person.name.clone())
            .unwrap_or_default();
        Some(Actor::Person { id, name })
    };
    let named = || {
        body.get("name")
            .and_then(Value::as_str)
            .map(|name| Subject::titled(name.trim()))
    };

    let admin = |note: Note| Some((Actor::Admin, note));
    match (method, segments) {
        ("POST", ["people"]) => admin(Note::new(Action::PersonCreate).about(named())),
        ("POST", ["people", id, "invite"]) => {
            admin(Note::new(Action::InviteIssue).about(name_of(id)))
        }
        ("DELETE", ["people", id, "sessions"]) => {
            admin(Note::new(Action::SignOut).about(name_of(id)))
        }
        ("DELETE", ["people", id]) => admin(Note::new(Action::PersonRemove).about(name_of(id))),
        ("PUT", ["rooms", room]) => {
            let fields: Vec<String> = ["owners", "operators"]
                .into_iter()
                .filter(|key| body.get(key).is_some())
                .map(str::to_string)
                .collect();
            match fields.is_empty() {
                true => None,
                false => admin(
                    Note::new(Action::MembersChange)
                        .with_fields(fields)
                        .in_room(room),
                ),
            }
        }
        // the person is only known once the code is spent: see [`redeemed`]
        ("POST", ["auth", "logout"]) => caller().map(|actor| (actor, Note::new(Action::SignOut))),
        ("POST", ["auth", "rooms", room, "operators"]) => caller().map(|actor| {
            (
                actor,
                Note::new(Action::OperatorAdd).about(named()).in_room(room),
            )
        }),
        ("POST", ["auth", "rooms", room, "operators", id, "invite"]) => caller().map(|actor| {
            (
                actor,
                Note::new(Action::InviteIssue)
                    .about(name_of(id))
                    .in_room(room),
            )
        }),
        ("DELETE", ["auth", "rooms", room, "operators", id]) => caller().map(|actor| {
            (
                actor,
                Note::new(Action::OperatorRemove)
                    .about(name_of(id))
                    .in_room(room),
            )
        }),
        _ => None,
    }
}

/// An invite being spent, described from the registry's answer: the person
/// it signed in is the actor.
pub fn redeemed(method: &str, segments: &[&str], response: &Value) -> Option<(Actor, Note)> {
    if (method, segments) != ("POST", ["auth", "redeem"].as_slice()) {
        return None;
    }
    let person = response.get("person")?;
    let id = person
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())?;
    let name = person
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some((Actor::Person { id, name }, Note::new(Action::InviteRedeem)))
}
