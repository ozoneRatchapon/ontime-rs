//! How often a room takes integration calls (plan 013).
//!
//! Upstream's API lives on a LAN, and this one is a public Worker whose calls
//! are GETs, so a stuck script or a leaked URL could otherwise hammer a room.
//! A token bucket per room: a burst is fine, and a steady flood is slowed to
//! the refill rate. Kept in memory by the host. Losing it on an eviction only
//! hands out a fresh burst, which is harmless.
//!
//! The policy lives here rather than in a host, so the Worker and the native
//! server cannot answer the same flood differently.

/// One bucket: `capacity` calls at once, refilled at `per_second`.
#[derive(Debug, Clone)]
pub struct Bucket {
    capacity: f64,
    per_ms: f64,
    tokens: f64,
    /// Epoch ms of the last refill; `None` until first used.
    at: Option<i64>,
}

impl Bucket {
    pub fn new(capacity: u32, per_second: u32) -> Self {
        Bucket {
            capacity: f64::from(capacity),
            per_ms: f64::from(per_second) / 1000.0,
            tokens: f64::from(capacity),
            at: None,
        }
    }

    /// Takes one call. `Err` holds how many milliseconds until one is free.
    pub fn take(&mut self, now: i64) -> Result<(), i64> {
        let elapsed = self.at.map_or(0, |at| (now - at).max(0));
        self.tokens = (self.tokens + elapsed as f64 * self.per_ms).min(self.capacity);
        self.at = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Ok(());
        }
        Err(((1.0 - self.tokens) / self.per_ms).ceil() as i64)
    }
}

/// A room's two buckets. Reading the state (`poll`, `version`) is what a
/// controller does in a loop, so it gets more room than driving the show.
#[derive(Debug, Clone)]
pub struct RateLimit {
    actions: Bucket,
    reads: Bucket,
}

/// Driving a show: a Stream Deck page can fire a few buttons at once, and no
/// operator presses 20 a second for long.
pub const ACTIONS_BURST: u32 = 40;
pub const ACTIONS_PER_SECOND: u32 = 20;
/// Polling: ten controllers polling ten times a second still fit.
pub const READS_BURST: u32 = 200;
pub const READS_PER_SECOND: u32 = 100;

impl Default for RateLimit {
    fn default() -> Self {
        RateLimit {
            actions: Bucket::new(ACTIONS_BURST, ACTIONS_PER_SECOND),
            reads: Bucket::new(READS_BURST, READS_PER_SECOND),
        }
    }
}

impl RateLimit {
    /// Takes one call for `tag`. `Err` holds the seconds to wait, for a
    /// `Retry-After` header, never less than one.
    pub fn take(&mut self, tag: &str, now: i64) -> Result<(), u64> {
        let bucket = match is_read(tag) {
            true => &mut self.reads,
            false => &mut self.actions,
        };
        bucket
            .take(now)
            .map_err(|ms| (ms.max(1) as u64).div_ceil(1000))
    }
}

/// Actions that only read. A members-only room lets anyone let in make these.
pub fn is_read(tag: &str) -> bool {
    matches!(tag.to_lowercase().as_str(), "version" | "poll")
}
