//! The audit log: who changed what, and when (plan 007).
//!
//! One line per change. A line keeps **who** ([`Actor`]), **what** ([`Action`]
//! and the [`Subject`] it touched), and for an edit the **names** of the
//! fields that changed, never their values. Notes can hold people's names, so
//! the log keeps none of them.
//!
//! Lines are kept in chunks of [`Retention::chunk_lines`]. A change rewrites
//! only the newest chunk instead of the whole log. The [`AuditIndex`] records
//! which chunks exist. It lives in the document the host already persists with
//! each change, so a change and its line are written together. The host stores
//! each chunk under [`chunk_key`]. Everything here is pure, and the host only
//! loads and stores what it is told to.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::rundown::MS_PER_DAY;

/// Who made a change: whatever let the request in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Actor {
    /// A signed-in person. The name is kept as it was then, so renaming or
    /// removing them later does not rewrite history.
    Person { id: Uuid, name: String },
    RoomPassword,
    /// The master password.
    Admin,
    /// Nothing was needed: an open room.
    Anyone,
}

/// What happened. Grouped by [`Area`] for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    // playback
    Start,
    Load,
    Pause,
    Stop,
    Reload,
    AddTime,
    Roll,
    // entries in a rundown
    EntryAdd,
    EntryEdit,
    EntryDelete,
    EntryDeleteAll,
    EntryMove,
    EntrySwap,
    EntryRenumber,
    EntryClone,
    EntryGroup,
    EntryUngroup,
    DelayApply,
    // the list of rundowns
    RundownCreate,
    RundownImport,
    RundownRename,
    RundownReplace,
    RundownDelete,
    RundownLoad,
    RundownDuplicate,
    // the room itself
    ProjectData,
    Settings,
    ViewSettings,
    CustomFields,
    UrlPresets,
    Css,
    PasswordSet,
    PasswordCleared,
    MembersOnlyOn,
    MembersOnlyOff,
    Restore,
    // the report
    ReportArchive,
    ReportClear,
    // people, kept by the registry
    PersonCreate,
    PersonRemove,
    InviteIssue,
    InviteRedeem,
    SignOut,
    /// The admin set a room's owners or operators.
    MembersChange,
    /// An owner added or removed an operator of their room.
    OperatorAdd,
    OperatorRemove,
    // the shared teleprompter
    PrompterPlay,
    PrompterPause,
    /// next, previous, top, goto or a scroll
    PrompterMove,
    PrompterSpeed,
    PrompterMode,
    PrompterBlackout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Area {
    Playback,
    Rundown,
    RundownList,
    Room,
    Report,
    People,
    Teleprompter,
}

impl Action {
    pub fn area(self) -> Area {
        use Action::*;
        match self {
            Start | Load | Pause | Stop | Reload | AddTime | Roll => Area::Playback,
            EntryAdd | EntryEdit | EntryDelete | EntryDeleteAll | EntryMove | EntrySwap
            | EntryRenumber | EntryClone | EntryGroup | EntryUngroup | DelayApply => Area::Rundown,
            RundownCreate | RundownImport | RundownRename | RundownReplace | RundownDelete
            | RundownLoad | RundownDuplicate => Area::RundownList,
            ProjectData | Settings | ViewSettings | CustomFields | UrlPresets | Css | PasswordSet
            | PasswordCleared | MembersOnlyOn | MembersOnlyOff | Restore => Area::Room,
            ReportArchive | ReportClear => Area::Report,
            PersonCreate | PersonRemove | InviteIssue | InviteRedeem | SignOut | MembersChange
            | OperatorAdd | OperatorRemove => Area::People,
            PrompterPlay | PrompterPause | PrompterMove | PrompterSpeed | PrompterMode | PrompterBlackout => {
                Area::Teleprompter
            }
        }
    }
}

/// The thing a change touched: an event (cue and title), a rundown (title), a
/// room or a person (name).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cue: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}

