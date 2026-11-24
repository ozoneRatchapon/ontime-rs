//! Phase 2: operators, owners managing their team, and members-only rooms.

use ontime_core::api::{handle, is_authorised, may_control, ApiRequest, HttpMethod, NEEDS_SIGN_IN};
use ontime_core::directory::{self, Directory};
use ontime_core::project::{hash_password, Project};
use ontime_core::registry::Role;
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

// --- the access rule ---------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Caller {
    Anonymous,
    RoomPassword,
    Master,
    Owner,
    Operator,
    WrongToken,
}

const CALLERS: [Caller; 6] = [
    Caller::Anonymous,
    Caller::RoomPassword,
    Caller::Master,
    Caller::Owner,
    Caller::Operator,
    Caller::WrongToken,
];

fn request(caller: Caller, method: HttpMethod, path: &str, body: Value) -> ApiRequest {
    let (token, member) = match caller {
        Caller::Anonymous => (None, None),
        Caller::RoomPassword => (Some(hash_password("room")), None),
        Caller::Master => (Some(hash_password("master")), None),
        Caller::Owner => (None, Some(Role::Owner)),
        Caller::Operator => (None, Some(Role::Operator)),
        Caller::WrongToken => (Some("nope".to_string()), None),
    };
    ApiRequest {
        method,
        path: path.into(),
        body,
        token,
        master_token: Some(hash_password("master")),
        room_is_new: false,
        member,
    }
}

fn room(password: bool, members_only: bool) -> Project {
    Project {
        password_hash: password.then(|| hash_password("room")),
        members_only,
        ..Project::default()
    }
}

/// (may watch, may control) for every caller, in `CALLERS` order.
fn matrix(project: &Project) -> Vec<(bool, bool)> {
    CALLERS
        .iter()
        .map(|&caller| {
            let req = request(caller, HttpMethod::Get, "/data/rundowns", json!({}));
            let watch = is_authorised(project, &req);
            (watch, watch && may_control(project, &req))
        })
        .collect()
}

#[test]
fn an_open_room_is_unchanged() {
    // everybody may do everything, exactly as before
    assert_eq!(matrix(&room(false, false)), vec![(true, true); 6]);
}

#[test]
fn a_password_room_also_lets_members_in() {
    assert_eq!(
        matrix(&room(true, false)),
        vec![
            (false, false), // anonymous
            (true, true),   // room password
            (true, true),   // master
            (true, true),   // owner — without knowing the password
            (true, true),   // operator
            (false, false), // wrong token
        ]
    );
}

#[test]
fn an_open_members_only_room_lets_anyone_watch_but_only_members_edit() {
    assert_eq!(
        matrix(&room(false, true)),
        vec![
            (true, false), // anonymous: the timer screen keeps working
            (true, false), // there is no room password to present
            (true, true),
            (true, true),
            (true, true),
            (true, false),
        ]
    );
}

#[test]
fn a_password_members_only_room_keeps_its_password_gate() {
    assert_eq!(
        matrix(&room(true, true)),
        vec![
            (false, false),
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (false, false)
        ]
    );
}

#[test]
fn a_members_only_room_refuses_edits_but_still_serves_reads() {
    let mut project = room(false, true);
    let read = handle(
        &mut project,
        &request(
            Caller::Anonymous,
            HttpMethod::Get,
            "/data/rundowns",
            json!({}),
        ),
        0,
    );
    assert_eq!(read.status, 200);

    let write = handle(
        &mut project,
        &request(
            Caller::Anonymous,
            HttpMethod::Post,
            "/data/rundowns",
            json!({ "title": "x" }),
        ),
        0,
    );
    assert_eq!(write.status, 403);
    assert_eq!(write.body.unwrap()["message"], NEEDS_SIGN_IN);

    // and claiming the room by setting a password is an edit like any other
    let claim = handle(
        &mut project,
        &request(
            Caller::Anonymous,
            HttpMethod::Post,
            "/data/session/password",
            json!({ "password": "mine" }),
        ),
        0,
    );
    assert_eq!(claim.status, 403);
    assert!(project.password_hash.is_none());

    let member = handle(
        &mut project,
        &request(
            Caller::Operator,
            HttpMethod::Post,
            "/data/rundowns",
            json!({ "title": "x" }),
        ),
        0,
    );
    assert_eq!(member.status, 200);
}

#[test]
fn only_an_owner_or_the_admin_flips_members_only() {
    for (caller, allowed) in [
        (Caller::Anonymous, false),
        (Caller::RoomPassword, false),
        (Caller::Operator, false),
        (Caller::Owner, true),
        (Caller::Master, true),
    ] {
        let mut project = room(false, false);
        let response = handle(
            &mut project,
            &request(
                caller,
                HttpMethod::Post,
                "/data/members-only",
                json!({ "on": true }),
            ),
            0,
        );
        assert_eq!(project.members_only, allowed, "{caller:?}");
        assert_eq!(response.status == 200, allowed, "{caller:?}");
    }
}

