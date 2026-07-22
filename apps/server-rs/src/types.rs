use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TimerType {
    CountDown,
    CountUp,
    TimeOfDay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    Stopped,
    Running,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventItem {
    pub id: String,
    pub cue: String,
    pub title: String,
    pub duration: u64, // millis
    pub subtitle: Option<String>,
    pub presenter: Option<String>,
    pub note: Option<String>,
}

impl EventItem {
    pub fn new(cue: impl Into<String>, title: impl Into<String>, duration_sec: u64) -> Self {
        Self {
            id: Uuid::now_v7().to_string(),
            cue: cue.into(),
            title: title.into(),
            duration: duration_sec * 1000,
            subtitle: None,
            presenter: None,
            note: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectData {
    pub title: String,
    pub description: String,
    pub public_url: Option<String>,
}

impl Default for ProjectData {
    fn default() -> Self {
        Self {
            title: "Ontime Production Rundown".to_string(),
            description: "Live Event Schedule".to_string(),
            public_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub version: String,
    pub time_format: String,
    pub editor_key: Option<String>,
    pub operator_key: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: "4.11.0".to_string(),
            time_format: "24h".to_string(),
            editor_key: None,
            operator_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSettings {
    pub theme: String,
    pub font_size: u32,
    pub show_seconds: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            font_size: 16,
            show_seconds: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RundownState {
    pub room_id: String,
    pub project: ProjectData,
    pub settings: Settings,
    pub view_settings: ViewSettings,
    pub playback_state: PlaybackState,
    pub current_event_index: usize,
    pub elapsed_millis: u64,
    pub started_at_utc: Option<u64>,
    pub events: Vec<EventItem>,
    pub updated_at: u64,
}

impl Default for RundownState {
    fn default() -> Self {
        Self {
            room_id: "default".to_string(),
            project: ProjectData::default(),
            settings: Settings::default(),
            view_settings: ViewSettings::default(),
            playback_state: PlaybackState::Stopped,
            current_event_index: 0,
            elapsed_millis: 0,
            started_at_utc: None,
            events: vec![
                EventItem::new("1", "Welcome & Intro", 300),
                EventItem::new("2", "Keynote Presentation", 1800),
                EventItem::new("3", "Q&A Session", 600),
            ],
            updated_at: chrono::Utc::now().timestamp_millis() as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum WsAction {
    SyncState(RundownState),
    StartTimer,
    PauseTimer,
    ResetTimer,
    NextEvent,
    PreviousEvent,
    AddEvent { cue: String, title: String, duration: u64 },
    UpdateProjectTitle { title: String },
}
