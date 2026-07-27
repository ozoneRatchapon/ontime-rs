//! The operator's message overlay, and the run report.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What is shown over the timer views.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerMessage {
    pub text: String,
    pub visible: bool,
    pub blink: bool,
    pub blackout: bool,
    /// One of `aux1`, `aux2`, `aux3`, `secondary`, or nothing.
    pub secondary_source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageState {
    pub timer: TimerMessage,
    pub secondary: String,
}

impl MessageState {
    /// Applies a `message` websocket payload. The client sends one field at a
    /// time — `{ timer: { blink: true } }`, `{ secondary: "…" }` — so anything
    /// absent must be left alone rather than reset to a default.
    ///
    /// Returns true when something changed.
    pub fn apply(&mut self, payload: &Value) -> bool {
        let Some(patch) = payload.as_object() else {
            return false;
        };

        let mut changed = false;

        if let Some(secondary) = patch.get("secondary").and_then(Value::as_str) {
            self.secondary = secondary.to_string();
            changed = true;
        }

        if let Some(timer) = patch.get("timer").and_then(Value::as_object) {
            if let Some(text) = timer.get("text").and_then(Value::as_str) {
                self.timer.text = text.to_string();
                changed = true;
            }
            for (key, field) in [
                ("visible", &mut self.timer.visible),
                ("blink", &mut self.timer.blink),
                ("blackout", &mut self.timer.blackout),
            ] {
                if let Some(value) = timer.get(key).and_then(Value::as_bool) {
                    *field = value;
                    changed = true;
                }
            }
            if let Some(source) = timer.get("secondarySource") {
                self.timer.secondary_source = source.as_str().map(str::to_string);
                changed = true;
            }
        }

        changed
    }
}

/// When an event actually started and ended, against which the planned times can
/// be compared after the fact.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportEntry {
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
}

/// Keyed by entry id; the client's `OntimeReport`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Report {
    entries: BTreeMap<String, ReportEntry>,
}

/// One event as it actually ran, with its planned title and duration copied in.
///
/// The live report holds only ids, which become meaningless once a rundown is
/// edited or deleted. An archive is a permanent record, so it denormalises the
/// title, cue and planned duration — it must stay analysable years later even if
/// the rundown it came from is long gone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedRunEvent {
    pub id: String,
    pub cue: String,
    pub title: String,
    /// Planned duration, milliseconds.
    pub planned_duration: i64,
    /// Time of day the event actually started and ended.
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
}

/// A completed run of one rundown, preserved for later analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedRun {
    pub rundown_id: String,
    pub rundown_title: String,
    /// Epoch milliseconds the snapshot was taken.
    pub archived_at: i64,
    pub events: Vec<ArchivedRunEvent>,
}

/// Append-only history of completed runs. Never overwritten and never
/// auto-deleted — that is the whole point.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReportArchive {
    runs: Vec<ArchivedRun>,
}

impl ReportArchive {
    pub fn push(&mut self, run: ArchivedRun) {
        self.runs.push(run);
    }

    pub fn runs(&self) -> &[ArchivedRun] {
        &self.runs
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

impl Report {
    /// Records the moment an event began. Restarting an event replaces its entry
    /// rather than keeping the first attempt, matching the original.
    pub fn on_start(&mut self, entry_id: &str, started_at: i64) {
        self.entries.insert(
            entry_id.to_string(),
            ReportEntry {
                started_at: Some(started_at),
                ended_at: None,
            },
        );
    }

    /// Records the moment an event stopped, keeping whatever start was recorded.
    pub fn on_stop(&mut self, entry_id: &str, ended_at: i64) {
        let started_at = self.entries.get(entry_id).and_then(|e| e.started_at);
        self.entries.insert(
            entry_id.to_string(),
            ReportEntry {
                started_at,
                ended_at: Some(ended_at),
            },
        );
    }

    pub fn clear_one(&mut self, entry_id: &str) -> bool {
        self.entries.remove(entry_id).is_some()
    }

    pub fn clear_all(&mut self) {
        self.entries.clear();
    }

    pub fn get(&self, entry_id: &str) -> Option<&ReportEntry> {
        self.entries.get(entry_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
