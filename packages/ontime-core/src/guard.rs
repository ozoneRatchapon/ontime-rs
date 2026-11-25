//! Slows down guessing a password or a token (plan 019).
//!
//! A room token is the password's digest, so a list of likely passwords can be
//! hashed offline and tried online against a room, the admin login or any
//! admin route. This counts the failed guesses per client and, past a free
//! allowance, makes each next guess wait twice as long as the last.
//!
//! A client is one or more keys ([`client_keys`]): an IPv4 address, or an IPv6
//! /64 and the /48 around it. The /48 has a larger allowance, so one site
//! cycling through its 65,536 /64s shares one count instead of getting 65,536.
//!
//! While any of a client's keys is locked, everything it presents is refused
//! before it is checked. Refusing only the wrong guesses would answer a right
//! one, and the lockout would hide nothing.
//!
//! The same wrong guess sent again is counted once, so screens left open
//! behind one venue's address after a password change, all retrying the old
//! token, do not lock the venue out.
//!
//! A success does not clear the count: a person typing the right password from
//! the same network as a guesser would otherwise hand the guesser a fresh
//! allowance. The count is forgotten after an hour without failures instead.
//!
//! Held in memory by the host, like the integration limit. An eviction hands
//! out a fresh allowance, and a guesser cannot cause one: its requests are
//! what keep the instance alive, and a full table drops unlocked clients first.

use std::collections::HashMap;
use std::net::IpAddr;

/// Typos, a stale link, a reconnecting socket: free.
pub const FREE_FAILURES: u32 = 10;
/// How many times more an IPv6 /48 may fail than one of its /64s.
pub const NETWORK_FACTOR: u32 = 16;
/// The first wait once the allowance is spent; it doubles from there.
pub const FIRST_LOCK_MS: i64 = 1_000;
/// The longest wait. Leaves a guesser about a hundred tries a day.
pub const MAX_LOCK_MS: i64 = 15 * 60_000;
/// A client quiet this long starts over.
pub const FORGET_AFTER_MS: i64 = 60 * 60_000;
/// Keys tracked at once, so a flood of addresses cannot grow the map without
/// bound.
pub const MAX_TRACKED: usize = 4_096;
/// Wrong guesses remembered per key. A screen left open after the password
/// changed retries one stale token, not nine.
pub const SEEN: usize = 8;

// a locked key is never forgotten while locked, which `make_room` relies on
const _: () = assert!(MAX_LOCK_MS < FORGET_AFTER_MS);

const UNKNOWN: &str = "unknown";
const NETWORK_SUFFIX: &str = "::/48";

/// The keys a client address is counted under: an IPv4 address alone, or an
/// IPv6 /64 (the block one subscriber is usually handed) and its /48.
pub fn client_keys(address: Option<&str>) -> Vec<String> {
    let Some(address) = address.map(str::trim).filter(|a| !a.is_empty()) else {
        return vec![UNKNOWN.to_string()];
    };
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => {
            let s = v6.segments();
            vec![
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3]),
                format!("{:x}:{:x}:{:x}{NETWORK_SUFFIX}", s[0], s[1], s[2]),
            ]
        }
        Ok(IpAddr::V4(v4)) => vec![v4.to_string()],
        Err(_) => vec![UNKNOWN.to_string()],
    }
}

/// The free failures for a key from [`client_keys`].
pub fn allowance(key: &str) -> u32 {
    match key.ends_with(NETWORK_SUFFIX) {
        true => FREE_FAILURES * NETWORK_FACTOR,
        false => FREE_FAILURES,
    }
}

/// A fingerprint of a guess, so the same wrong token sent again is recognised
/// without being kept.
pub fn fingerprint(attempt: &str) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ontime-guard-v1:");
    hasher.update(attempt.as_bytes());
    let digest = hasher.finalize();
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(first)
}

#[derive(Debug, Clone)]
struct Strikes {
    failures: u32,
    /// Epoch ms until which this key is refused; 0 when it is not.
    locked_until: i64,
    /// Epoch ms of the latest counted failure.
    last: i64,
    /// The latest distinct wrong guesses, newest last, at most [`SEEN`].
    seen: Vec<u64>,
}

