//! The guessing guard (plan 019): a free allowance, then waits that double up
//! to a cap, refusal of everything while locked, a repeated wrong token counted
//! once, and a bounded memory.

use ontime_core::api::is_known_token;
use ontime_core::guard::{
    allowance, client_keys, fingerprint, lock_ms, Guard, FIRST_LOCK_MS, FORGET_AFTER_MS,
    FREE_FAILURES, MAX_LOCK_MS, MAX_TRACKED, NETWORK_FACTOR, SEEN,
};
use ontime_core::project::{hash_password, Project};

const T0: i64 = 1_700_000_000_000;

/// The n-th distinct wrong guess.
fn guess(n: u32) -> u64 {
    fingerprint(&format!("guess-{n}"))
}

fn k(key: &str) -> Vec<String> {
    vec![key.to_string()]
}

fn spend_allowance(guard: &mut Guard, key: &str, now: i64) {
    for n in 0..FREE_FAILURES {
        assert!(guard.attempt(&k(key), false, guess(n), now).is_ok());
    }
}

/// Locks `key` with the first failure past the allowance.
fn lock(guard: &mut Guard, key: &str, now: i64) {
    spend_allowance(guard, key, now);
    guard.fail(&k(key), guess(FREE_FAILURES), now);
}

#[test]
fn the_policy_numbers() {
    assert_eq!(FREE_FAILURES, 10);
    assert_eq!(FIRST_LOCK_MS, 1_000);
    assert_eq!(MAX_LOCK_MS, 900_000);
    assert_eq!(FORGET_AFTER_MS, 3_600_000);
    assert_eq!(MAX_TRACKED, 4_096);
    assert_eq!(SEEN, 8);
}

#[test]
fn the_allowance_is_free() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "1.2.3.4", T0);
    assert_eq!(guard.check(&k("1.2.3.4"), T0), Ok(()));
    assert_eq!(guard.failures("1.2.3.4", T0), FREE_FAILURES);
}

#[test]
fn the_guess_past_the_allowance_locks_for_a_second() {
    let mut guard = Guard::default();
    lock(&mut guard, "1.2.3.4", T0);
    assert_eq!(guard.check(&k("1.2.3.4"), T0), Err(1));
    assert_eq!(guard.check(&k("1.2.3.4"), T0 + FIRST_LOCK_MS - 1), Err(1));
    assert_eq!(guard.check(&k("1.2.3.4"), T0 + FIRST_LOCK_MS), Ok(()));
}

#[test]
fn a_locked_client_is_refused_even_with_the_right_answer() {
    let mut guard = Guard::default();
    lock(&mut guard, "1.2.3.4", T0);
    assert_eq!(guard.attempt(&k("1.2.3.4"), true, 0, T0 + 10), Err(1));
}

#[test]
fn waiting_out_a_lock_does_not_lengthen_it() {
    let mut guard = Guard::default();
    lock(&mut guard, "k", T0);
    // refused attempts while locked are not counted
    for ms in 0..50 {
        assert!(guard
            .attempt(&k("k"), false, guess(100 + ms), T0 + ms as i64)
            .is_err());
    }
    assert_eq!(guard.failures("k", T0), FREE_FAILURES + 1);
    assert_eq!(guard.check(&k("k"), T0 + FIRST_LOCK_MS), Ok(()));
    guard.fail(&k("k"), guess(999), T0 + FIRST_LOCK_MS);
    // the 12th failure waits two seconds, not more
    assert_eq!(guard.check(&k("k"), T0 + FIRST_LOCK_MS), Err(2));
}

#[test]
fn retry_after_rounds_up_to_whole_seconds() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "k", T0);
    for n in 0..5 {
        guard.fail(&k("k"), guess(50 + n), T0);
    }
    // the 15th failure waits 16 s
    assert_eq!(guard.check(&k("k"), T0), Err(16));
    assert_eq!(guard.check(&k("k"), T0 + 1), Err(16));
    assert_eq!(guard.check(&k("k"), T0 + 15_001), Err(1));
}

#[test]
fn waits_double_up_to_the_cap() {
    assert_eq!(lock_ms(0, FREE_FAILURES), None);
    assert_eq!(lock_ms(FREE_FAILURES, FREE_FAILURES), None);
    assert_eq!(
        lock_ms(FREE_FAILURES + 1, FREE_FAILURES),
        Some(FIRST_LOCK_MS)
    );
    assert_eq!(
        lock_ms(FREE_FAILURES + 2, FREE_FAILURES),
        Some(2 * FIRST_LOCK_MS)
    );
    assert_eq!(
        lock_ms(FREE_FAILURES + 5, FREE_FAILURES),
        Some(16 * FIRST_LOCK_MS)
    );
    assert_eq!(
        lock_ms(FREE_FAILURES + 10, FREE_FAILURES),
        Some(512 * FIRST_LOCK_MS)
    );
    assert_eq!(
        lock_ms(FREE_FAILURES + 11, FREE_FAILURES),
        Some(MAX_LOCK_MS)
    );
    assert_eq!(
        lock_ms(FREE_FAILURES + 40, FREE_FAILURES),
        Some(MAX_LOCK_MS)
    );
    assert_eq!(lock_ms(u32::MAX, FREE_FAILURES), Some(MAX_LOCK_MS));
}

