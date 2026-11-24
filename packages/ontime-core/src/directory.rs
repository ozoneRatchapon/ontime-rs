//! The registry instance's whole surface: the room list plus the people who
//! own and run rooms. Routes that touch only rooms go to [`registry::handle`];
//! the rest need both documents, because people belong to rooms.
//!
//! Who may call what is the host's job: `people` and `rooms` sit behind the
//! master token, `auth` is public but every route needs a code or a session,
//! `auth/check` and `report` are internal (asked by rooms, never routed from
//! outside).

use serde_json::{json, Value};
use uuid::Uuid;

use crate::identity::{Identity, IdentityError, PersonSummary, Secrets};
use crate::registry::{self, Member, Registry, RegistryResponse, Role};

/// Both documents the registry instance stores.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Directory {
    pub rooms: Registry,
    pub identity: Identity,
}

fn text<'a>(body: &'a Value, key: &str) -> &'a str {
    body.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn person_id(raw: &str) -> Option<Uuid> {
    Uuid::parse_str(raw).ok()
}

const SIGNED_OUT: &str = "This device is not signed in";
const NOT_OWNER: &str = "Only an owner of this room can do that";

fn identity_error(error: IdentityError) -> RegistryResponse {
    match error {
        IdentityError::UnknownPerson => RegistryResponse::error(404, "no such person"),
        IdentityError::InvalidName => RegistryResponse::error(400, "a person needs a name"),
        IdentityError::InvalidInvite => {
            RegistryResponse::error(401, "This invite link is not valid any more. Ask for a new one.")
        }
    }
}

/// Routes a request below the registry root. `segments` is the path split on `/`.
///
/// Admin (master token):
/// - `GET    people`                  everyone, with devices and pending invites
/// - `POST   people`                  `{ name }`
/// - `POST   people/<id>/invite`      a new invite code, replacing any unused one
/// - `DELETE people/<id>/sessions`    sign that person out everywhere
/// - `DELETE people/<id>`             remove them, and from every room
/// - `PUT    rooms/<name>`            also `{ owners?: [id], operators?: [id] }`
///
/// A person (`token` in the body is their session):
/// - `POST   auth/redeem`             `{ code, device }` -> `{ token, person }`
/// - `POST   auth/me`                 -> `{ person, rooms }`, each room with its `role`
/// - `POST   auth/logout`
/// - `GET    auth/rooms/<room>/team`                     owner only
/// - `POST   auth/rooms/<room>/operators`                owner only, `{ name }`
/// - `POST   auth/rooms/<room>/operators/<id>/invite`    owner only, people they invited
/// - `DELETE auth/rooms/<room>/operators/<id>`           owner only
///
/// Internal:
/// - `POST   auth/check`              `{ token, room }` -> `{ person, role }`, asked by a room
///
/// Anything else is a room-list route, see [`registry::handle`].
pub fn handle(
    directory: &mut Directory,
    method: &str,
    segments: &[&str],
    body: &Value,
    now: i64,
    secrets: &mut impl Secrets,
) -> RegistryResponse {
    let Directory { rooms, identity } = directory;
    // expired links and sessions go whenever something is written anyway
    let pruned = method != "GET" && identity.prune(now);
    let mut response = match segments {
        ["auth", "rooms", room, rest @ ..] => team(rooms, identity, method, room, rest, body, now, secrets),
        _ => route(rooms, identity, method, segments, body, now, secrets),
    };
    response.changed |= pruned;
    response
}

/// Parses a list of people ids, all of which must be real people.
fn people_list(identity: &Identity, value: &Value) -> Result<Vec<Uuid>, RegistryResponse> {
    let Some(listed) = value.as_array() else {
        return Err(RegistryResponse::error(400, "owners and operators have to be lists of people"));
    };
    let mut people = Vec::new();
    for raw in listed {
        match raw.as_str().and_then(person_id).filter(|id| identity.person(id).is_some()) {
            Some(id) if !people.contains(&id) => people.push(id),
            Some(_) => {}
            None => return Err(RegistryResponse::error(400, "the list names someone who is not a person")),
        }
    }
    Ok(people)
}

