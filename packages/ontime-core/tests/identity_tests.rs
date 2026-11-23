use ontime_core::directory::{handle, Directory};
use ontime_core::identity::{device_label, Identity, IdentityError, INVITE_TTL_MS, SESSION_TTL_MS, TOUCH_INTERVAL_MS};
use ontime_core::registry::{Registry, RoomRecord};
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

#[test]
fn an_invite_signs_in_once() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (code, expires_at) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    assert_eq!(expires_at, NOW + INVITE_TTL_MS);

    let (token, who) = identity.redeem(&code, "iPhone Safari", NOW + 1, &mut secrets).unwrap();
    assert_eq!(who.id, somsri);
    assert_eq!(identity.authenticate(&token, NOW + 2), Some((somsri, false)));

    let again = identity.redeem(&code, "Mac Chrome", NOW + 3, &mut secrets);
    assert_eq!(again.unwrap_err(), IdentityError::InvalidInvite);
}

#[test]
fn an_expired_invite_fails_and_is_spent() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let late = identity.redeem(&code, "", NOW + INVITE_TTL_MS, &mut secrets);
    assert_eq!(late.unwrap_err(), IdentityError::InvalidInvite);
    assert!(identity.invites.is_empty());
}

#[test]
fn a_new_invite_replaces_the_old_one() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (old, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let (new, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    assert_eq!(identity.invites.len(), 1);
    assert!(identity.redeem(&old, "", NOW, &mut secrets).is_err());
    assert!(identity.redeem(&new, "", NOW, &mut secrets).is_ok());
}

#[test]
fn only_hashes_are_stored() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let stored = serde_json::to_string(&identity).unwrap();
    assert!(!stored.contains(&code));
    let (token, _) = identity.redeem(&code, "", NOW, &mut secrets).unwrap();
    let stored = serde_json::to_string(&identity).unwrap();
    assert!(!stored.contains(&token));
}

#[test]
fn a_session_token_is_not_an_invite_code() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    assert_eq!(identity.authenticate(&code, NOW), None);
}

#[test]
fn sessions_slide_and_expire() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let (token, _) = identity.redeem(&code, "", NOW, &mut secrets).unwrap();

    // used within the hour: nothing to store
    assert_eq!(identity.authenticate(&token, NOW + 1000), Some((somsri, false)));
    // used later: last_seen moves, which extends the session
    let later = NOW + SESSION_TTL_MS - 1;
    assert_eq!(identity.authenticate(&token, later), Some((somsri, true)));
    assert_eq!(identity.authenticate(&token, later + TOUCH_INTERVAL_MS), Some((somsri, true)));
    // left unused for the whole lifetime: gone
    let idle = later + TOUCH_INTERVAL_MS + SESSION_TTL_MS;
    assert_eq!(identity.authenticate(&token, idle), None);
    assert!(identity.prune(idle));
    assert!(identity.sessions.is_empty());
}

#[test]
fn revoking_signs_out_every_device() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let mut tokens = Vec::new();
    for _ in 0..2 {
        let (code, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
        tokens.push(identity.redeem(&code, "", NOW, &mut secrets).unwrap().0);
    }
    assert!(identity.revoke(&somsri));
    for token in tokens {
        assert_eq!(identity.authenticate(&token, NOW), None);
    }
    assert!(identity.person(&somsri).is_some());
}

#[test]
fn signing_out_ends_only_that_device() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    let (a, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let (phone, _) = identity.redeem(&a, "", NOW, &mut secrets).unwrap();
    let (b, _) = identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let (laptop, _) = identity.redeem(&b, "", NOW, &mut secrets).unwrap();
    assert!(identity.sign_out(&phone, NOW));
    assert_eq!(identity.authenticate(&phone, NOW), None);
    assert!(identity.authenticate(&laptop, NOW).is_some());
}

#[test]
fn a_person_needs_a_name() {
    let mut identity = Identity::default();
    assert_eq!(identity.create_person("   ", NOW).unwrap_err(), IdentityError::InvalidName);
}

#[test]
fn summaries_never_carry_hashes() {
    let mut identity = Identity::default();
    let mut secrets = counter();
    let somsri = person(&mut identity, "Somsri");
    identity.issue_invite(somsri, NOW, &mut secrets).unwrap();
    let listed = serde_json::to_value(identity.summaries()).unwrap();
    let text = listed.to_string();
    assert!(!text.contains("Hash"));
    assert_eq!(listed[0]["name"], "Somsri");
    assert_eq!(listed[0]["inviteExpiresAt"], NOW + INVITE_TTL_MS);
}

