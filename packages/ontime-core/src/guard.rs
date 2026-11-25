//! Slows down guessing a password or a token (plan 019).
//!
//! A room token is the password's digest, so a list of likely passwords can be
//! hashed offline and tried online against a room, the admin login or any
//! admin route. This counts the failed guesses per client and, past a free
//! allowance, makes each next guess wait twice as long as the last.
//!
//! While a client is locked out, everything it presents is refused before it is
//! checked. Refusing only the wrong guesses would answer a right one, and the
//! lockout would hide nothing.
//!
//! A success does not clear the count: a person typing the right password from
//! the same network as a guesser would otherwise hand the guesser a fresh
//! allowance. The count is forgotten after an hour without failures instead.
//!
//! Held in memory by the host, like the integration limit. An eviction hands
//! out a fresh allowance, and a guesser cannot cause one: its requests are
//! what keep the instance alive.

use std::collections::HashMap;
use std::net::IpAddr;

/// Typos, a stale link, a reconnecting socket: free.
pub const FREE_FAILURES: u32 = 10;
/// The first wait once the allowance is spent; it doubles from there.
pub const FIRST_LOCK_MS: i64 = 1_000;
/// The longest wait. Leaves a guesser about a hundred tries a day.
pub const MAX_LOCK_MS: i64 = 15 * 60_000;
/// A client quiet this long starts over.
pub const FORGET_AFTER_MS: i64 = 60 * 60_000;
/// Clients tracked at once, so a flood of addresses cannot grow the map
/// without bound. The quietest is dropped first.
pub const MAX_TRACKED: usize = 4_096;

/// The key for a client address. An IPv6 client is keyed by its /64, the block
/// one subscriber is usually handed, so cycling through it does not help.
pub fn client_key(address: Option<&str>) -> String {
    let Some(address) = address.map(str::trim).filter(|a| !a.is_empty()) else {
        return "unknown".to_string();
    };
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        Ok(IpAddr::V4(v4)) => v4.to_string(),
        Err(_) => "unknown".to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
struct Strikes {
    failures: u32,
    /// Epoch ms until which this client is refused; 0 when it is not.
    locked_until: i64,
    /// Epoch ms of the latest failure.
    last: i64,
}

/// Failed guesses per client.
#[derive(Debug, Default, Clone)]
pub struct Guard {
    strikes: HashMap<String, Strikes>,
}

impl Guard {
    /// `Err` holds the seconds until `key` may try again, for `Retry-After`,
    /// never less than one.
    pub fn check(&self, key: &str, now: i64) -> Result<(), u64> {
        let Some(strikes) = self.strikes.get(key) else {
            return Ok(());
        };
        match strikes.locked_until > now {
            true => Err(((strikes.locked_until - now) as u64).div_ceil(1000).max(1)),
            false => Ok(()),
        }
    }

    /// Counts one wrong guess. Only call it for a guess that was let through
    /// [`Guard::check`], or waiting out a lock would make it longer.
    pub fn fail(&mut self, key: &str, now: i64) {
        self.make_room(key, now);
        let strikes = self.strikes.entry(key.to_string()).or_insert(Strikes {
            failures: 0,
            locked_until: 0,
            last: now,
        });
        if now - strikes.last >= FORGET_AFTER_MS {
            strikes.failures = 0;
        }
        strikes.failures = strikes.failures.saturating_add(1);
        strikes.last = now;
        if let Some(wait) = lock_ms(strikes.failures) {
            strikes.locked_until = now + wait;
        }
    }

    /// [`Guard::check`], then [`Guard::fail`] when the guess was wrong: for a
    /// host that knows the answer before it asks.
    pub fn attempt(&mut self, key: &str, ok: bool, now: i64) -> Result<(), u64> {
        self.check(key, now)?;
        if !ok {
            self.fail(key, now);
        }
        Ok(())
    }

    /// Clients being tracked.
    pub fn len(&self) -> usize {
        self.strikes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strikes.is_empty()
    }

    /// Forgets the quiet clients, and the quietest one if that was not enough.
    fn make_room(&mut self, key: &str, now: i64) {
        if self.strikes.len() < MAX_TRACKED || self.strikes.contains_key(key) {
            return;
        }
        self.strikes
            .retain(|_, s| now - s.last < FORGET_AFTER_MS || s.locked_until > now);
        if self.strikes.len() < MAX_TRACKED {
            return;
        }
        let quietest = self
            .strikes
            .iter()
            .min_by_key(|(_, s)| s.last)
            .map(|(k, _)| k.clone());
        if let Some(quietest) = quietest {
            self.strikes.remove(&quietest);
        }
    }
}

/// How long the `failures`-th wrong guess locks a client out, if at all.
pub fn lock_ms(failures: u32) -> Option<i64> {
    let over = failures.checked_sub(FREE_FAILURES)?.checked_sub(1)?;
    let wait = FIRST_LOCK_MS.saturating_mul(1_i64.checked_shl(over.min(32)).unwrap_or(i64::MAX));
    Some(wait.min(MAX_LOCK_MS))
}
