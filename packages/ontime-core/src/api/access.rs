//! Who may read, change, create or administer a room.

use crate::project::{token_matches, Project};
use crate::registry::Role;

use super::*;

/// Whether the request may act on this room.
///
/// A room with no password is open, matching the original server with
/// `SESSION_PASSWORD` unset, so adding this could not lock anyone out.
///
/// Public because a websocket upgrade never reaches [`handle`] — the host owns
/// the socket — but must be gated by exactly the same rule.
pub fn is_authorised(project: &Project, req: &ApiRequest) -> bool {
    // a member is let in without knowing the room password
    if req.member.is_some() {
        return true;
    }
    let Some(room_hash) = project.password_hash.as_deref() else {
        return true;
    };
    let Some(presented) = req.token.as_deref() else {
        return false;
    };
    if token_matches(room_hash, presented) {
        return true;
    }
    match req.master_token.as_deref() {
        Some(master) => token_matches(master, presented),
        None => false,
    }
}

/// Whether `presented` is the room password's token or the master token: a
/// refused request carrying anything else was a wrong guess, and is counted by
/// [`crate::guard::Guard`].
pub fn is_known_token(project: &Project, master: Option<&str>, presented: &str) -> bool {
    let room = project
        .password_hash
        .as_deref()
        .is_some_and(|hash| token_matches(hash, presented));
    room || master.is_some_and(|master| token_matches(master, presented))
}

/// Whether the caller holds more than a link: a membership, the room
/// password, or the master password.
pub fn is_privileged(project: &Project, req: &ApiRequest) -> bool {
    if req.member.is_some() {
        return true;
    }
    let Some(presented) = req.token.as_deref() else {
        return false;
    };
    let room = project
        .password_hash
        .as_deref()
        .is_some_and(|hash| token_matches(hash, presented));
    let master = req
        .master_token
        .as_deref()
        .is_some_and(|master| token_matches(master, presented));
    room || master
}

/// Whether the caller may change the room — edit it, or drive its timers.
/// Only a members-only room asks for more than being let in.
pub fn may_control(project: &Project, req: &ApiRequest) -> bool {
    !project.members_only || is_privileged(project, req)
}

/// Turning members-only on or off is for an owner or the admin — not an
/// operator, and not the room password, which members-only is there to replace.
pub(crate) fn may_switch_members_only(req: &ApiRequest) -> bool {
    is_owner_or_admin(req)
}

/// The master password, or an owner's session. Who may switch members-only,
/// and who may read the room's activity.
pub fn is_owner_or_admin(req: &ApiRequest) -> bool {
    is_admin(req) || req.member == Some(Role::Owner)
}

/// Whether the request presents the master password.
pub fn is_admin(req: &ApiRequest) -> bool {
    match (req.master_token.as_deref(), req.token.as_deref()) {
        (Some(master), Some(presented)) => token_matches(master, presented),
        _ => false,
    }
}

pub const NEEDS_SIGN_IN: &str =
    "This room needs sign-in to edit. Ask its owner for an invite link.";

/// Whether a request is allowed to bring a new room into existence.
///
/// Open when no master token is configured, which is how a single operator or a
/// LAN install is expected to run. Once one is configured, a new room needs it —
/// existing rooms are unaffected, so turning this on cannot lock anyone out of
/// what they already have.
pub fn may_create_room(req: &ApiRequest) -> bool {
    if !req.room_is_new {
        return true;
    }
    let Some(master) = req.master_token.as_deref() else {
        return true;
    };
    req.token
        .as_deref()
        .is_some_and(|presented| token_matches(master, presented))
}