impl Subject {
    pub fn titled(title: impl Into<String>) -> Self {
        Self { cue: String::new(), title: title.into() }
    }
}

/// A change as described before it is attributed and timed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Subject>,
    /// For an edit, the names of the fields that changed. Never their values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    /// How many more entries than the subject the change touched.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more: u32,
    /// Milliseconds added (negative: removed), for [`Action::AddTime`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    /// The room a registry line is about. A room's own lines leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
}

impl Note {
    pub fn new(action: Action) -> Self {
        Self { action, subject: None, fields: Vec::new(), more: 0, amount: None, room: None }
    }

    pub fn about(mut self, subject: Option<Subject>) -> Self {
        self.subject = subject;
        self
    }

    pub fn with_fields(mut self, fields: Vec<String>) -> Self {
        self.fields = fields;
        self
    }

    pub fn with_more(mut self, more: usize) -> Self {
        self.more = u32::try_from(more).unwrap_or(u32::MAX);
        self
    }

    pub fn with_amount(mut self, amount: i64) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn in_room(mut self, room: &str) -> Self {
        self.room = Some(room.to_string());
        self
    }
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn is_one(value: &u32) -> bool {
    *value == 1
}

fn one() -> u32 {
    1
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLine {
    /// Epoch milliseconds of the latest occurrence.
    pub at: i64,
    pub actor: Actor,
    #[serde(flatten)]
    pub note: Note,
    /// Identical playback commands close together are merged into one line.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub times: u32,
}

/// How much is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// The log keeps at most this many lines.
    pub max_lines: usize,
    pub max_age_ms: i64,
    pub chunk_lines: usize,
}

impl Retention {
    pub const ROOM: Retention = Retention { max_lines: 2_000, max_age_ms: 90 * MS_PER_DAY, chunk_lines: 100 };
    pub const REGISTRY: Retention =
        Retention { max_lines: 2_000, max_age_ms: 365 * MS_PER_DAY, chunk_lines: 100 };

    fn max_chunks(&self) -> usize {
        (self.max_lines / self.chunk_lines).max(1)
    }
}

/// Repeats of one playback command within this long merge into one line.
pub const MERGE_WINDOW_MS: i64 = 2_000;

/// Most lines one page may ask for.
pub const MAX_PAGE: usize = 200;

/// Where a host without a project document keeps its [`AuditIndex`].
pub const INDEX_KEY: &str = "audit_index_v1";

/// The storage key of chunk `no`. Zero-padded so keys also sort in order.
pub fn chunk_key(no: u64) -> String {
    format!("audit_v1:{no:010}")
}

/// What is known about one stored chunk without loading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub no: u64,
    /// When its newest line happened.
    pub newest: i64,
    pub len: usize,
}

/// One chunk of lines, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub no: u64,
    pub lines: Vec<AuditLine>,
}

/// Which chunks exist, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditIndex {
    pub chunks: Vec<ChunkMeta>,
}

/// What the host has to store after [`AuditIndex::record`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writes {
    /// The chunk to write. It is always the head.
    pub put: Chunk,
    /// Chunks to delete.
    pub delete: Vec<u64>,
}

impl AuditIndex {
    /// The number of the newest chunk, which [`record`](Self::record) needs
    /// loaded.
    pub fn head(&self) -> Option<u64> {
        self.chunks.last().map(|meta| meta.no)
    }

