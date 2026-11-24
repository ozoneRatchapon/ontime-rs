//! The room registry: which rooms exist, who looks after each, and when it was
//! last used.
//!
//! A room is a Durable Object addressed by name, so nothing ever lists them — the
//! platform only knows hashed ids. Each room therefore reports itself here (see
//! [`should_report`]) and an operator adds the owner and a note by hand.
//!
//! All of it is plain data and routing, so it is tested without a Worker; the
//! host only loads the document, calls [`handle`], and stores it back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// The Durable Object name the registry lives under. `@` is not allowed in a
/// room name, so no room can ever collide with it.
pub const REGISTRY_NAME: &str = "@registry";

/// The room a request without one lands in.
pub const DEFAULT_ROOM: &str = "default_room";

/// A room reports at most this often while it is in use.
pub const REPORT_INTERVAL_MS: i64 = 10 * 60 * 1000;

/// Owner and note are labels for people, not documents.
const MAX_LABEL_CHARS: usize = 200;

/// A room name has to be safe to put in a URL and stable as a Durable Object key.
pub fn is_valid_room(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether a room should tell the registry about itself now: when it never
/// has, when what it would say has changed (a new title must not wait out the
/// interval), or when the interval has passed, to refresh "last used".
pub fn should_report(last: Option<&(i64, RoomReport)>, current: &RoomReport, now: i64) -> bool {
    match last {
        None => true,
        Some((at, sent)) => sent != current || now - at >= REPORT_INTERVAL_MS,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoomRecord {
    pub name: String,
    /// The project title the room last reported.
    pub title: String,
    /// A free-text owner label from before people existed. Kept, and shown,
    /// until an operator clears it, so no label is lost.
    pub owner: String,
    /// The people who own the room — see `identity`.
    pub owners: Vec<Uuid>,
    /// People an owner (or the admin) let run the room.
    pub operators: Vec<Uuid>,
    /// Only members, the room password or the master password may control it.
    pub members_only: bool,
    pub note: String,
    pub protected: bool,
    pub rundowns: usize,
    /// When the registry first heard of the room. For rooms older than the
    /// registry this is the first use after it was introduced, not their birth.
    pub first_seen: i64,
    /// Last report, or 0 when only registered by hand and not used since.
    pub last_seen: i64,
}

/// What a room says about itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoomReport {
    pub room: String,
    pub title: String,
    pub protected: bool,
    pub rundowns: usize,
    pub members_only: bool,
}

/// What a signed-in person is to a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Operator,
}

/// Who a session belongs to and what they are to one room, as `auth/check`
/// answers it. The room keeps this for a minute, and its audit log uses the
/// name as it is at that moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub person: Uuid,
    /// Missing from a registry that predates the audit log.
    #[serde(default)]
    pub name: String,
    pub role: Role,
}

/// A room as one of its members sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberRoom<'a> {
    #[serde(flatten)]
    pub room: &'a RoomRecord,
    pub role: Role,
}

