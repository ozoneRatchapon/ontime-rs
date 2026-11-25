//! Custom views (plan 021): the names and pages upstream's customViews.service
//! accepts, run through upstream by scripts/upstream-oracle/gen-custom_views.ts,
//! and how a page is served here.

use ontime_core::custom_views::{
    content_security_policy, is_valid_slug, serve, validate, ViewError, DEMO_HTML, MAX_BYTES,
};
use serde_json::Value;

fn fixture() -> Value {
    let text = include_str!("fixtures/custom_views.json");
    serde_json::from_str::<Value>(text).unwrap()["cases"].clone()
}

#[test]
fn names_are_taken_as_upstream_takes_them() {
    for case in fixture()["slugs"].as_array().unwrap() {
        let slug = case["slug"].as_str().unwrap();
        assert_eq!(
            is_valid_slug(slug),
            case["valid"].as_bool().unwrap(),
            "{slug:?}"
        );
    }
}

#[test]
fn pages_are_refused_as_upstream_refuses_them() {
    for case in fixture()["pages"].as_array().unwrap() {
        let html = case["html"].as_str().unwrap();
        let ours = validate(html).err().map(ViewError::message);
        let theirs = case["refused"].as_str().map(str::to_string);
        assert_eq!(ours, theirs, "{}", case["name"]);
    }
}

#[test]
fn the_demo_page_is_upstreams_and_is_accepted() {
    assert!(DEMO_HTML.starts_with("<!doctype html>"));
    assert!(DEMO_HTML.contains("/ws"));
    assert_eq!(validate(DEMO_HTML), Ok(()));
}

#[test]
fn size_limits() {
    assert_eq!(validate(""), Err(ViewError::Empty));
    let big = format!("<html>{}</html>", "x".repeat(MAX_BYTES));
    assert_eq!(validate(&big), Err(ViewError::TooLarge));
    assert_eq!(ViewError::TooLarge.status(), 413);
}

#[test]
fn the_room_script_runs_before_the_page() {
    let served =
        serve("<!doctype html><html><HEAD lang=\"x\"><script>own()</script></head></html>");
    let shim = served.find("location.pathname.match").unwrap();
    let own = served.find("own()").unwrap();
    assert!(served.starts_with("<!doctype html><html><HEAD lang=\"x\"><script>"));
    assert!(shim < own);
    // a page with no head still gets it first
    assert!(serve("<html><body></body></html>").starts_with("<script>"));
    // a <header> is not the head
    assert!(serve("<html><body><header>x</header></body></html>").starts_with("<script>"));
}

#[test]
fn a_page_is_sandboxed_and_talks_only_to_its_worker() {
    let policy = content_security_policy("https://w.example");
    assert!(policy.starts_with("sandbox allow-scripts"));
    assert!(!policy.contains("allow-same-origin"));
    assert!(policy.contains("connect-src https://w.example wss://w.example"));
    assert!(policy.contains("default-src 'none'"));
    let local = content_security_policy("http://127.0.0.1:8787");
    assert!(local.contains("ws://127.0.0.1:8787"));
}
