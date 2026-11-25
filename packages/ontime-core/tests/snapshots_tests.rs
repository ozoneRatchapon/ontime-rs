//! The room's undo history policy (plan 018).

use ontime_core::snapshots::{key, take, Reason, EVERY_MS, KEEP};

const T0: i64 = 1_700_000_000_000;

#[test]
fn the_first_edit_takes_a_copy() {
    let mut index = Vec::new();
    let taken = take(&mut index, T0, Reason::Edit, true).unwrap();
    assert_eq!(taken.key, key(T0));
    assert!(taken.drop.is_empty());
    assert_eq!(index, vec![T0]);
}

#[test]
fn ordinary_edits_take_one_every_ten_minutes() {
    let mut index = vec![T0];
    assert!(take(&mut index, T0 + EVERY_MS - 1, Reason::Edit, true).is_none());
    assert!(take(&mut index, T0 + EVERY_MS, Reason::Edit, true).is_some());
    assert_eq!(index.len(), 2);
}

#[test]
fn a_destructive_change_is_always_copied_first() {
    let mut index = vec![T0];
    assert!(take(&mut index, T0 + 1_000, Reason::Destructive, true).is_some());
}

#[test]
fn the_same_state_is_never_copied_twice() {
    let mut index = vec![T0];
    assert!(take(&mut index, T0 + EVERY_MS * 5, Reason::Edit, false).is_none());
    assert!(take(&mut index, T0 + 1, Reason::Destructive, false).is_none());
}

#[test]
fn only_the_newest_are_kept() {
    let mut index: Vec<i64> = (0..KEEP as i64).map(|n| T0 + n * EVERY_MS).collect();
    let taken = take(&mut index, T0 + KEEP as i64 * EVERY_MS, Reason::Edit, true).unwrap();
    assert_eq!(taken.drop, vec![key(T0)]);
    assert_eq!(index.len(), KEEP);
    assert_eq!(index[0], T0 + EVERY_MS);
}
