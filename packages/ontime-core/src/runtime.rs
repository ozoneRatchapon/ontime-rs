//! Playback runtime: what is loaded, whether it is running, and the timer maths.
//!
//! Time is stored internally as epoch milliseconds so that pausing and resuming
//! never has to reason about midnight. The client's `RuntimeStore` speaks
//! *time of day* (milliseconds since local midnight), so the conversion happens
//! once, in `snapshot`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::offset::{find_day_offset, runtime_offset, OffsetInputs};
use crate::rundown::{Rundown, MS_PER_DAY};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Playback {
    Roll,
    Play,
    Pause,
    #[default]
    Stop,
    Armed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerPhase {
    None,
    Default,
    Warning,
    Danger,
    Overtime,
    Pending,
}

/// A playback instruction received over the websocket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Resume, or start whatever is loaded (loading the first event if nothing is).
    Start,
    StartNext,
    StartId(String),
    LoadNext,
    LoadPrevious,
    LoadId(String),
    Pause,
    Stop,
    /// Restart the loaded event from zero.
    Reload,
    /// Milliseconds to add to the running event; negative removes time.
    AddTime(i64),
}

impl Command {
    /// Maps a `{tag, payload}` websocket packet onto a command.
    /// Returns `None` for tags this engine does not implement.
    pub fn parse(tag: &str, payload: Option<&Value>) -> Option<Self> {
        let as_id = || {
            payload
                .and_then(|p| p.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let as_str = || payload.and_then(Value::as_str);

        match tag {
            "start" => match as_str() {
                Some("next") => Some(Command::StartNext),
                _ => match as_id() {
                    Some(id) => Some(Command::StartId(id)),
                    None => Some(Command::Start),
                },
            },
            "load" => match as_str() {
                Some("next") => Some(Command::LoadNext),
                Some("previous") => Some(Command::LoadPrevious),
                _ => as_id().map(Command::LoadId),
            },
            "pause" => Some(Command::Pause),
            "stop" => Some(Command::Stop),
            "reload" => Some(Command::Reload),
            "addtime" => payload.and_then(Value::as_i64).map(Command::AddTime),
            _ => None,
        }
    }
}

/// The inputs the per-frame timer computation needs.
///
/// Serialised into every `runtime-data` packet so the browser can keep counting
/// between packets without the server having to tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerInputs {
    pub playback: Playback,
    /// Epoch milliseconds.
    pub started_at: Option<i64>,
    pub paused_at: Option<i64>,
    pub paused_total: i64,
    pub added_time: i64,
    pub planned_duration: Option<i64>,
    pub time_warning: i64,
    pub time_danger: i64,
    /// False when nothing is loaded, which makes the timer read as empty.
    pub has_event: bool,
}

impl TimerInputs {
    pub fn elapsed(&self, now: i64) -> Option<i64> {
        let started_at = self.started_at?;
        match self.playback {
            Playback::Play => Some(now - started_at - self.paused_total),
            Playback::Pause => Some(self.paused_at? - started_at - self.paused_total),
            _ => None,
        }
    }

    pub fn duration(&self) -> Option<i64> {
        self.planned_duration.map(|d| d + self.added_time)
    }

    pub fn current(&self, now: i64) -> Option<i64> {
        match (self.duration(), self.elapsed(now)) {
            (Some(duration), Some(elapsed)) => Some(duration - elapsed),
            // armed but not started: the full duration is still ahead
            (Some(duration), None) => Some(duration),
            _ => None,
        }
    }

    pub fn phase(&self, now: i64) -> TimerPhase {
        let Some(current) = self.current(now) else {
            return TimerPhase::None;
        };
        if !self.has_event {
            return TimerPhase::None;
        }
        if current < 0 {
            TimerPhase::Overtime
        } else if current <= self.time_danger {
            TimerPhase::Danger
        } else if current <= self.time_warning {
            TimerPhase::Warning
        } else {
            TimerPhase::Default
        }
    }

    /// The `TimerState` object the client's runtime store holds.
    pub fn timer_state(&self, now: i64, tz_offset: i64) -> Value {
        let duration = self.duration();
        json!({
            "addedTime": self.added_time,
            "current": self.current(now),
            "duration": duration,
            "elapsed": self.elapsed(now),
            "expectedFinish": match (self.started_at, duration) {
                (Some(started), Some(duration)) => {
                    Some(time_of_day(started + duration + self.paused_total, tz_offset))
                }
                _ => None,
            },
            "phase": self.phase(now),
            "playback": self.playback,
            "secondaryTimer": Value::Null,
            "startedAt": self.started_at.map(|t| time_of_day(t, tz_offset)),
        })
    }