#[test]
fn info_tells_the_editor_whether_it_may_edit() {
    let mut project = room(false, true);
    let info = |project: &mut Project, caller| {
        handle(
            project,
            &request(caller, HttpMethod::Get, "/data/session/info", json!({})),
            0,
        )
        .body
        .unwrap()
    };
    let anonymous = info(&mut project, Caller::Anonymous);
    assert_eq!(
        (
            anonymous["membersOnly"].clone(),
            anonymous["canEdit"].clone()
        ),
        (json!(true), json!(false))
    );
    assert_eq!(info(&mut project, Caller::Operator)["canEdit"], json!(true));
}

#[test]
fn an_old_project_is_not_members_only() {
    let mut stored = serde_json::to_value(Project::default()).unwrap();
    assert!(stored
        .as_object_mut()
        .unwrap()
        .remove("membersOnly")
        .is_some());
    let project: Project = serde_json::from_value(stored).unwrap();
    assert!(!project.members_only);
}

// --- owners and their team ---------------------------------------------------

struct World {
    directory: Directory,
    secrets: Box<dyn FnMut() -> String>,
}

impl World {
    fn new() -> Self {
        let mut n = 0;
        Self {
            directory: Directory::default(),
            secrets: Box::new(move || {
                n += 1;
                format!("secret-{n}")
            }),
        }
    }

    fn call(&mut self, method: &str, path: &str, body: Value) -> (u16, Value) {
        let segments: Vec<&str> = path.split('/').collect();
        let mut secrets = || (self.secrets)();
        let response = directory::handle(
            &mut self.directory,
            method,
            &segments,
            &body,
            NOW,
            &mut secrets,
        );
        (response.status, response.body)
    }

    /// An admin-made person, signed in; returns (id, session token).
    fn person(&mut self, name: &str) -> (String, String) {
        let (_, person) = self.call("POST", "people", json!({ "name": name }));
        let id = person["id"].as_str().unwrap().to_string();
        let (_, invite) = self.call("POST", &format!("people/{id}/invite"), Value::Null);
        let (_, signed) = self.call("POST", "auth/redeem", json!({ "code": invite["code"] }));
        (id, signed["token"].as_str().unwrap().to_string())
    }

    fn sign_in(&mut self, code: &Value) -> String {
        let (status, signed) = self.call("POST", "auth/redeem", json!({ "code": code }));
        assert_eq!(status, 200);
        signed["token"].as_str().unwrap().to_string()
    }
}

#[test]
fn an_owner_invites_an_operator_who_can_then_run_the_room() {
    let mut world = World::new();
    let (owner, owner_token) = world.person("Owner");
    world.call("PUT", "rooms/exam", json!({ "owners": [owner] }));

    let (status, made) = world.call(
        "POST",
        "auth/rooms/exam/operators",
        json!({ "token": owner_token, "name": "Op" }),
    );
    assert_eq!(status, 200);
    let operator = made["person"]["id"].as_str().unwrap().to_string();
    let operator_token = world.sign_in(&made["code"]);

    let (status, checked) = world.call(
        "POST",
        "auth/check",
        json!({ "token": operator_token, "room": "exam" }),
    );
    assert_eq!((status, checked["role"].clone()), (200, json!("operator")));
    let (_, checked) = world.call(
        "POST",
        "auth/check",
        json!({ "token": owner_token, "room": "exam" }),
    );
    assert_eq!(checked["role"], "owner");

    // the operator sees the room, with their role
    let (_, me) = world.call("POST", "auth/me", json!({ "token": operator_token }));
    assert_eq!(
        (
            me["rooms"][0]["name"].clone(),
            me["rooms"][0]["role"].clone()
        ),
        (json!("exam"), json!("operator"))
    );

    // the team, as the owner sees it
    let (_, team) = world.call(
        "GET",
        "auth/rooms/exam/team",
        json!({ "token": owner_token }),
    );
    assert_eq!(team["operators"][0]["person"]["id"], json!(operator));
    assert_eq!(team["operators"][0]["mine"], json!(true));
    assert_eq!(team["owners"][0]["mine"], json!(false));
}

#[test]
fn membership_is_per_room() {
    let mut world = World::new();
    let (owner, owner_token) = world.person("Owner");
    world.call("PUT", "rooms/exam", json!({ "owners": [owner] }));
    let (status, _) = world.call(
        "POST",
        "auth/check",
        json!({ "token": owner_token, "room": "other" }),
    );
    assert_eq!(status, 403);
    let (status, _) = world.call(
        "POST",
        "auth/check",
        json!({ "token": "made-up", "room": "exam" }),
    );
    assert_eq!(status, 401);
}

