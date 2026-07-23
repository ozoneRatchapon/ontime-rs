//! The persisted project document — the Rust counterpart of `DatabaseModel`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::aux_timer::AuxTimers;
use crate::message::{MessageState, Report};
use crate::rundown::Rundown;
use crate::runtime::Runtime;

pub const ONTIME_VERSION: &str = "4.11.0";
pub const DEFAULT_RUNDOWN_ID: &str = "default";

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: ONTIME_VERSION.to_string(),
            editor_key: None,
            operator_key: None,
            time_format: "24".to_string(),
            language: "en".to_string(),
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
    /// When each event actually ran.
    #[serde(default)]
    pub report: Report,
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
            timezone_offset: 0,
            heartbeat_ms: default_heartbeat_ms(),
            password_hash: None,
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
}
