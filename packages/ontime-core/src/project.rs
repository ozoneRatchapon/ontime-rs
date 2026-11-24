//! The persisted project document — the Rust counterpart of `DatabaseModel`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::audit::AuditIndex;
use crate::teleprompter::room::{Prompter, ScriptCache};
use crate::teleprompter::settings::TeleprompterSettings;
use crate::aux_timer::{AuxTimerNames, AuxTimers};
use crate::message::{ArchivedRun, ArchivedRunEvent, MessageState, Report, ReportArchive};
use crate::rundown::Rundown;
use crate::runtime::Runtime;

pub const ONTIME_VERSION: &str = "4.11.0";
pub const DEFAULT_RUNDOWN_ID: &str = "default";

/// What a rundown replacement changed beyond the rundown itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Replaced {
    /// The title or entry count moved, which the rundown list shows.
    pub list_changed: bool,
    /// The loaded event did not survive, so playback was stopped.
    pub stopped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceError {
    NotFound,
    NotAnEntry,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectData {
    pub title: String,
    pub description: String,
    pub url: String,
    pub info: String,
    pub logo: Option<String>,
    pub custom: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub version: String,
    pub editor_key: Option<String>,
    pub operator_key: Option<String>,
    pub time_format: String,
    pub language: String,
    #[serde(
        default,
        deserialize_with = "crate::aux_timer::deserialize_aux_timer_names"
    )]
    pub aux_timer_names: AuxTimerNames,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: ONTIME_VERSION.to_string(),
            editor_key: None,
            operator_key: None,
            time_format: "24".to_string(),
            language: "en".to_string(),
            aux_timer_names: AuxTimerNames::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSettings {
    pub override_styles: bool,
    pub normal_color: String,
    pub warning_color: String,
    pub danger_color: String,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            override_styles: false,
            normal_color: "#ffffffcc".to_string(),
            warning_color: "#ffa528".to_string(),
            danger_color: "#ff7300".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Keyed by rundown id.
    pub rundowns: BTreeMap<String, Rundown>,
    /// Id of the rundown the runtime has loaded.
    pub loaded: String,
    pub project: ProjectData,
    pub settings: Settings,
    pub view_settings: ViewSettings,
    /// Kept as raw JSON: the server does not act on these yet, it only stores them.
    #[serde(default)]
    pub url_presets: Vec<Value>,
    #[serde(default)]
    pub custom_fields: Value,
    #[serde(default)]
    pub automation: Value,
    #[serde(default)]
    pub css_override: String,
    /// Playback state, persisted so a restart mid-show resumes where it left off.
    #[serde(default)]
    pub runtime: Runtime,
    /// The three auxiliary stopwatches, independent of the rundown.
    #[serde(default)]
    pub aux_timers: AuxTimers,
    /// The operator's overlay on the timer views.
    #[serde(default)]
    pub message: MessageState,
    /// When each event actually ran — the live, overwritable record.
    #[serde(default)]
    pub report: Report,
    /// Permanent history of completed runs, never overwritten.
    #[serde(default)]
    pub report_archive: ReportArchive,
    /// Milliseconds to add to UTC to get the operator's wall clock. A Worker has
    /// no local timezone, so without this every displayed time-of-day is UTC.
    #[serde(default)]
    pub timezone_offset: i64,
    /// How often the server pushes `runtime-data` while a timer runs, in
    /// milliseconds. The browser advances the timer itself, so this only has to
    /// cover clients that cannot — see [`default_heartbeat_ms`].
    #[serde(default = "default_heartbeat_ms")]
    pub heartbeat_ms: i64,
    /// Hash of this room's password. `None` means the room is open, which matches
    /// the original server's behaviour when `SESSION_PASSWORD` is unset.
    #[serde(default)]
    pub password_hash: Option<String>,
    /// Only members, the room password or the master password may control the
    /// room; anyone allowed in may still watch. Off by default, so a room
    /// behaves as it always has until its owner turns it on.
    #[serde(default)]
    pub members_only: bool,
    /// Which chunks of the audit log exist. The lines are stored apart, see
    /// [`crate::audit`]. It lives here because this document is written with
    /// every change, so a change and its log line are written together.
    #[serde(default)]
    pub audit: AuditIndex,
    /// What the teleprompter reads and how it lays it out. Same key as
    /// upstream's project file, so project files stay compatible.
    #[serde(default)]
    pub teleprompter: TeleprompterSettings,
    /// The shared teleprompter transport, kept here because a Durable Object
    /// forgets anything held only in memory.
    #[serde(default)]
    pub prompter: Prompter,
    /// The teleprompter script, rebuilt only when what it reads changes.
    #[serde(skip)]
    pub prompter_cache: ScriptCache,
}