#[test]
fn device_labels_are_short() {
    let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
    let mac_chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";
    assert_eq!(device_label(iphone), "iPhone Safari");
    assert_eq!(device_label(mac_chrome), "Mac Chrome");
    assert_eq!(device_label(""), "Device browser");
}

#[test]
fn an_old_room_record_still_loads() {
    let old = r#"{"rooms":{"a":{"name":"a","owner":"Aj. A","note":"","protected":false,"rundowns":1,"firstSeen":1,"lastSeen":2}}}"#;
    let registry: Registry = serde_json::from_str(old).unwrap();
    let record: &RoomRecord = &registry.rooms["a"];
    assert_eq!(record.owner, "Aj. A");
    assert!(record.owners.is_empty());
}

// --- the routes, as the registry instance sees them

fn call(directory: &mut Directory, method: &str, path: &str, body: Value, secrets: &mut impl FnMut() -> String) -> (u16, Value, bool) {
    let segments: Vec<&str> = path.split('/').collect();
    let response = handle(directory, method, &segments, &body, NOW, secrets);
    (response.status, response.body, response.changed)
}

#[test]
fn owners_see_their_rooms() {
    let mut directory = Directory::default();
    let mut secrets = counter();
    let (_, created, _) = call(&mut directory, "POST", "people", json!({ "name": "Somsri" }), &mut secrets);
    let id = created["id"].as_str().unwrap().to_string();

    let (status, invite, _) = call(&mut directory, "POST", &format!("people/{id}/invite"), Value::Null, &mut secrets);
    assert_eq!(status, 200);
    let code = invite["code"].as_str().unwrap();

    let (status, signed_in, changed) = call(&mut directory, "POST", "auth/redeem", json!({ "code": code, "device": "Mac" }), &mut secrets);
    assert_eq!((status, changed), (200, true));
    let token = signed_in["token"].as_str().unwrap().to_string();

    let (_, me, _) = call(&mut directory, "POST", "auth/me", json!({ "token": token }), &mut secrets);
    assert_eq!(me["person"]["name"], "Somsri");
    assert_eq!(me["rooms"], json!([]));

    let (status, room, _) = call(&mut directory, "PUT", "rooms/exam", json!({ "owners": [id], "note": "IS" }), &mut secrets);
    assert_eq!(status, 200);
    assert_eq!(room["note"], "IS");
    let (_, me, _) = call(&mut directory, "POST", "auth/me", json!({ "token": token }), &mut secrets);
    assert_eq!(me["rooms"][0]["name"], "exam");

    // removing the person takes them off the room and signs them out
    let (status, _, _) = call(&mut directory, "DELETE", &format!("people/{id}"), Value::Null, &mut secrets);
    assert_eq!(status, 200);
    assert!(directory.rooms.rooms["exam"].owners.is_empty());
    let (status, _, _) = call(&mut directory, "POST", "auth/me", json!({ "token": token }), &mut secrets);
    assert_eq!(status, 401);
}

#[test]
fn owners_must_be_people() {
    let mut directory = Directory::default();
    let mut secrets = counter();
    let stranger = Uuid::now_v7().to_string();
    let (status, _, _) = call(&mut directory, "PUT", "rooms/exam", json!({ "owners": [stranger] }), &mut secrets);
    assert_eq!(status, 400);
    let (status, _, _) = call(&mut directory, "PUT", "rooms/exam", json!({ "owners": "nobody" }), &mut secrets);
    assert_eq!(status, 400);
    assert!(directory.rooms.rooms.is_empty());
}

#[test]
fn a_made_up_code_changes_nothing() {
    let mut directory = Directory::default();
    let mut secrets = counter();
    let (status, body, changed) = call(&mut directory, "POST", "auth/redeem", json!({ "code": "guess" }), &mut secrets);
    assert_eq!((status, changed), (401, false));
    assert!(body["message"].as_str().unwrap().contains("not valid"));
}

#[test]
fn room_routes_still_work() {
    let mut directory = Directory::default();
    let mut secrets = counter();
    let (status, _, changed) = call(&mut directory, "PUT", "rooms/exam", json!({ "owner": "Aj. A" }), &mut secrets);
    assert_eq!((status, changed), (200, true));
    let (_, listed, _) = call(&mut directory, "GET", "rooms", Value::Null, &mut secrets);
    assert_eq!(listed["rooms"][0]["owner"], "Aj. A");
}