#[test]
fn the_same_wrong_token_again_is_counted_once() {
    let mut guard = Guard::default();
    let stale = fingerprint("yesterday's token");
    for _ in 0..(10 * FREE_FAILURES) {
        assert!(guard.attempt(&k("venue"), false, stale, T0).is_ok());
    }
    assert_eq!(guard.failures("venue", T0), 1);
}

#[test]
fn a_guess_seen_long_enough_ago_counts_again() {
    let mut guard = Guard::default();
    guard.fail(&k("k"), guess(0), T0);
    for n in 1..=SEEN as u32 {
        guard.fail(&k("k"), guess(n), T0);
    }
    // guess 0 has been pushed out of the remembered ones
    guard.fail(&k("k"), guess(0), T0);
    assert_eq!(guard.failures("k", T0), SEEN as u32 + 2);
    // and the newest are remembered
    guard.fail(&k("k"), guess(SEEN as u32), T0);
    assert_eq!(guard.failures("k", T0), SEEN as u32 + 2);
}

#[test]
fn a_success_does_not_hand_out_a_fresh_allowance() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "k", T0);
    assert!(guard.attempt(&k("k"), true, 0, T0).is_ok());
    assert_eq!(guard.failures("k", T0), FREE_FAILURES);
    guard.fail(&k("k"), guess(FREE_FAILURES), T0);
    assert!(guard.check(&k("k"), T0).is_err());
}

#[test]
fn an_hour_of_quiet_starts_over() {
    let mut guard = Guard::default();
    spend_allowance(&mut guard, "k", T0);
    assert_eq!(guard.failures("k", T0 + FORGET_AFTER_MS - 1), FREE_FAILURES);
    assert_eq!(guard.failures("k", T0 + FORGET_AFTER_MS), 0);
    let later = T0 + FORGET_AFTER_MS;
    // the remembered guesses are forgotten too
    guard.fail(&k("k"), guess(0), later);
    assert_eq!(guard.failures("k", later), 1);
    assert_eq!(guard.check(&k("k"), later), Ok(()));
}

#[test]
fn clients_are_counted_apart() {
    let mut guard = Guard::default();
    lock(&mut guard, "a", T0);
    assert!(guard.check(&k("a"), T0).is_err());
    assert_eq!(guard.check(&k("b"), T0), Ok(()));
    assert_eq!(guard.failures("b", T0), 0);
}

fn address(n: usize) -> String {
    format!("10.{}.{}.{}", n / 65_536, (n / 256) % 256, n % 256)
}

#[test]
fn memory_is_capped_at_max_tracked() {
    let mut guard = Guard::default();
    assert!(guard.is_empty());
    for n in 0..MAX_TRACKED {
        guard.fail(&k(&address(n)), 1, T0 + n as i64);
    }
    assert_eq!(guard.len(), MAX_TRACKED);
    assert!(!guard.is_empty());
    guard.fail(&k("one more"), 1, T0 + MAX_TRACKED as i64);
    assert_eq!(guard.len(), MAX_TRACKED);
    // the quietest went, the newest stayed
    assert_eq!(guard.failures(&address(0), T0 + MAX_TRACKED as i64), 0);
    assert_eq!(guard.failures(&address(1), T0 + MAX_TRACKED as i64), 1);
    assert_eq!(guard.failures("one more", T0 + MAX_TRACKED as i64), 1);
}

#[test]
fn a_known_client_is_counted_without_dropping_anyone() {
    let mut guard = Guard::default();
    for n in 0..MAX_TRACKED {
        guard.fail(&k(&address(n)), 1, T0);
    }
    guard.fail(&k(&address(0)), 2, T0);
    assert_eq!(guard.len(), MAX_TRACKED);
    assert_eq!(guard.failures(&address(0), T0), 2);
    assert_eq!(guard.failures(&address(MAX_TRACKED - 1), T0), 1);
}

#[test]
fn forgotten_clients_make_room_before_anyone_recent() {
    let mut guard = Guard::default();
    for n in 0..MAX_TRACKED - 1 {
        guard.fail(&k(&address(n)), 1, T0);
    }
    let recent = T0 + FORGET_AFTER_MS - 1;
    guard.fail(&k("recent"), 1, recent);
    let now = T0 + FORGET_AFTER_MS;
    guard.fail(&k("new"), 1, now);
    // every one of the old ones was forgotten at once, not just the quietest
    assert_eq!(guard.len(), 2);
    assert_eq!(guard.failures("recent", now), 1);
}

