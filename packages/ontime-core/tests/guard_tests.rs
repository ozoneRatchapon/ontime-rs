//! The guessing guard (plan 019): a free allowance, then waits that double up
//! to a cap, refusal of everything while locked, and a bounded memory.

use ontime_core::api::is_known_token;
use ontime_core::guard::{
    client_key, lock_ms, Guard, FIRST_LOCK_MS, FORGET_AFTER_MS, FREE_FAILURES, MAX_LOCK_MS,
    MAX_TRACKED,
};
use ontime_core::project::{hash_password, Project};

const T0: i64 = 1_700_000_000_000;

fn spend_allowance(guard: &mut Guard, key: &str, now: i64) {
    for _ in 0..FREE_FAILURES {
        assert!(guard.attempt(key, false, now).is_ok());
    }
}

#[test]
fn the_allowance_is_free() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "1.2.3.4", T0);
    assert_eq!(guard.check("1.2.3.4", T0), Ok(()));
}

#[test]
fn the_guess_past_the_allowance_locks_for_a_second() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "1.2.3.4", T0);
    assert!(guard.attempt("1.2.3.4", false, T0).is_ok());
    assert_eq!(guard.check("1.2.3.4", T0), Err(1));
    assert_eq!(guard.check("1.2.3.4", T0 + FIRST_LOCK_MS), Ok(()));
}

#[test]
fn a_locked_client_is_refused_even_with_the_right_answer() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "1.2.3.4", T0);
    guard.fail("1.2.3.4", T0);
    assert_eq!(guard.attempt("1.2.3.4", true, T0 + 10), Err(1));
}

#[test]
fn waiting_out_a_lock_does_not_lengthen_it() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "k", T0);
    guard.fail("k", T0);
    // refused attempts while locked are not counted
    for ms in 0..50 {
        assert!(guard.attempt("k", false, T0 + ms).is_err());
    }
    assert_eq!(guard.check("k", T0 + FIRST_LOCK_MS), Ok(()));
    guard.fail("k", T0 + FIRST_LOCK_MS);
    // the 12th failure waits two seconds, not more
    assert_eq!(guard.check("k", T0 + FIRST_LOCK_MS), Err(2));
}

#[test]
fn waits_double_up_to_the_cap() {
    assert_eq!(lock_ms(FREE_FAILURES), None);
    assert_eq!(lock_ms(FREE_FAILURES + 1), Some(FIRST_LOCK_MS));
    assert_eq!(lock_ms(FREE_FAILURES + 2), Some(2 * FIRST_LOCK_MS));
    assert_eq!(lock_ms(FREE_FAILURES + 5), Some(16 * FIRST_LOCK_MS));
    assert_eq!(lock_ms(FREE_FAILURES + 30), Some(MAX_LOCK_MS));
    assert_eq!(lock_ms(u32::MAX), Some(MAX_LOCK_MS));
}

#[test]
fn a_success_does_not_hand_out_a_fresh_allowance() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "k", T0);
    assert!(guard.attempt("k", true, T0).is_ok());
    guard.fail("k", T0);
    assert!(guard.check("k", T0).is_err());
}

#[test]
fn an_hour_of_quiet_starts_over() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "k", T0);
    let later = T0 + FORGET_AFTER_MS;
    guard.fail("k", later);
    assert_eq!(guard.check("k", later), Ok(()));
}

#[test]
fn clients_are_counted_apart() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "a", T0);
    guard.fail("a", T0);
    assert!(guard.check("a", T0).is_err());
    assert_eq!(guard.check("b", T0), Ok(()));
}

#[test]
fn memory_stays_bounded_and_keeps_the_locked() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "locked", T0);
    guard.fail("locked", T0);
    for n in 0..(MAX_TRACKED * 2) {
        guard.fail(&format!("10.0.{}.{}", n / 256, n % 256), T0 + 1 + n as i64);
    }
    assert!(guard.len() <= MAX_TRACKED);
    // the quietest go first, and a lock is still young here, so a flood of
    // fresh addresses pushes it out only once it is the quietest of all
    assert!(guard
        .check("10.0.31.255", T0 + 2 * MAX_TRACKED as i64)
        .is_ok());
}

#[test]
fn ipv6_clients_are_keyed_by_their_64() {
    let a = client_key(Some("2001:db8:1:2:aaaa::1"));
    let b = client_key(Some("2001:db8:1:2:bbbb::9"));
    let c = client_key(Some("2001:db8:1:3::1"));
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(client_key(Some(" 1.2.3.4 ")), "1.2.3.4");
    assert_eq!(client_key(None), "unknown");
    assert_eq!(client_key(Some("not an address")), "unknown");
}

#[test]
fn known_tokens_are_the_room_and_master_tokens_only() {
    let mut project = Project::default();
    let master = hash_password("master");
    assert!(!is_known_token(&project, None, "anything"));
    assert!(is_known_token(&project, Some(&master), &master));
    project.password_hash = Some(hash_password("room"));
    assert!(is_known_token(&project, None, &hash_password("room")));
    assert!(!is_known_token(
        &project,
        Some(&master),
        &hash_password("guess")
    ));
}
