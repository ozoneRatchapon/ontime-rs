//! Gaps mutation testing found in who-gets-in (plan 019, item 7): the policy
//! numbers, what revoking and pruning report, and that the routes which change
//! sign-ins say so, or the registry would not store the change.

use ontime_core::directory::{handle, Directory};
use ontime_core::identity::{Identity, INVITE_TTL_MS, SESSION_TTL_MS, TOUCH_INTERVAL_MS};
use ontime_core::project::{hash_password, token_matches};
use serde_json::{json, Value};
use uuid::Uuid;

const NOW: i64 = 1_800_000_000_000;

fn counter() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("secret-{n}")
    }
}

fn person(identity: &mut Identity, name: &str) -> Uuid {
    identity.create_person(name, NOW).unwrap().id
}

fn call(
    directory: &mut Directory,
    method: &str,
    path: &str,
    body: Value,
    now: i64,
) -> (u16, Value, bool) {
    let segments: Vec<&str> = path.split('/').collect();
    let response = handle(directory, method, &segments, &body, now, &mut counter());
    (response.status, response.body, response.changed)
}

/// A person signed in on one device, and the session token.
fn signed_in(directory: &mut Directory, name: &str) -> (Uuid, String) {
    let mut secrets = counter();
    let id = person(&mut directory.identity, name);
    let (code, _) = directory
        .identity
        .issue_invite(id, NOW, &mut secrets)
        .unwrap();
    let (token, _) = directory
        .identity
        .redeem(&code, "Mac", NOW, &mut secrets)
        .unwrap();
    (id, token)
}

#[test]
fn the_policy_numbers() {
    const DAY: i64 = 24 * 60 * 60 * 1000;
    assert_eq!(INVITE_TTL_MS, 7 * DAY);
    assert_eq!(INVITE_TTL_MS, 604_800_000);
    assert_eq!(SESSION_TTL_MS, 7_776_000_000);
    assert_eq!(TOUCH_INTERVAL_MS, 3_600_000);
}

#[test]
fn a_token_whose_differences_cancel_out_still_fails() {
    // the same bit flipped in two places: an xor of the differences is zero
    assert!(!token_matches("ab", "`c"));
    assert!(!token_matches(&hash_password("a"), &hash_password("b")));
    assert!(token_matches("ab", "ab"));
}

#[test]
fn revoking_removes_only_that_persons_sign_ins_and_says_so() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let niran = person(&mut identity, "Niran");
    let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    identity.redeem(&code, "", NOW, &mut secrets).unwrap();
    identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    identity.issue_invite(niran, NOW, &mut secrets).unwrap();

    assert!(identity.revoke(&somsri));
    assert!(identity.sessions.is_empty());
    assert_eq!(identity.invites.len(), 1);
    assert_eq!(identity.invites[0].person, niran);
    // nothing left to revoke
    assert!(!identity.revoke(&somsri));
}

#[test]
fn pruning_drops_an_invite_the_moment_it_expires_and_says_so() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    identity.redeem(&code, "", NOW, &mut secrets).unwrap();
    identity.issue_invite(somsri, NOW, &mut secrets).unwrap();

    assert!(!identity.prune(NOW + INVITE_TTL_MS - 1));
    assert!(identity.prune(NOW + INVITE_TTL_MS));
    assert!(identity.invites.is_empty());
    assert_eq!(identity.sessions.len(), 1);
}

#[test]
fn signing_out_everywhere_is_a_stored_change() {
    let mut directory = Directory::default();
    let (id, token) = signed_in(&mut directory, "Somsri");
    let (status, body, changed) = call(
        &mut directory,
        "DELETE",
        &format!("people/{id}/sessions"),
        Value::Null,
        NOW,
    );
    assert_eq!((status, changed), (200, true));
    assert_eq!(body["signedOut"], json!(id));
    assert!(directory.identity.sessions.is_empty());
    let (status, _, _) = call(
        &mut directory,
        "POST",
        "auth/me",
        json!({ "token": token }),
        NOW,
    );
    assert_eq!(status, 401);
}