#[test]
fn a_locked_client_survives_a_flood_of_new_addresses() {
    let mut guard = Guard::default();
    let start = T0 + FORGET_AFTER_MS;
    for n in 0..MAX_TRACKED - 1 {
        guard.fail(&k(&address(n)), 1, T0);
    }
    lock(&mut guard, "guesser", start);
    for n in 0..MAX_TRACKED {
        guard.fail(&k(&format!("flood-{n}")), 1, start + 1);
    }
    assert!(guard.check(&k("guesser"), start + 1).is_err());
}

#[test]
fn ipv6_clients_are_keyed_by_their_64_and_their_48() {
    let a = client_keys(Some("2001:db8:1:2:aaaa::1"));
    let b = client_keys(Some("2001:db8:1:2:bbbb::9"));
    let c = client_keys(Some("2001:db8:1:3::1"));
    assert_eq!(a, ["2001:db8:1:2::/64", "2001:db8:1::/48"]);
    assert_eq!(a, b);
    assert_ne!(a[0], c[0]);
    assert_eq!(a[1], c[1]);
    assert_eq!(client_keys(Some(" 1.2.3.4 ")), ["1.2.3.4"]);
    assert_eq!(client_keys(None), ["unknown"]);
    assert_eq!(client_keys(Some("  ")), ["unknown"]);
    assert_eq!(client_keys(Some("not an address")), ["unknown"]);
}

#[test]
fn a_48_may_fail_more_than_a_64() {
    assert_eq!(NETWORK_FACTOR, 16);
    assert_eq!(allowance("2001:db8:1::/48"), FREE_FAILURES * NETWORK_FACTOR);
    assert_eq!(allowance("2001:db8:1:2::/64"), FREE_FAILURES);
    assert_eq!(allowance("1.2.3.4"), FREE_FAILURES);
    assert_eq!(allowance("unknown"), FREE_FAILURES);
    assert_eq!(
        lock_ms(FREE_FAILURES + 1, FREE_FAILURES * NETWORK_FACTOR),
        None
    );
}

#[test]
fn cycling_through_64s_runs_into_the_48() {
    let mut guard = Guard::default();
    let free = FREE_FAILURES * NETWORK_FACTOR;
    let mut n = 0;
    // each /64 stays inside its own allowance, so only the /48 can stop this
    'outer: for block in 0..u16::MAX {
        let keys = client_keys(Some(&format!("2001:db8:1:{block:x}::1")));
        for _ in 0..FREE_FAILURES {
            if guard.attempt(&keys, false, guess(n), T0).is_err() {
                break 'outer;
            }
            n += 1;
        }
    }
    assert_eq!(n, free + 1);
    let elsewhere = client_keys(Some("2001:db8:2::1"));
    assert_eq!(guard.check(&elsewhere, T0), Ok(()));
}

#[test]
fn the_longest_wait_of_a_clients_keys_wins() {
    let mut guard = Guard::default();
    for n in 0..FREE_FAILURES + 3 {
        guard.fail(&k("long"), guess(n), T0);
    }
    lock(&mut guard, "short", T0);
    let both = vec!["short".to_string(), "long".to_string()];
    assert_eq!(guard.check(&both, T0), Err(4));
    assert_eq!(guard.check(&[], T0), Ok(()));
}

#[test]
fn fingerprints_tell_guesses_apart() {
    assert_eq!(fingerprint("a"), fingerprint("a"));
    assert_ne!(fingerprint("a"), fingerprint("b"));
    assert_ne!(fingerprint(""), 0);
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

#[test]
fn nothing_is_swept_until_the_table_is_full() {
    let mut guard = Guard::default();
    guard.fail(&k("old"), 1, T0);
    guard.fail(&k("new"), 1, T0 + FORGET_AFTER_MS);
    // forgotten, but only swept when room is needed: no scan per failure
    assert_eq!(guard.len(), 2);
    assert_eq!(guard.failures("old", T0 + FORGET_AFTER_MS), 0);
}

#[test]
fn a_lock_that_ends_now_no_longer_protects_from_eviction() {
    let mut guard = Guard::default();
    lock(&mut guard, "was locked", T0);
    for n in 0..MAX_TRACKED - 1 {
        guard.fail(&k(&address(n)), 1, T0 + 1);
    }
    let ended = T0 + FIRST_LOCK_MS;
    guard.fail(&k("new"), 1, ended);
    assert_eq!(guard.failures("was locked", ended), 0);
    assert_eq!(guard.failures(&address(0), ended), 1);
}
