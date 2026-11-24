// Pre-authenticated share links. Ported from the original server's
// `generateShareUrl` tests so the URLs the client builds are byte-for-byte the
// same shape — a locked preset masks the path, `lockNav` adds `n=1`, and
// `authenticate` injects the room token.

use ontime_core::api::generate_share_url;
use serde_json::{json, Value};

fn url(body: Value, room_hash: Option<&str>) -> String {
    generate_share_url(&body, room_hash).unwrap()
}

#[test]
fn a_plain_link_has_no_lock_or_token() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://localhost:3000", "path": "timer" }),
            None
        ),
        "http://localhost:3000/timer"
    );
}

#[test]
fn lock_nav_adds_the_n_param() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "timer", "lockNav": true }),
            None
        ),
        "http://192.168.10.173:4001/timer?n=1"
    );
}

#[test]
fn authenticate_appends_the_room_token_after_nav() {
    // the token is the room's password hash, passed in here
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "timer", "lockNav": true, "authenticate": true }),
            Some("1234")
        ),
        "http://192.168.10.173:4001/timer?n=1&token=1234"
    );
}

#[test]
fn an_explicit_hash_overrides_the_room_token() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "timer", "authenticate": true, "hash": "abcd" }),
            Some("room-hash")
        ),
        "http://192.168.10.173:4001/timer?token=abcd"
    );
}

#[test]
fn an_unlocked_preset_replaces_the_path() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "timer", "preset": "minimal" }),
            None
        ),
        "http://192.168.10.173:4001/minimal"
    );
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "timer", "preset": "minimal", "lockNav": true }),
            None
        ),
        "http://192.168.10.173:4001/minimal?n=1"
    );
}

#[test]
fn a_locked_preset_is_masked() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "timer", "preset": "minimal", "lockConfig": true }),
            None
        ),
        "http://192.168.10.173:4001/preset/minimal"
    );
}

#[test]
fn the_cuesheet_preset_is_masked_even_unlocked() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "cuesheet", "preset": "sheet" }),
            None
        ),
        "http://192.168.10.173:4001/preset/sheet"
    );
}

#[test]
fn companion_links_point_at_the_root() {
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "<<companion>>" }),
            None
        ),
        "http://192.168.10.173:4001/"
    );
    assert_eq!(
        url(
            json!({ "baseUrl": "http://192.168.10.173:4001", "path": "<<companion>>", "authenticate": true }),
            Some("tok")
        ),
        "http://192.168.10.173:4001/?token=tok"
    );
}

#[test]
fn a_missing_base_url_is_refused() {
    assert!(generate_share_url(&json!({ "path": "timer" }), None).is_err());
}

// upstream 7de31da6: a base URL must be a full URL, eg: http://192.168.10.1:4001
#[test]
fn a_base_url_without_a_protocol_is_refused() {
    for base in [
        "192.168.10.1:4001",
        "localhost:4001",
        "host",
        "",
        "http://",
        "://host",
        "1http://host",
    ] {
        assert_eq!(
            generate_share_url(&json!({ "baseUrl": base, "path": "timer" }), None),
            Err("baseUrl must be a full URL, eg: http://192.168.10.1:4001"),
            "{base:?}"
        );
    }
}

#[test]
fn a_missing_path_is_refused() {
    assert!(generate_share_url(&json!({ "baseUrl": "http://host" }), None).is_err());
    assert!(generate_share_url(&json!({ "baseUrl": "http://host", "path": " " }), None).is_err());
}

#[test]
fn the_route_answers_422_for_invalid_input() {
    use ontime_core::api::{handle, ApiRequest, HttpMethod};
    let mut project = ontime_core::Project::default();
    let req = ApiRequest {
        method: HttpMethod::Post,
        path: "/data/session/url".into(),
        body: json!({ "baseUrl": "192.168.10.1:4001", "path": "timer" }),
        token: None,
        master_token: None,
        room_is_new: false,
        member: None,
    };
    assert_eq!(handle(&mut project, &req, 0).status, 422);
}

#[test]
fn only_the_origin_of_the_base_url_is_used() {
    // defensive: a baseUrl that carries a path keeps only scheme+host+port
    assert_eq!(
        url(
            json!({ "baseUrl": "http://host:4001/editor", "path": "timer" }),
            None
        ),
        "http://host:4001/timer"
    );
}
