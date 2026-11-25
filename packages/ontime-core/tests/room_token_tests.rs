//! Room tokens since plan 019, item 2: a room is locked with a random token,
//! not one made from a password, and a room locked before keeps its links.

use ontime_core::api::{handle, is_authorised, ApiRequest, HttpMethod};
use ontime_core::project::{fresh_room_token, hash_password, Project};
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

fn post(token: Option<&str>, path: &str, body: Value) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Post,
        path: path.into(),
        body,
        token: token.map(str::to_string),
        master_token: None,
        room_is_new: false,
        member: None,
    }
}

fn read(token: Option<&str>) -> ApiRequest {
    ApiRequest {
        method: HttpMethod::Get,
        ..post(token, "/data/settings", Value::Null)
    }
}

/// Locks `project` and returns the token it answered with.
fn lock(project: &mut Project, token: Option<&str>, body: Value) -> String {
    let response = handle(project, &post(token, "/data/session/password", body), NOW);
    assert_eq!(response.status, 200);
    let body = response.body.unwrap();
    assert_eq!(body["protected"], json!(true));
    body["token"].as_str().unwrap().to_string()
}

#[test]
fn fresh_tokens_are_random_and_shaped_like_the_old_ones() {
    let a = fresh_room_token();
    let b = fresh_room_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn locking_makes_a_token_no_password_leads_to() {
    let mut project = Project::default();
    let token = lock(&mut project, None, json!({ "password": "show-secret" }));
    assert_ne!(token, hash_password("show-secret"));
    assert_eq!(project.password_hash.as_deref(), Some(token.as_str()));
    assert!(is_authorised(&project, &read(Some(&token))));
    assert!(!is_authorised(
        &project,
        &read(Some(&hash_password("show-secret")))
    ));
    assert!(!is_authorised(&project, &read(None)));
}

#[test]
fn the_same_password_twice_gives_two_tokens() {
    let mut one = Project::default();
    let mut two = Project::default();
    let a = lock(&mut one, None, json!({ "password": "same" }));
    let b = lock(&mut two, None, json!({ "password": "same" }));
    assert_ne!(a, b);
}

#[test]
fn our_client_locks_without_a_password() {
    let mut project = Project::default();
    let token = lock(&mut project, None, json!({ "lock": true }));
    assert!(is_authorised(&project, &read(Some(&token))));
}

#[test]
fn locking_again_makes_a_new_link_and_retires_the_old() {
    let mut project = Project::default();
    let old = lock(&mut project, None, json!({ "lock": true }));
    let new = lock(&mut project, Some(&old), json!({ "lock": true }));
    assert_ne!(old, new);
    assert!(is_authorised(&project, &read(Some(&new))));
    assert!(!is_authorised(&project, &read(Some(&old))));
}

#[test]
fn a_room_locked_before_keeps_its_links() {
    // what the storage of a room locked before plan 019 holds
    let legacy = hash_password("the old password");
    let mut project = Project {
        password_hash: Some(legacy.clone()),
        ..Project::default()
    };
    assert!(is_authorised(&project, &read(Some(&legacy))));
    // and only someone holding the link can give it a new one
    let refused = handle(
        &mut project,
        &post(None, "/data/session/password", json!({ "lock": true })),
        NOW,
    );
    assert_eq!(refused.status, 401);
    assert_eq!(project.password_hash.as_deref(), Some(legacy.as_str()));
}

#[test]
fn reopening_and_bad_requests() {
    let mut project = Project::default();
    let token = lock(&mut project, None, json!({ "lock": true }));
    for body in [
        json!({}),
        json!({ "password": "" }),
        json!({ "lock": false }),
    ] {
        let response = handle(
            &mut project,
            &post(Some(&token), "/data/session/password", body),
            NOW,
        );
        assert_eq!(response.status, 400);
    }
    assert!(project.password_hash.is_some());
    let reopened = handle(
        &mut project,
        &post(
            Some(&token),
            "/data/session/password",
            json!({ "password": null }),
        ),
        NOW,
    );
    assert_eq!(reopened.status, 200);
    assert!(project.password_hash.is_none());
}

#[test]
fn share_links_carry_the_random_token() {
    let mut project = Project::default();
    let token = lock(&mut project, None, json!({ "lock": true }));
    let body = json!({ "baseUrl": "https://example.com", "path": "timer", "authenticate": true });
    let response = handle(
        &mut project,
        &post(Some(&token), "/data/session/url", body),
        NOW,
    );
    let url = response.body.unwrap()["url"].as_str().unwrap().to_string();
    assert!(url.contains(&token), "{url}");
}