    /// Epoch milliseconds at which the loaded event runs out, if it is running.
    /// Lets the server schedule a single alarm per event instead of ticking.
    pub fn ends_at(&self) -> Option<i64> {
        if self.playback != Playback::Play {
            return None;
        }
        Some(self.started_at? + self.duration()? + self.paused_total)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub playback: Playback,
    /// Id of the loaded event, if any.
    pub selected_id: Option<String>,
    /// Epoch ms at which the loaded event started running.
    pub started_at: Option<i64>,
    /// Epoch ms at which it was paused, while paused.
    pub paused_at: Option<i64>,
    /// Total milliseconds spent paused during this event.
    pub paused_total: i64,
    /// Milliseconds added by the operator, can be negative.
    pub added_time: i64,
    /// Epoch ms at which the first event of this run started.
    pub actual_start: Option<i64>,
    /// Which day the run began on, relative to the rundown's planned start.
    /// Fixed when the run starts, because everything after is measured from it.
    #[serde(default)]
    pub start_day_offset: Option<i64>,
}

impl Runtime {
    // #region commands ===========================================

    /// Applies a command. `now` is epoch milliseconds.
    ///
    /// `tz_offset` is needed because starting a run fixes which *day* it began
    /// on, and that is a wall-clock question.
    ///
    /// Returns `true` when the resulting state should be broadcast.
    pub fn apply(&mut self, command: Command, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        match command {
            Command::Start => self.start(rundown, now, tz_offset),
            Command::StartNext => {
                self.step(rundown, 1);
                self.start(rundown, now, tz_offset)
            }
            Command::StartId(id) => {
                self.load(rundown, Some(id));
                self.start(rundown, now, tz_offset)
            }
            Command::LoadNext => self.step(rundown, 1),
            Command::LoadPrevious => self.step(rundown, -1),
            Command::LoadId(id) => self.load(rundown, Some(id)),
            Command::Pause => self.pause(now),
            Command::Stop => self.stop(),
            Command::Reload => self.reload(rundown, now, tz_offset),
            Command::AddTime(amount) => self.add_time(amount),
        }
    }

    fn start(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        // resuming from a pause keeps the elapsed time
        if self.playback == Playback::Pause {
            if let Some(paused_at) = self.paused_at.take() {
                self.paused_total += now - paused_at;
            }
            self.playback = Playback::Play;
            return true;
        }

        if self.playback == Playback::Play {
            return false;
        }

        if self.selected_id.is_none() {
            let first = playable(rundown).first().cloned();
            if first.is_none() {
                return false;
            }
            self.selected_id = first;
        }

        self.playback = Playback::Play;
        self.started_at = Some(now);
        self.paused_at = None;
        self.paused_total = 0;
        self.added_time = 0;
        // the day the run began is fixed here: every later event is measured
        // against it, so it must not be recomputed as the rundown advances
        if self.actual_start.is_none() {
            self.actual_start = Some(now);
            let clock = time_of_day(now, tz_offset);
            let event = self.selected_id.as_ref().and_then(|id| rundown.get(id));
            let event_start = event.and_then(|e| field(e, "timeStart")).unwrap_or(0);
            let event_day = event.and_then(|e| field(e, "dayOffset")).unwrap_or(0);
            self.start_day_offset = Some(find_day_offset(event_start, clock) + event_day);
        }
        true
    }

    fn pause(&mut self, now: i64) -> bool {
        if self.playback != Playback::Play {
            return false;
        }
        self.playback = Playback::Pause;
        self.paused_at = Some(now);
        true
    }

    fn stop(&mut self) -> bool {
        if self.playback == Playback::Stop && self.selected_id.is_none() {
            return false;
        }
        // a full reset: the next run establishes its own day and start time
        *self = Runtime::default();
        true
    }

    fn reload(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        let Some(id) = self.selected_id.clone() else {
            return false;
        };
        let was_playing = self.playback == Playback::Play;
        self.load(rundown, Some(id));
        if was_playing {
            self.start(rundown, now, tz_offset);
        }
        true
    }