#[test]
fn signing_out_a_stranger_is_refused() {
    let mut directory = Directory::default();
    let (status, _, changed) = call(
        &mut directory,
        "DELETE",
        &format!("people/{}/sessions", Uuid::nil()),
        Value::Null,
        NOW,
    );
    assert_eq!((status, changed), (404, false));
}

#[test]
fn expired_sign_ins_go_with_any_write_but_not_a_read() {
    let mut directory = Directory::default();
    signed_in(&mut directory, "Somsri");
    let idle = NOW + SESSION_TTL_MS;
    let (_, _, changed) = call(&mut directory, "GET", "people", Value::Null, idle);
    assert!(!changed);
    assert_eq!(directory.identity.sessions.len(), 1);
    let (_, _, changed) = call(
        &mut directory,
        "POST",
        "people",
        json!({ "name": "Niran" }),
        idle,
    );
    assert!(changed);
    assert!(directory.identity.sessions.is_empty());
}

#[test]
fn the_admin_lists_people() {
    let mut directory = Directory::default();
    signed_in(&mut directory, "Somsri");
    let (status, body, changed) = call(&mut directory, "GET", "people", Value::Null, NOW);
    assert_eq!((status, changed), (200, false));
    assert_eq!(body["people"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["people"][0]["devices"], json!(["Mac"]));
}

#[test]
fn an_owner_named_twice_is_one_owner() {
    let mut directory = Directory::default();
    let (id, _) = signed_in(&mut directory, "Somsri");
    let (status, _, _) = call(
        &mut directory,
        "PUT",
        "rooms/show",
        json!({ "owners": [id, id] }),
        NOW,
    );
    assert_eq!(status, 200);
    let record = directory.rooms.rooms.get("show").unwrap();
    assert_eq!(record.owners, vec![id]);
}

#[test]
fn a_note_alone_is_not_a_team_change() {
    let mut directory = Directory::default();
    let (id, _) = signed_in(&mut directory, "Somsri");
    call(
        &mut directory,
        "PUT",
        "rooms/show",
        json!({ "owners": [id] }),
        NOW,
    );
    let (status, _, _) = call(
        &mut directory,
        "PUT",
        "rooms/show",
        json!({ "note": "Friday" }),
        NOW,
    );
    assert_eq!(status, 200);
    let record = directory.rooms.rooms.get("show").unwrap();
    assert_eq!(record.note, "Friday");
    assert_eq!(record.owners, vec![id]);
}

#[test]
fn a_refused_request_still_stores_the_sign_in_it_refreshed() {
    let mut directory = Directory::default();
    let (_, token) = signed_in(&mut directory, "Somsri");
    let later = NOW + TOUCH_INTERVAL_MS;
    // not a member: refused, but the session was used and moved on
    let (status, _, changed) = call(
        &mut directory,
        "POST",
        "auth/check",
        json!({ "token": token, "room": "show" }),
        later,
    );
    assert_eq!((status, changed), (403, true));
    // not an owner of the room's team, likewise
    let later = later + TOUCH_INTERVAL_MS;
    let (status, _, changed) = call(
        &mut directory,
        "GET",
        "auth/rooms/show/team",
        json!({ "token": token }),
        later,
    );
    assert_eq!((status, changed), (403, true));
}

#[test]
fn an_invite_for_someone_gone_is_spent_and_stored() {
    let mut directory = Directory::default();
    let mut secrets = counter();
    let id = person(&mut directory.identity, "Somsri");
    let (code, _) = directory
        .identity
        .issue_invite(id, NOW, &mut secrets)
        .unwrap();
    // still valid, so pruning keeps it; only trying it finds the person gone
    directory.identity.people.remove(&id);
    let (status, _, changed) = call(
        &mut directory,
        "POST",
        "auth/redeem",
        json!({ "code": code, "device": "Mac" }),
        NOW,
    );
    assert_eq!((status, changed), (401, true));
    assert!(directory.identity.invites.is_empty());
}