/// Failed guesses per key.
#[derive(Debug, Default, Clone)]
pub struct Guard {
    strikes: HashMap<String, Strikes>,
}

impl Guard {
    /// `Err` holds the seconds until the client may try again, for
    /// `Retry-After`, never less than one: the longest wait of its keys.
    pub fn check(&self, keys: &[String], now: i64) -> Result<(), u64> {
        let until = keys
            .iter()
            .filter_map(|key| self.strikes.get(key))
            .map(|s| s.locked_until)
            .max()
            .unwrap_or(0);
        match until > now {
            true => Err(((until - now) as u64).div_ceil(1000).max(1)),
            false => Ok(()),
        }
    }

    /// Counts one wrong guess against each key, `guess` being its
    /// [`fingerprint`]. The same wrong guess again is not counted: a screen
    /// retrying a stale token is not guessing, and a guesser gains nothing by
    /// repeating itself.
    ///
    /// Only call it for a guess that was let through [`Guard::check`], or
    /// waiting out a lock would make it longer.
    pub fn fail(&mut self, keys: &[String], guess: u64, now: i64) {
        for key in keys {
            self.fail_key(key, guess, now);
        }
    }

    /// [`Guard::check`], then [`Guard::fail`] when the guess was wrong: for a
    /// host that knows the answer before it asks.
    pub fn attempt(&mut self, keys: &[String], ok: bool, guess: u64, now: i64) -> Result<(), u64> {
        self.check(keys, now)?;
        if !ok {
            self.fail(keys, guess, now);
        }
        Ok(())
    }

    /// The wrong guesses counted against `key` that are not yet forgotten.
    pub fn failures(&self, key: &str, now: i64) -> u32 {
        match self.strikes.get(key) {
            Some(s) if now - s.last < FORGET_AFTER_MS => s.failures,
            _ => 0,
        }
    }

    /// Keys being tracked.
    pub fn len(&self) -> usize {
        self.strikes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strikes.is_empty()
    }

    fn fail_key(&mut self, key: &str, guess: u64, now: i64) {
        self.make_room(key, now);
        let strikes = self.strikes.entry(key.to_string()).or_insert(Strikes {
            failures: 0,
            locked_until: 0,
            last: now,
            seen: Vec::new(),
        });
        if now - strikes.last >= FORGET_AFTER_MS {
            strikes.failures = 0;
            strikes.seen.clear();
        }
        if strikes.seen.contains(&guess) {
            return;
        }
        if strikes.seen.len() == SEEN {
            strikes.seen.remove(0);
        }
        strikes.seen.push(guess);
        strikes.failures = strikes.failures.saturating_add(1);
        strikes.last = now;
        if let Some(wait) = lock_ms(strikes.failures, allowance(key)) {
            strikes.locked_until = now + wait;
        }
    }

    /// Forgets the quiet keys; if that was not enough, drops the quietest
    /// unlocked one, so a flood of fresh addresses cannot push a lock out.
    fn make_room(&mut self, key: &str, now: i64) {
        if self.strikes.len() < MAX_TRACKED || self.strikes.contains_key(key) {
            return;
        }
        self.strikes.retain(|_, s| now - s.last < FORGET_AFTER_MS);
        if self.strikes.len() < MAX_TRACKED {
            return;
        }
        let dropped = self
            .strikes
            .iter()
            .min_by_key(|(_, s)| (s.locked_until > now, s.last))
            .map(|(k, _)| k.clone());
        if let Some(dropped) = dropped {
            self.strikes.remove(&dropped);
        }
    }
}

/// How long the `failures`-th wrong guess locks a key with `free` failures
/// out, if at all.
pub fn lock_ms(failures: u32, free: u32) -> Option<i64> {
    let over = failures.checked_sub(free)?.checked_sub(1)?;
    let wait = FIRST_LOCK_MS.saturating_mul(1_i64.checked_shl(over.min(32)).unwrap_or(i64::MAX));
    Some(wait.min(MAX_LOCK_MS))
}