#[test]
fn only_owners_manage_a_team() {
    let mut world = World::new();
    let (owner, owner_token) = world.person("Owner");
    let (_, stranger_token) = world.person("Stranger");
    world.call("PUT", "rooms/exam", json!({ "owners": [owner] }));
    let (_, made) = world.call(
        "POST",
        "auth/rooms/exam/operators",
        json!({ "token": owner_token, "name": "Op" }),
    );
    let operator_token = world.sign_in(&made["code"]);

    for token in [&stranger_token, &operator_token] {
        let (status, _) = world.call(
            "POST",
            "auth/rooms/exam/operators",
            json!({ "token": token, "name": "Sneaky" }),
        );
        assert_eq!(status, 403);
        let (status, _) = world.call("GET", "auth/rooms/exam/team", json!({ "token": token }));
        assert_eq!(status, 403);
    }
    let (status, _) = world.call("GET", "auth/rooms/exam/team", json!({}));
    assert_eq!(status, 401);
}

#[test]
fn an_owner_cannot_relink_someone_they_did_not_invite() {
    let mut world = World::new();
    let (owner, owner_token) = world.person("Owner A");
    // the admin makes B an operator of A's room; B also owns another room
    let (b, _) = world.person("Owner B");
    world.call(
        "PUT",
        "rooms/exam",
        json!({ "owners": [owner], "operators": [b] }),
    );
    world.call("PUT", "rooms/other", json!({ "owners": [b] }));

    let (status, body) = world.call(
        "POST",
        &format!("auth/rooms/exam/operators/{b}/invite"),
        json!({ "token": owner_token }),
    );
    assert_eq!(status, 403, "{body}");
    // nor can A remove B from B's own room, or from anywhere but A's room
    let (status, _) = world.call(
        "DELETE",
        &format!("auth/rooms/other/operators/{b}"),
        json!({ "token": owner_token }),
    );
    assert_eq!(status, 403);
}

#[test]
fn an_owner_relinks_and_removes_their_own_operator() {
    let mut world = World::new();
    let (owner, owner_token) = world.person("Owner");
    world.call("PUT", "rooms/exam", json!({ "owners": [owner] }));
    let (_, made) = world.call(
        "POST",
        "auth/rooms/exam/operators",
        json!({ "token": owner_token, "name": "Op" }),
    );
    let operator = made["person"]["id"].as_str().unwrap().to_string();

    let (status, relinked) = world.call(
        "POST",
        &format!("auth/rooms/exam/operators/{operator}/invite"),
        json!({ "token": owner_token }),
    );
    assert_eq!(status, 200);
    let operator_token = world.sign_in(&relinked["code"]);

    let (status, _) = world.call(
        "DELETE",
        &format!("auth/rooms/exam/operators/{operator}"),
        json!({ "token": owner_token }),
    );
    assert_eq!(status, 200);
    // in no room any more and brought in by this owner: gone entirely
    let (status, _) = world.call("POST", "auth/me", json!({ "token": operator_token }));
    assert_eq!(status, 401);
    assert!(world.directory.identity.people.len() == 1);
}

#[test]
fn removing_an_operator_who_is_elsewhere_keeps_them() {
    let mut world = World::new();
    let (owner, owner_token) = world.person("Owner");
    world.call("PUT", "rooms/exam", json!({ "owners": [owner] }));
    world.call("PUT", "rooms/second", json!({ "owners": [owner] }));
    let (_, made) = world.call(
        "POST",
        "auth/rooms/exam/operators",
        json!({ "token": owner_token, "name": "Op" }),
    );
    let operator = made["person"]["id"].as_str().unwrap().to_string();
    let operator_token = world.sign_in(&made["code"]);
    world.call("PUT", "rooms/second", json!({ "operators": [operator] }));

    world.call(
        "DELETE",
        &format!("auth/rooms/exam/operators/{operator}"),
        json!({ "token": owner_token }),
    );
    let (_, me) = world.call("POST", "auth/me", json!({ "token": operator_token }));
    let rooms: Vec<&str> = me["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(rooms, vec!["second"]);
}

#[test]
fn removing_a_person_takes_them_off_operator_lists_too() {
    let mut world = World::new();
    let (op, _) = world.person("Op");
    world.call("PUT", "rooms/exam", json!({ "operators": [op] }));
    world.call("DELETE", &format!("people/{op}"), Value::Null);
    assert!(world.directory.rooms.rooms["exam"].operators.is_empty());
}