    /// Selects an event and arms it — loaded but not counting.
    fn load(&mut self, rundown: &Rundown, id: Option<String>) -> bool {
        let Some(id) = id else { return false };
        if rundown.get(&id).is_none() {
            return false;
        }
        self.selected_id = Some(id);
        self.playback = Playback::Armed;
        self.started_at = None;
        self.paused_at = None;
        self.paused_total = 0;
        self.added_time = 0;
        true
    }

    /// Moves the selection by `offset` positions through the playable events.
    fn step(&mut self, rundown: &Rundown, offset: i64) -> bool {
        let events = playable(rundown);
        if events.is_empty() {
            return false;
        }

        let next = match self
            .selected_id
            .as_ref()
            .and_then(|id| events.iter().position(|e| e == id))
        {
            Some(current) => {
                let target = current as i64 + offset;
                if target < 0 || target as usize >= events.len() {
                    return false;
                }
                events[target as usize].clone()
            }
            // nothing loaded: stepping forward starts at the top
            None if offset > 0 => events[0].clone(),
            None => return false,
        };

        self.load(rundown, Some(next))
    }

    fn add_time(&mut self, amount: i64) -> bool {
        if self.selected_id.is_none() {
            return false;
        }
        self.added_time += amount;
        true
    }

    // #endregion commands ========================================
    // #region timer maths ========================================

    pub fn is_running(&self) -> bool {
        self.playback == Playback::Play
    }

    /// Called on each tick. Applies the loaded event's `endAction` once its time
    /// is up. With `none` the timer simply runs into overtime, which is the
    /// default behaviour an operator expects.
    ///
    /// Returns `true` if the state changed.
    pub fn on_tick(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        if self.playback != Playback::Play {
            return false;
        }

        let Some(event) = self.selected_id.as_ref().and_then(|id| rundown.get(id)) else {
            return false;
        };
        let end_action = event
            .get("endAction")
            .and_then(Value::as_str)
            .unwrap_or("none");
        if end_action == "none" {
            return false;
        }

        let duration = field(event, "duration").unwrap_or(0) + self.added_time;
        let Some(elapsed) = self.elapsed(now) else {
            return false;
        };
        if elapsed < duration {
            return false;
        }

        match end_action {
            "load-next" => {
                let advanced = self.step(rundown, 1);
                if !advanced {
                    // nothing left to load: hold at the end rather than loop
                    self.playback = Playback::Pause;
                    self.paused_at = Some(now);
                }
                true
            }
            "play-next" => {
                if self.step(rundown, 1) {
                    self.start(rundown, now, tz_offset);
                } else {
                    self.playback = Playback::Pause;
                    self.paused_at = Some(now);
                }
                true
            }
            _ => false,
        }
    }

    /// Milliseconds the loaded event has been counting, excluding paused time.
    fn elapsed(&self, now: i64) -> Option<i64> {
        self.inputs_for(None).elapsed(now)
    }

    /// Everything needed to compute the timer, decoupled from the rundown.
    ///
    /// The browser recomputes the timer between server packets using exactly this,
    /// so the maths lives in one place rather than being reimplemented in TypeScript.
    pub fn inputs_for(&self, event: Option<&Value>) -> TimerInputs {
        TimerInputs {
            playback: self.playback,
            started_at: self.started_at,
            paused_at: self.paused_at,
            paused_total: self.paused_total,
            added_time: self.added_time,
            planned_duration: event.and_then(|e| field(e, "duration")),
            time_warning: event
                .and_then(|e| field(e, "timeWarning"))
                .unwrap_or(120_000),
            time_danger: event.and_then(|e| field(e, "timeDanger")).unwrap_or(60_000),
            has_event: event.is_some(),
        }
    }

    // #endregion timer maths =====================================