/// Hashes a room password. The resulting digest doubles as the bearer token, the
/// same way the original server uses `hashPassword(password)` as its token.
pub fn hash_password(password: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ontime-room-v1:");
    hasher.update(password.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Constant-time comparison, so a token cannot be recovered by timing the response.
pub fn token_matches(expected: &str, provided: &str) -> bool {
    if expected.len() != provided.len() {
        return false;
    }
    expected
        .bytes()
        .zip(provided.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Matches the original server's `notificationRate`, so a client that cannot run
/// the local engine behaves exactly as it does against `apps/server`.
/// Raising it cuts the running cost proportionally; see handover 009.
pub fn default_heartbeat_ms() -> i64 {
    1_000
}

impl Default for Project {
    fn default() -> Self {
        let rundown = Rundown::demo(DEFAULT_RUNDOWN_ID, "Default");
        let mut rundowns = BTreeMap::new();
        rundowns.insert(rundown.id.clone(), rundown);

        Self {
            rundowns,
            loaded: DEFAULT_RUNDOWN_ID.to_string(),
            project: ProjectData::default(),
            settings: Settings::default(),
            view_settings: ViewSettings::default(),
            url_presets: Vec::new(),
            custom_fields: json!({}),
            automation: json!({
                "enabledAutomations": true,
                "enabledOscIn": false,
                "oscPortIn": 8888,
                "triggers": [],
                "automations": {},
            }),
            css_override: String::new(),
            runtime: Runtime::default(),
            aux_timers: AuxTimers::default(),
            message: MessageState::default(),
            report: Report::default(),
            report_archive: ReportArchive::default(),
            timezone_offset: 0,
            heartbeat_ms: default_heartbeat_ms(),
            password_hash: None,
            members_only: false,
            audit: AuditIndex::default(),
            teleprompter: TeleprompterSettings::default(),
            prompter: Prompter::default(),
            prompter_cache: ScriptCache::default(),
        }
    }
}

impl Project {
    /// The rundown the runtime has loaded, falling back to any rundown that exists.
    pub fn current(&self) -> Option<&Rundown> {
        self.rundowns
            .get(&self.loaded)
            .or_else(|| self.rundowns.values().next())
    }

    pub fn current_mut(&mut self) -> Option<&mut Rundown> {
        let id = if self.rundowns.contains_key(&self.loaded) {
            self.loaded.clone()
        } else {
            self.rundowns.keys().next()?.clone()
        };
        self.rundowns.get_mut(&id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Rundown> {
        self.rundowns.get_mut(id)
    }

    /// `ProjectRundownsList` as the client expects it.
    pub fn rundown_list(&self) -> Value {
        let loaded = self.current().map(|r| r.id.clone()).unwrap_or_default();
        json!({
            "loaded": loaded,
            "rundowns": self.rundowns.values().map(Rundown::summary).collect::<Vec<_>>(),
        })
    }

    /// Snapshots a rundown's completed run into the permanent archive, then clears
    /// those entries from the live report.
    ///
    /// Titles, cues and planned durations are copied in so the record survives the
    /// rundown being edited or deleted. Only events that actually started are
    /// included; returns whether anything was archived (nothing ran = no-op, so
    /// switching between rundowns that were never played does not litter the
    /// archive with empty runs).
    pub fn archive_run(&mut self, rundown_id: &str, archived_at: i64) -> bool {
        let Some(rundown) = self.rundowns.get(rundown_id) else {
            return false;
        };

        let mut events = Vec::new();
        let mut ran_ids = Vec::new();
        for id in &rundown.flat_order {
            let Some(entry) = rundown.get(id) else {
                continue;
            };
            if entry.get("type").and_then(|v| v.as_str()) != Some("event") {
                continue;
            }
            let Some(record) = self.report.get(id) else {
                continue;
            };
            // an event only counts as having run if it actually started
            if record.started_at.is_none() {
                continue;
            }
            events.push(ArchivedRunEvent {
                id: id.clone(),
                cue: entry
                    .get("cue")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                title: entry
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                planned_duration: entry.get("duration").and_then(|v| v.as_i64()).unwrap_or(0),
                started_at: record.started_at,
                ended_at: record.ended_at,
            });
            ran_ids.push(id.clone());
        }

        if events.is_empty() {
            return false;
        }

        let rundown_title = rundown.title.clone();
        self.report_archive.push(ArchivedRun {
            rundown_id: rundown_id.to_string(),
            rundown_title,
            archived_at,
            events,
        });
        for id in ran_ids {
            self.report.clear_one(&id);
        }
        true
    }

    /// Swaps the contents of an existing rundown for `entries` (entry payloads in
    /// order, as `POST …/entry` takes them), keeping its id, and its title unless
    /// one is given.
    ///
    /// Replacing the loaded rundown is a change, not a switch: playback carries on
    /// while the loaded event is still there and playable, and stops otherwise.
    pub fn replace_rundown(
        &mut self,
        id: &str,
        title: Option<&str>,
        entries: &[Value],
    ) -> Result<Replaced, ReplaceError> {
        let existing = self.rundowns.get(id).ok_or(ReplaceError::NotFound)?;
        if entries.iter().any(|entry| !entry.is_object()) {
            return Err(ReplaceError::NotAnEntry);
        }

        let mut rundown = Rundown::new(id, title.unwrap_or(&existing.title));
        for entry in entries {
            // an id already taken by an earlier entry gets a fresh one
            rundown.add_entry(entry, crate::rundown::Insert::End);
        }
        let ids: Vec<String> = rundown.entries.keys().cloned().collect();
        crate::custom_fields::drop_unknown_values(&self.custom_fields, &mut rundown, &ids);
        rundown.revision = existing.revision + 1;
        let list_changed =
            rundown.title != existing.title || rundown.entries.len() != existing.entries.len();
        let was: Vec<String> = existing.entries.keys().cloned().collect();
        self.rundowns.insert(id.to_string(), rundown);
        self.prune_report(&was, id);

        let stopped = self.reconcile_runtime();
        Ok(Replaced {
            list_changed,
            stopped,
        })
    }

    /// After a change to the loaded rundown: if the loaded event is gone or can
    /// no longer play (skipped), playback stops, as upstream's runtime does.
    /// Returns true when it stopped.
    pub fn reconcile_runtime(&mut self) -> bool {
        let Some(selected) = self.runtime.selected_id.as_ref() else {
            return false;
        };
        let still_playable = self
            .current()
            .is_some_and(|rundown| crate::runtime::playable(rundown).contains(selected));
        if still_playable {
            return false;
        }
        self.runtime = Runtime::default();
        true
    }

    /// Ensures at least one rundown exists and that `loaded` points at a real one.
    pub fn ensure_consistent(&mut self) {
        if self.rundowns.is_empty() {
            let rundown = Rundown::new(DEFAULT_RUNDOWN_ID, "Default");
            self.rundowns.insert(rundown.id.clone(), rundown);
        }
        if !self.rundowns.contains_key(&self.loaded) {
            if let Some(first) = self.rundowns.keys().next().cloned() {
                self.loaded = first;
            }
        }
    }

    /// Clears live report records whose entries no longer exist in `rundown_id`,
    /// given the ids that were present before a delete. The report is keyed by
    /// entry id, so a deleted entry would otherwise leave a record pointing at
    /// nothing — never viewable, never clearable through the UI. The permanent
    /// archive is untouched: a run you want to keep is snapshotted before editing.
    pub(crate) fn prune_report(&mut self, was: &[String], rundown_id: &str) {
        let still: std::collections::HashSet<&str> = self
            .rundowns
            .get(rundown_id)
            .map(|r| r.entries.keys().map(String::as_str).collect())
            .unwrap_or_default();
        for id in was {
            if !still.contains(id.as_str()) {
                self.report.clear_one(id);
            }
        }
    }
}
