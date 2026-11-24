//! People, invite links and device sessions.
//!
//! The admin adds a person and sends them a personal link; opening it once
//! signs that device in. There is no password and no email. Only blake3 hashes
//! of invite codes and session tokens are stored, so a storage dump signs no
//! one in.
//!
//! Plain data, so it is tested without a Worker. Randomness is passed in by the
//! host (see [`Secrets`]), which keeps every function here deterministic.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::project::token_matches;

/// How long an unused invite link stays valid.
pub const INVITE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// How long a device stays signed in without being used.
pub const SESSION_TTL_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// `last_seen` is refreshed at most this often, so using a session does not
/// cost a storage write every time.
pub const TOUCH_INTERVAL_MS: i64 = 60 * 60 * 1000;

const MAX_NAME_CHARS: usize = 80;

/// Source of fresh, unguessable secrets. The Worker draws them from the
/// platform's crypto RNG; tests use a counter.
pub trait Secrets {
    fn fresh(&mut self) -> String;
}

impl<F: FnMut() -> String> Secrets for F {
    fn fresh(&mut self) -> String {
        self()
    }
}

/// Domain-separated, so a session token can never pass as an invite code.
fn digest(kind: &str, secret: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ontime-identity-v1:");
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(secret.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    pub created_at: i64,
    /// The owner who invited them; `None` when the admin did. An owner may only
    /// make new links for people they invited, or they could sign in as anyone.
    #[serde(default)]
    pub invited_by: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub code_hash: String,
    pub person: Uuid,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token_hash: String,
    pub person: Uuid,
    pub created_at: i64,
    pub last_seen: i64,
    /// A short label such as "iPhone Safari", so sessions can be told apart.
    pub device: String,
}

/// What the admin sees about a person — never any hash.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonSummary<'a> {
    #[serde(flatten)]
    pub person: &'a Person,
    pub devices: Vec<&'a str>,
    pub last_seen: i64,
    /// Expiry of the person's live invite link, if one is waiting to be opened.
    pub invite_expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    UnknownPerson,
    InvalidName,
    /// Deliberately one case for "never existed", "already used" and
    /// "expired", so a caller learns nothing by probing.
    InvalidInvite,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub people: BTreeMap<Uuid, Person>,
    pub invites: Vec<Invite>,
    pub sessions: Vec<Session>,
}

impl Identity {
    pub fn create_person(&mut self, name: &str, now: i64) -> Result<&Person, IdentityError> {
        self.create_person_by(name, None, now)
    }

    pub fn create_person_by(&mut self, name: &str, invited_by: Option<Uuid>, now: i64) -> Result<&Person, IdentityError> {
        let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
        if name.is_empty() {
            return Err(IdentityError::InvalidName);
        }
        let id = Uuid::now_v7();
        self.people.insert(
            id,
            Person {
                id,
                name,
                created_at: now,
                invited_by,
            },
        );
        Ok(&self.people[&id])
    }

    pub fn person(&self, id: &Uuid) -> Option<&Person> {
        self.people.get(id)
    }

    /// Issues a new invite link code, replacing any unused one, so a person
    /// only ever has one live link. Returns the code — the only time it exists
    /// in the clear.
    pub fn issue_invite(&mut self, person: Uuid, now: i64, secrets: &mut impl Secrets) -> Result<(String, i64), IdentityError> {
        if !self.people.contains_key(&person) {
            return Err(IdentityError::UnknownPerson);
        }
        self.invites.retain(|invite| invite.person != person);
        let code = secrets.fresh();
        let expires_at = now + INVITE_TTL_MS;
        self.invites.push(Invite {
            code_hash: digest("invite", &code),
            person,
            expires_at,
        });
        Ok((code, expires_at))
    }

    /// Spends an invite and signs a device in. Returns the session token.
    pub fn redeem(&mut self, code: &str, device: &str, now: i64, secrets: &mut impl Secrets) -> Result<(String, &Person), IdentityError> {
        let hash = digest("invite", code);
        let index = self
            .invites
            .iter()
            .position(|invite| token_matches(&invite.code_hash, &hash))
            .ok_or(IdentityError::InvalidInvite)?;
        let invite = self.invites.remove(index);
        if invite.expires_at <= now || !self.people.contains_key(&invite.person) {
            return Err(IdentityError::InvalidInvite);
        }
        let token = secrets.fresh();
        self.sessions.push(Session {
            token_hash: digest("session", &token),
            person: invite.person,
            created_at: now,
            last_seen: now,
            device: device.chars().take(MAX_NAME_CHARS).collect(),
        });
        Ok((token, &self.people[&invite.person]))
    }