    /// Builds the `runtime-data` payload the client's `RuntimeStore` expects.
    ///
    /// `now` is epoch milliseconds; `tz_offset` shifts wall-clock values so a
    /// Worker (which runs on UTC) can report the operator's local time of day.
    pub fn snapshot(&self, rundown: &Rundown, now: i64, tz_offset: i64) -> Value {
        let events = playable(rundown);
        let selected_index = self
            .selected_id
            .as_ref()
            .and_then(|id| events.iter().position(|e| e == id));

        let event_now = self.selected_id.as_ref().and_then(|id| rundown.get(id));
        let event_next = selected_index
            .and_then(|i| events.get(i + 1))
            .or_else(|| {
                if selected_index.is_none() {
                    events.first()
                } else {
                    None
                }
            })
            .and_then(|id| rundown.get(id));

        let inputs = self.inputs_for(event_now);

        let planned_start = events
            .first()
            .and_then(|id| rundown.get(id))
            .and_then(|e| field(e, "timeStart"));
        let planned_end = events
            .last()
            .and_then(|id| rundown.get(id))
            .and_then(|e| field(e, "timeEnd"));

        // How far the run is from the plan. Positive means behind.
        let (absolute, relative) = match (
            event_now,
            self.started_at,
            self.actual_start,
            self.start_day_offset,
            planned_start,
        ) {
            (
                Some(event),
                Some(started),
                Some(actual_start),
                Some(start_day_offset),
                Some(planned),
            ) => {
                // measured in epoch, so a pause spanning midnight still counts
                // correctly — the original computes this in time-of-day and
                // notes the midnight case as broken
                let paused_for = match (self.playback, self.paused_at) {
                    (Playback::Pause, Some(paused_at)) => now - paused_at,
                    _ => 0,
                };

                let offset_inputs = OffsetInputs {
                    started_at: time_of_day(started, tz_offset),
                    start_day_offset,
                    event_time_start: field(event, "timeStart").unwrap_or(0),
                    event_day_offset: field(event, "dayOffset").unwrap_or(0),
                    current: inputs.current(now).unwrap_or(0),
                    added_time: self.added_time,
                    paused_for,
                    count_to_end: event
                        .get("countToEnd")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    planned_start: planned,
                    actual_start: time_of_day(actual_start, tz_offset),
                };
                runtime_offset(&offset_inputs)
            }
            _ => (0, 0),
        };

        json!({
            "clock": time_of_day(now, tz_offset),
            "timer": inputs.timer_state(now, tz_offset),
            // Not part of the client's RuntimeStore. Carries the inputs the browser
            // needs to keep counting between packets, so the server does not have to
            // tick once a second. `serverNow` lets the client correct for clock skew.
            "_engine": {
                "serverNow": now,
                "tzOffset": tz_offset,
                "inputs": inputs,
            },
            "rundown": {
                "selectedEventIndex": selected_index,
                "numEvents": events.len(),
                "plannedStart": planned_start,
                "plannedEnd": planned_end,
                "actualStart": self.actual_start.map(|t| time_of_day(t, tz_offset)),
                "actualGroupStart": Value::Null,
                "currentDay": event_now.and_then(|e| field(e, "dayOffset")),
            },
            "offset": {
                "absolute": absolute,
                "relative": relative,
                "mode": "absolute",
                // groups and flags need the accumulated-gap walk, which is not
                // ported yet; null rather than a number that would look authoritative
                "expectedFlagStart": Value::Null,
                "expectedGroupEnd": Value::Null,
                "expectedRundownEnd": planned_end.map(|end| wrap_day(end + absolute)),
            },
            "eventNow": event_now,
            "eventNext": event_next,
            "eventFlag": Value::Null,
            "groupNow": event_now
                .and_then(|e| e.get("parent"))
                .and_then(Value::as_str)
                .and_then(|parent| rundown.get(parent)),
        })
    }
}

// #region helpers ================================================

/// Ids of the events that playback can select: real events, in order, not skipped.
fn playable(rundown: &Rundown) -> Vec<String> {
    rundown
        .flat_order
        .iter()
        .filter(|id| {
            rundown
                .get(id)
                .map(|entry| {
                    entry.get("type").and_then(Value::as_str) == Some("event")
                        && entry.get("skip").and_then(Value::as_bool) != Some(true)
                })
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn field(entry: &Value, key: &str) -> Option<i64> {
    entry.get(key).and_then(Value::as_i64)
}

fn wrap_day(value: i64) -> i64 {
    value.rem_euclid(MS_PER_DAY)
}

/// Milliseconds since local midnight for an epoch timestamp.
fn time_of_day(epoch_ms: i64, tz_offset: i64) -> i64 {
    wrap_day(epoch_ms + tz_offset)
}

// #endregion helpers =============================================
