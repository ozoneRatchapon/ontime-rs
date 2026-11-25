//! The integration API's rate limit (plan 013).

use ontime_core::integration::{Bucket, RateLimit};

const T0: i64 = 1_700_000_000_000;

#[test]
fn a_burst_is_taken_and_then_refused() {
    let mut bucket = Bucket::new(3, 1);
    assert!(bucket.take(T0).is_ok());
    assert!(bucket.take(T0).is_ok());
    assert!(bucket.take(T0).is_ok());
    // one a second: the next is free in a second
    assert_eq!(bucket.take(T0), Err(1000));
}

#[test]
fn it_refills_at_the_rate() {
    let mut bucket = Bucket::new(2, 10);
    bucket.take(T0).unwrap();
    bucket.take(T0).unwrap();
    assert!(bucket.take(T0 + 50).is_err());
    // ten a second is one every 100ms
    assert!(bucket.take(T0 + 100).is_ok());
    assert!(bucket.take(T0 + 100).is_err());
}

#[test]
fn it_never_holds_more_than_its_burst() {
    let mut bucket = Bucket::new(2, 10);
    bucket.take(T0).unwrap();
    // an hour idle still refills to two, not to 36,000
    let later = T0 + 3_600_000;
    assert!(bucket.take(later).is_ok());
    assert!(bucket.take(later).is_ok());
    assert!(bucket.take(later).is_err());
}

#[test]
fn a_clock_going_back_takes_nothing_extra() {
    let mut bucket = Bucket::new(1, 1);
    bucket.take(T0).unwrap();
    assert!(bucket.take(T0 - 10_000).is_err());
}

#[test]
fn driving_and_reading_are_limited_apart() {
    let mut limit = RateLimit::default();
    // a flood of presses runs out the actions
    let mut refused = None;
    for n in 0..100 {
        if let Err(wait) = limit.take("start", T0) {
            refused = Some((n, wait));
            break;
        }
    }
    let (taken, wait) = refused.expect("a flood is refused");
    assert_eq!(taken, 40, "the burst is 40 actions");
    assert_eq!(wait, 1, "Retry-After is at least a second");
    // and polling still works, as a controller showing the state needs
    assert!(limit.take("poll", T0).is_ok());
    assert!(limit.take("VERSION", T0).is_ok());
}