#[allow(clippy::too_many_arguments)]
fn route(
    rooms: &mut Registry,
    identity: &mut Identity,
    method: &str,
    segments: &[&str],
    body: &Value,
    now: i64,
    secrets: &mut impl Secrets,
) -> RegistryResponse {
    match (method, segments) {
        ("GET", ["people"]) => RegistryResponse::ok(json!({ "people": identity.summaries() }), false),

        ("POST", ["people"]) => match identity.create_person(text(body, "name"), now) {
            Ok(person) => RegistryResponse::ok(json!(person), true),
            Err(error) => identity_error(error),
        },

        ("POST", ["people", id, "invite"]) => {
            let Some(id) = person_id(id) else {
                return identity_error(IdentityError::UnknownPerson);
            };
            invite(identity, id, now, secrets)
        }

        ("DELETE", ["people", id, "sessions"]) => match person_id(id).filter(|id| identity.person(id).is_some()) {
            Some(id) => {
                let changed = identity.revoke(&id);
                RegistryResponse::ok(json!({ "signedOut": id }), changed)
            }
            None => identity_error(IdentityError::UnknownPerson),
        },

        ("DELETE", ["people", id]) => match person_id(id).filter(|id| identity.remove_person(id)) {
            Some(id) => {
                rooms.drop_person(&id);
                RegistryResponse::ok(json!({ "removed": id }), true)
            }
            None => identity_error(IdentityError::UnknownPerson),
        },

        ("PUT", ["rooms", name]) if body.get("owners").is_some() || body.get("operators").is_some() => {
            for (key, role) in [("owners", Role::Owner), ("operators", Role::Operator)] {
                let Some(value) = body.get(key) else {
                    continue;
                };
                let people = match people_list(identity, value) {
                    Ok(people) => people,
                    Err(refused) => return refused,
                };
                if rooms.set_members(name, role, people, now).is_none() {
                    return RegistryResponse::error(400, "not a valid room name");
                }
            }
            // owner and note may travel in the same request
            registry::handle(rooms, method, segments, body, now)
        }

        ("POST", ["auth", "redeem"]) => {
            let invites = identity.invites.len();
            match identity.redeem(text(body, "code"), text(body, "device"), now, secrets) {
                Ok((token, person)) => RegistryResponse::ok(json!({ "token": token, "person": person }), true),
                // an expired invite is spent by the attempt; a made-up one changes nothing
                Err(error) => RegistryResponse {
                    changed: identity.invites.len() != invites,
                    ..identity_error(error)
                },
            }
        }

        ("POST", ["auth", "me"]) => match identity.authenticate(text(body, "token"), now) {
            Some((id, touched)) => RegistryResponse::ok(
                json!({ "person": identity.person(&id), "rooms": rooms.rooms_of(&id) }),
                touched,
            ),
            None => RegistryResponse::error(401, SIGNED_OUT),
        },

        ("POST", ["auth", "check"]) => match identity.authenticate(text(body, "token"), now) {
            Some((id, touched)) => match rooms.role_in(text(body, "room"), &id) {
                Some(role) => {
                    let name = identity.person(&id).map(|person| person.name.clone()).unwrap_or_default();
                    let member = Member { person: id, name, role };
                    RegistryResponse::ok(json!(member), touched)
                }
                None => RegistryResponse {
                    changed: touched,
                    ..RegistryResponse::error(403, "not a member of this room")
                },
            },
            None => RegistryResponse::error(401, SIGNED_OUT),
        },

        ("POST", ["auth", "logout"]) => match identity.sign_out(text(body, "token"), now) {
            true => RegistryResponse::ok(json!({}), true),
            false => RegistryResponse::error(401, SIGNED_OUT),
        },

        _ => registry::handle(rooms, method, segments, body, now),
    }
}

fn invite(identity: &mut Identity, id: Uuid, now: i64, secrets: &mut impl Secrets) -> RegistryResponse {
    match identity.issue_invite(id, now, secrets) {
        Ok((code, expires_at)) => RegistryResponse::ok(json!({ "code": code, "expiresAt": expires_at }), true),
        Err(error) => identity_error(error),
    }
}

/// An owner managing the operators of one of their rooms.
#[allow(clippy::too_many_arguments)]
fn team(
    rooms: &mut Registry,
    identity: &mut Identity,
    method: &str,
    room: &str,
    rest: &[&str],
    body: &Value,
    now: i64,
    secrets: &mut impl Secrets,
) -> RegistryResponse {
    let Some((owner, touched)) = identity.authenticate(text(body, "token"), now) else {
        return RegistryResponse::error(401, SIGNED_OUT);
    };
    if rooms.role_in(room, &owner) != Some(Role::Owner) {
        return RegistryResponse {
            changed: touched,
            ..RegistryResponse::error(403, NOT_OWNER)
        };
    }

    let mut response = match (method, rest) {
        ("GET", ["team"]) => {
            let record = &rooms.rooms[room];
            let summary = |ids: &[Uuid]| -> Vec<Value> {
                let summaries = identity.summaries();
                ids.iter()
                    .filter_map(|id| summaries.iter().find(|s: &&PersonSummary| &s.person.id == id))
                    .map(|s| json!({ "person": s, "mine": s.person.invited_by == Some(owner) }))
                    .collect()
            };
            RegistryResponse::ok(
                json!({ "owners": summary(&record.owners), "operators": summary(&record.operators) }),
                false,
            )
        }

        ("POST", ["operators"]) => {
            let person = match identity.create_person_by(text(body, "name"), Some(owner), now) {
                Ok(person) => person.clone(),
                Err(error) => return identity_error(error),
            };
            let mut operators = rooms.rooms[room].operators.clone();
            operators.push(person.id);
            rooms.set_members(room, Role::Operator, operators, now);
            let mut made = invite(identity, person.id, now, secrets);
            if let Some(object) = made.body.as_object_mut() {
                object.insert("person".into(), json!(person));
            }
            made
        }

        ("POST", ["operators", id, "invite"]) => {
            let Some(id) = person_id(id).filter(|id| rooms.rooms[room].operators.contains(id)) else {
                return identity_error(IdentityError::UnknownPerson);
            };
            if identity.person(&id).and_then(|person| person.invited_by) != Some(owner) {
                return RegistryResponse::error(403, "You can only make links for people you invited");
            }
            invite(identity, id, now, secrets)
        }

        ("DELETE", ["operators", id]) => {
            let Some(id) = person_id(id).filter(|id| rooms.rooms[room].operators.contains(id)) else {
                return identity_error(IdentityError::UnknownPerson);
            };
            let operators = rooms.rooms[room].operators.iter().copied().filter(|other| other != &id).collect();
            rooms.set_members(room, Role::Operator, operators, now);
            // someone this owner brought in and who is now in no room at all is
            // removed, so nobody is left signed in with nothing to do
            let invited_here = identity.person(&id).and_then(|person| person.invited_by) == Some(owner);
            if invited_here && rooms.rooms_of(&id).is_empty() {
                identity.remove_person(&id);
            }
            RegistryResponse::ok(json!({ "removed": id }), true)
        }

        _ => RegistryResponse::error(404, "not found"),
    };
    response.changed |= touched;
    response
}