    fn session_index(&self, token: &str, now: i64) -> Option<usize> {
        let hash = digest("session", token);
        self.sessions
            .iter()
            .position(|session| token_matches(&session.token_hash, &hash))
            .filter(|&index| now - self.sessions[index].last_seen < SESSION_TTL_MS)
    }

    /// Who a session token belongs to. The second value says whether
    /// `last_seen` moved, i.e. whether the identity has to be stored.
    /// Whose live session a token is, without touching it.
    pub fn person_of(&self, token: &str, now: i64) -> Option<Uuid> {
        self.session_index(token, now).map(|index| self.sessions[index].person)
    }

    pub fn authenticate(&mut self, token: &str, now: i64) -> Option<(Uuid, bool)> {
        let index = self.session_index(token, now)?;
        let session = &mut self.sessions[index];
        let touched = now - session.last_seen >= TOUCH_INTERVAL_MS;
        if touched {
            session.last_seen = now;
        }
        Some((session.person, touched))
    }

    /// Signs one device out.
    pub fn sign_out(&mut self, token: &str, now: i64) -> bool {
        match self.session_index(token, now) {
            Some(index) => {
                self.sessions.remove(index);
                true
            }
            None => false,
        }
    }

    /// Signs a person out of every device and cancels their invite link.
    pub fn revoke(&mut self, person: &Uuid) -> bool {
        let before = self.sessions.len() + self.invites.len();
        self.sessions.retain(|session| &session.person != person);
        self.invites.retain(|invite| &invite.person != person);
        before != self.sessions.len() + self.invites.len()
    }

    pub fn remove_person(&mut self, person: &Uuid) -> bool {
        self.revoke(person);
        self.people.remove(person).is_some()
    }

    /// Drops expired invites and sessions. Returns whether anything went.
    pub fn prune(&mut self, now: i64) -> bool {
        let before = self.sessions.len() + self.invites.len();
        self.invites.retain(|invite| invite.expires_at > now);
        self.sessions.retain(|session| now - session.last_seen < SESSION_TTL_MS);
        before != self.sessions.len() + self.invites.len()
    }

    /// Everyone, by name, with their devices and any pending invite.
    pub fn summaries(&self) -> Vec<PersonSummary<'_>> {
        let mut people: Vec<PersonSummary> = self
            .people
            .values()
            .map(|person| {
                let sessions: Vec<&Session> = self.sessions.iter().filter(|s| s.person == person.id).collect();
                PersonSummary {
                    person,
                    devices: sessions.iter().map(|s| s.device.as_str()).collect(),
                    last_seen: sessions.iter().map(|s| s.last_seen).max().unwrap_or(0),
                    invite_expires_at: self
                        .invites
                        .iter()
                        .find(|invite| invite.person == person.id)
                        .map(|invite| invite.expires_at),
                }
            })
            .collect();
        people.sort_by(|a, b| a.person.name.cmp(&b.person.name));
        people
    }
}

/// A short, human label for a User-Agent: "<device> <browser>".
pub fn device_label(user_agent: &str) -> String {
    let has = |needle: &str| user_agent.contains(needle);
    let device = match () {
        _ if has("iPhone") => "iPhone",
        _ if has("iPad") => "iPad",
        _ if has("Android") => "Android",
        _ if has("Macintosh") => "Mac",
        _ if has("Windows") => "Windows",
        _ if has("Linux") => "Linux",
        _ => "Device",
    };
    let browser = match () {
        _ if has("Edg/") => "Edge",
        _ if has("Line/") => "LINE",
        _ if has("Firefox/") || has("FxiOS") => "Firefox",
        _ if has("Chrome/") || has("CriOS") => "Chrome",
        _ if has("Safari/") => "Safari",
        _ => "browser",
    };
    format!("{device} {browser}")
}
