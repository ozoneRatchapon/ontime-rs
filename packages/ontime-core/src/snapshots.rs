//! The room's own undo history (plan 018): copies of the stored project kept
//! in the room's own storage, so a mistake can be undone even when no
//! off-site backup ran. Free: the copies live in the room's own storage.
//!
//! The policy is here, and the host does the storage, so every host keeps the
//! same history.

/// At most one copy every ten minutes for ordinary edits.
pub const EVERY_MS: i64 = 600_000;
/// About twelve hours of ten-minute copies. At ~70 KB for a large room that
/// is about 5 MB, well inside the free 5 GB.
pub const KEEP: usize = 72;
/// Where the list of copies is kept.
pub const INDEX_KEY: &str = "snapshot_index";

pub fn key(at: i64) -> String {
    format!("snapshot:{at}")
}

/// Why a copy is being considered, just before a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// An ordinary edit: one copy every ten minutes is enough.
    Edit,
    /// A change that removes things (a delete, a whole rundown replaced, a
    /// backup restored): copied whenever anything changed since the last copy,
    /// so exactly that change can be undone.
    Destructive,
}

/// What to store and what to drop, when a copy is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take {
    pub key: String,
    pub drop: Vec<String>,
}

/// Records a copy at `now` in `index` (oldest first) when one is due, and
/// keeps the newest `KEEP`. `changed_since_copy` says whether anything was
/// written since the newest copy; a copy of the same state is never taken.
pub fn take(
    index: &mut Vec<i64>,
    now: i64,
    reason: Reason,
    changed_since_copy: bool,
) -> Option<Take> {
    if !changed_since_copy || index.last() == Some(&now) {
        return None;
    }
    let due = match reason {
        Reason::Destructive => true,
        Reason::Edit => index.last().is_none_or(|last| now - last >= EVERY_MS),
    };
    if !due {
        return None;
    }
    index.push(now);
    let excess = index.len().saturating_sub(KEEP);
    let drop = index.drain(..excess).map(key).collect();
    Some(Take {
        key: key(now),
        drop,
    })
}