impl RoomRecord {
    /// Owner wins when someone is listed as both.
    pub fn role_of(&self, person: &Uuid) -> Option<Role> {
        match () {
            _ if self.owners.contains(person) => Some(Role::Owner),
            _ if self.operators.contains(person) => Some(Role::Operator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub rooms: BTreeMap<String, RoomRecord>,
}

fn label(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().chars().take(MAX_LABEL_CHARS).collect())
}

impl Registry {
    /// The record for a room, registering it if it is not known yet — that is
    /// how a room older than the registry is added.
    fn entry(&mut self, name: &str, now: i64) -> &mut RoomRecord {
        self.rooms.entry(name.to_string()).or_insert_with(|| RoomRecord {
            name: name.to_string(),
            first_seen: now,
            ..RoomRecord::default()
        })
    }

    /// Records a room's own report. Owner and note are left alone — the room
    /// does not know them.
    pub fn apply_report(&mut self, report: &RoomReport, now: i64) -> bool {
        if !is_valid_room(&report.room) {
            return false;
        }
        let record = self.entry(&report.room, now);
        record.title = report.title.clone();
        record.protected = report.protected;
        record.rundowns = report.rundowns;
        record.members_only = report.members_only;
        record.last_seen = now;
        true
    }

    /// Sets owner and/or note, registering the room if it is not known yet —
    /// that is how a room older than the registry is added.
    pub fn set_labels(&mut self, name: &str, body: &Value, now: i64) -> Option<&RoomRecord> {
        if !is_valid_room(name) {
            return None;
        }
        let record = self.entry(name, now);
        if let Some(owner) = label(body, "owner") {
            record.owner = owner;
        }
        if let Some(note) = label(body, "note") {
            record.note = note;
        }
        Some(record)
    }

    /// Replaces a room's owners or operators, registering the room if it is
    /// not known yet. The caller has already checked every id is a real person.
    pub fn set_members(&mut self, name: &str, role: Role, people: Vec<Uuid>, now: i64) -> Option<&RoomRecord> {
        if !is_valid_room(name) {
            return None;
        }
        let record = self.entry(name, now);
        match role {
            Role::Owner => record.owners = people,
            Role::Operator => record.operators = people,
        }
        Some(record)
    }

    /// Takes a person off every room.
    pub fn drop_person(&mut self, person: &Uuid) {
        for record in self.rooms.values_mut() {
            record.owners.retain(|owner| owner != person);
            record.operators.retain(|operator| operator != person);
        }
    }

    /// Every room a person belongs to, in list order.
    pub fn rooms_of(&self, person: &Uuid) -> Vec<MemberRoom<'_>> {
        self.list()
            .into_iter()
            .filter_map(|room| room.role_of(person).map(|role| MemberRoom { room, role }))
            .collect()
    }

    /// What a person is to a named room.
    pub fn role_in(&self, name: &str, person: &Uuid) -> Option<Role> {
        self.rooms.get(name)?.role_of(person)
    }

    /// Drops a room from the list. Its data is untouched — the next report
    /// brings it back.
    pub fn forget(&mut self, name: &str) -> bool {
        self.rooms.remove(name).is_some()
    }

    /// Most recently used first; rooms never seen in use go last, by name.
    pub fn list(&self) -> Vec<&RoomRecord> {
        let mut rooms: Vec<&RoomRecord> = self.rooms.values().collect();
        rooms.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(a.name.cmp(&b.name)));
        rooms
    }
}

/// What a registry request produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryResponse {
    pub status: u16,
    pub body: Value,
    /// The registry changed and has to be stored.
    pub changed: bool,
}

impl RegistryResponse {
    pub fn ok(body: Value, changed: bool) -> Self {
        Self { status: 200, body, changed }
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            body: json!({ "message": message }),
            changed: false,
        }
    }
}

/// Routes a registry request. `segments` is the path below the registry root:
///
/// - `GET    rooms`         every known room
/// - `PUT    rooms/<name>`  set `{ owner?, note? }`, registering the room if new
/// - `DELETE rooms/<name>`  forget a room (its data stays)
/// - `POST   report`        a room reporting itself — internal, never routed from outside
pub fn handle(
    registry: &mut Registry,
    method: &str,
    segments: &[&str],
    body: &Value,
    now: i64,
) -> RegistryResponse {
    match (method, segments) {
        ("GET", ["rooms"]) => RegistryResponse::ok(json!({ "rooms": registry.list() }), false),

        ("PUT", ["rooms", name]) => match registry.set_labels(name, body, now) {
            Some(record) => RegistryResponse::ok(json!(record), true),
            None => RegistryResponse::error(400, "not a valid room name"),
        },

        ("DELETE", ["rooms", name]) => {
            if registry.forget(name) {
                RegistryResponse::ok(json!({ "forgotten": name }), true)
            } else {
                RegistryResponse::error(404, "room not in the registry")
            }
        }

        ("POST", ["report"]) => match serde_json::from_value::<RoomReport>(body.clone()) {
            Ok(report) if registry.apply_report(&report, now) => RegistryResponse::ok(json!({}), true),
            _ => RegistryResponse::error(400, "not a valid report"),
        },

        _ => RegistryResponse::error(404, "not found"),
    }
}
