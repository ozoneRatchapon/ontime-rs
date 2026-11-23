//! The registry instance's whole surface: the room list plus the people who
//! own rooms. Routes that touch only rooms go to [`registry::handle`]; the
//! rest need both documents, because people own rooms.
//!
//! Who may call what is the host's job: `people` and `rooms` sit behind the
//! master token, `auth` is public, `report` is internal.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::identity::{Identity, IdentityError, Secrets};
use crate::registry::{self, Registry, RegistryResponse};

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
/// - `GET    people`                  everyone, with devices and pending invites
/// - `POST   people`                  `{ name }`
/// - `POST   people/<id>/invite`      a new invite code, replacing any unused one
/// - `DELETE people/<id>/sessions`    sign that person out everywhere
/// - `DELETE people/<id>`             remove them, and from every room they own
/// - `PUT    rooms/<name>`            also accepts `{ owners: [id] }`
/// - `POST   auth/redeem`             `{ code, device }` -> `{ token, person }`
/// - `POST   auth/me`                 `{ token }` -> `{ person, rooms }`
/// - `POST   auth/logout`             `{ token }`
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
    let mut response = route(rooms, identity, method, segments, body, now, secrets);
    response.changed |= pruned;
    response
}

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
            match identity.issue_invite(id, now, secrets) {
                Ok((code, expires_at)) => RegistryResponse::ok(json!({ "code": code, "expiresAt": expires_at }), true),
                Err(error) => identity_error(error),
            }
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
                rooms.drop_owner(&id);
                RegistryResponse::ok(json!({ "removed": id }), true)
            }
            None => identity_error(IdentityError::UnknownPerson),
        },

        ("PUT", ["rooms", name]) if body.get("owners").is_some() => {
            let Some(listed) = body.get("owners").and_then(Value::as_array) else {
                return RegistryResponse::error(400, "owners has to be a list of people");
            };
            let mut owners = Vec::new();
            for raw in listed {
                match raw.as_str().and_then(person_id).filter(|id| identity.person(id).is_some()) {
                    Some(id) if !owners.contains(&id) => owners.push(id),
                    Some(_) => {}
                    None => return RegistryResponse::error(400, "owners names someone who is not a person"),
                }
            }
            if rooms.set_owners(name, owners, now).is_none() {
                return RegistryResponse::error(400, "not a valid room name");
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
                json!({ "person": identity.person(&id), "rooms": rooms.owned_by(&id) }),
                touched,
            ),
            None => RegistryResponse::error(401, SIGNED_OUT),
        },

        ("POST", ["auth", "logout"]) => match identity.sign_out(text(body, "token"), now) {
            true => RegistryResponse::ok(json!({}), true),
            false => RegistryResponse::error(401, SIGNED_OUT),
        },

        _ => registry::handle(rooms, method, segments, body, now),
    }
}