    pub fn len(&self) -> usize {
        self.chunks.iter().map(|meta| meta.len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Chunk numbers, newest first, skipping chunks whose lines are all at or
    /// after `before`. Each chunk only holds lines older than the next one's.
    pub fn newest_first(&self, before: Option<i64>) -> impl Iterator<Item = u64> + '_ {
        let before = before.unwrap_or(i64::MAX);
        let skip = self
            .chunks
            .iter()
            .rev()
            .zip(self.chunks.iter().rev().skip(1))
            .take_while(|(_, older)| older.newest >= before)
            .count();
        self.chunks.iter().rev().skip(skip).map(|meta| meta.no)
    }

    /// Adds a line, merging it into the previous one when it repeats a playback
    /// command, and drops whatever is now too old or over the limit.
    ///
    /// `head` must be the stored chunk numbered [`head`](Self::head), or `None`
    /// when there is none yet.
    pub fn record(
        &mut self,
        head: Option<Chunk>,
        actor: Actor,
        note: Note,
        now: i64,
        retention: Retention,
    ) -> Writes {
        // a head that could not be loaded is not guessed at: a new chunk starts
        // after it, so nothing stored is overwritten
        let next_no = self.head().map_or(0, |no| no + 1);
        let mut head = head.filter(|chunk| Some(chunk.no) == self.head());

        let merged = head
            .as_mut()
            .and_then(|chunk| chunk.lines.last_mut())
            .filter(|last| is_repeat(last, &actor, &note, now))
            .map(|last| {
                last.times = last.times.saturating_add(1);
                last.at = now;
            })
            .is_some();

        let mut chunk = match head {
            Some(chunk) if merged || chunk.lines.len() < retention.chunk_lines => chunk,
            _ => Chunk { no: next_no, lines: Vec::new() },
        };
        if !merged {
            chunk.lines.push(AuditLine { at: now, actor, note, times: 1 });
        }

        let meta = ChunkMeta { no: chunk.no, newest: now, len: chunk.lines.len() };
        match self.chunks.last_mut() {
            Some(last) if last.no == chunk.no => *last = meta,
            _ => self.chunks.push(meta),
        }

        let delete = self.prune(now, retention);
        Writes { put: chunk, delete }
    }

    /// Drops chunks that are entirely too old, or beyond the line limit.
    /// Returns their numbers. The head is never dropped.
    pub fn prune(&mut self, now: i64, retention: Retention) -> Vec<u64> {
        let oldest_kept = now - retention.max_age_ms;
        let over = self.chunks.len().saturating_sub(retention.max_chunks());
        let mut dropped = Vec::new();
        let last = self.chunks.len().saturating_sub(1);
        let mut position = 0;
        self.chunks.retain(|meta| {
            let keep = position == last || (position >= over && meta.newest >= oldest_kept);
            position += 1;
            if !keep {
                dropped.push(meta.no);
            }
            keep
        });
        dropped
    }
}

fn is_repeat(last: &AuditLine, actor: &Actor, note: &Note, now: i64) -> bool {
    matches!(note.action.area(), Area::Playback | Area::Teleprompter)
        && last.note == *note
        && last.actor == *actor
        && now - last.at <= MERGE_WINDOW_MS
}

/// Collects one page of lines, newest first, from chunks fed newest first.
#[derive(Debug)]
pub struct Page {
    before: i64,
    oldest_kept: i64,
    limit: usize,
    pub lines: Vec<AuditLine>,
}

impl Page {
    /// `before`: only lines strictly older than this. `limit` is capped at
    /// [`MAX_PAGE`].
    pub fn new(before: Option<i64>, limit: Option<usize>, now: i64, retention: Retention) -> Self {
        Self {
            before: before.unwrap_or(i64::MAX),
            oldest_kept: now - retention.max_age_ms,
            limit: limit.unwrap_or(100).clamp(1, MAX_PAGE),
            lines: Vec::new(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.lines.len() >= self.limit
    }

    /// Takes what it needs from the next older chunk. Returns `true` once full.
    pub fn take(&mut self, chunk: &Chunk) -> bool {
        for line in chunk.lines.iter().rev() {
            if self.is_full() {
                break;
            }
            if line.at < self.before && line.at >= self.oldest_kept {
                self.lines.push(line.clone());
            }
        }
        self.is_full()
    }

    /// Where the next page starts, when this one is full.
    pub fn next_before(&self) -> Option<i64> {
        self.is_full().then(|| self.lines.last().map(|line| line.at)).flatten()
    }
}
