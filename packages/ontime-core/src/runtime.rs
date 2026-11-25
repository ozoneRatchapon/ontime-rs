//! Playback runtime: what is loaded, whether it is running, and the timer maths.
//!
//! Time is stored internally as epoch milliseconds so that pausing and resuming
//! never has to reason about midnight. The client's `RuntimeStore` speaks
//! *time of day* (milliseconds since local midnight), so the conversion happens
//! once, in `snapshot`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use crate::command::Command;
use crate::offset::{
    expected_start, find_day_offset, runtime_offset, ExpectedStartInputs, OffsetInputs, OffsetMode,
};
use crate::roll::{is_now, normalise_roll_start};
pub use crate::roll::{load_roll, RollTarget};
use crate::rundown::{Rundown, MS_PER_DAY};
use crate::selection::{
    event_at_index, is_loadable, next_playable, next_roll_event, next_with_cue, playable,
    previous_playable, timed,
};

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
    /// In roll mode, milliseconds until the armed event is due. `None` once it
    /// is running.
    #[serde(default)]
    pub secondary_timer: Option<i64>,
    /// The event counts to a fixed wall-clock end rather than for a duration.
    #[serde(default)]
    pub count_to_end: bool,
    /// The event's planned end, as a time of day. Only meaningful with
    /// `count_to_end`.
    #[serde(default)]
    pub time_end: Option<i64>,
    /// Whether the event runs through midnight, which its end time alone cannot say.
    #[serde(default)]
    pub crosses_midnight: bool,
    /// Needed because a count-to-end timer is measured against the wall clock.
    #[serde(default)]
    pub tz_offset: i64,
}

impl TimerInputs {
    pub fn elapsed(&self, now: i64) -> Option<i64> {
        let started_at = self.started_at?;
        let elapsed = match self.playback {
            // roll counts exactly like play once its event is under way; the
            // difference is only in how the event was chosen
            Playback::Play | Playback::Roll => now - started_at - self.paused_total,
            Playback::Pause => self.paused_at? - started_at - self.paused_total,
            _ => return None,
        };
        // Anchoring the start to the nearest second can place it up to half a
        // second in the future. Without this the remaining time would briefly
        // exceed the duration and a 60s event would flash 01:01.
        Some(elapsed.max(0))
    }

    pub fn duration(&self) -> Option<i64> {
        self.planned_duration.map(|d| d + self.added_time)
    }

    pub fn current(&self, now: i64) -> Option<i64> {
        // an event pinned to a wall-clock end counts to that time whatever time it
        // was started, so it ignores the elapsed time entirely
        if self.count_to_end {
            let time_end = self.time_end?;
            let clock = time_of_day(now, self.tz_offset);
            let correct_day = if self.crosses_midnight { MS_PER_DAY } else { 0 };
            return Some(correct_day - clock + time_end + self.added_time);
        }

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
        // waiting for a rolling event to come round is its own phase: the timer
        // views show the countdown to the start rather than a running timer
        if self.secondary_timer.is_some() {
            return TimerPhase::Pending;
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
            // the planned length, as upstream reports it: added time shows in
            // `current` and `expectedFinish`, not here
            "duration": self.planned_duration,
            "elapsed": self.elapsed(now),
            "expectedFinish": match (self.started_at, duration) {
                (Some(started), Some(duration)) => {
                    Some(time_of_day(started + duration + self.paused_total, tz_offset))
                }
                _ => None,
            },
            "phase": self.phase(now),
            "playback": self.playback,
            "secondaryTimer": self.secondary_timer,
            "startedAt": self.started_at.map(|t| time_of_day(t, tz_offset)),
        })
    }

    /// Epoch milliseconds at which the loaded event runs out, if it is running.
    /// Lets the server schedule a single alarm per event instead of ticking.
    pub fn ends_at(&self, now: i64) -> Option<i64> {
        if !matches!(self.playback, Playback::Play | Playback::Roll) {
            return None;
        }
        // a count-to-end event ends at its wall-clock time, not a duration after
        // whenever it happened to start
        if self.count_to_end {
            return Some(now + self.current(now)?);
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
    /// In roll mode while waiting: the time of day the loaded event is due to
    /// start. `None` once it is actually running.
    #[serde(default)]
    pub secondary_target: Option<i64>,
    /// Which offset the expected times follow. A setting of the room rather
    /// than of the run, so a stop keeps it.
    #[serde(default)]
    pub offset_mode: OffsetMode,
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
            Command::StartNext => self.load_next(rundown) && self.start(rundown, now, tz_offset),
            Command::StartPrevious => {
                self.load_previous(rundown) && self.start(rundown, now, tz_offset)
            }
            Command::StartId(id) => {
                self.load_event(rundown, &id) && self.start(rundown, now, tz_offset)
            }
            Command::StartIndex(index) => {
                self.load_index(rundown, index) && self.start(rundown, now, tz_offset)
            }
            Command::StartCue(cue) => {
                // an armed event may itself be the one asked for
                let allow_current = self.playback == Playback::Armed;
                let order = timed(rundown);
                self.load_cue(rundown, &order, &cue, allow_current)
                    && self.start(rundown, now, tz_offset)
            }
            Command::LoadNext => self.load_next(rundown),
            Command::LoadPrevious => self.load_previous(rundown),
            Command::LoadId(id) => self.load_event(rundown, &id),
            Command::LoadIndex(index) => self.load_index(rundown, index),
            Command::LoadCue(cue) => {
                // upstream searches the playable order here, from a position in
                // the timed one; kept, so the same cue loads the same event
                let order = playable(rundown);
                self.load_cue(rundown, &order, &cue, false)
            }
            Command::Pause => self.pause(now),
            Command::Stop => self.stop(),
            Command::Reload => self.reload(rundown, now, tz_offset),
            Command::AddTime(amount) => self.add_time(amount),
            Command::Roll => self.roll(rundown, now, tz_offset),
            Command::SetOffsetMode(mode) => {
                let changed = self.offset_mode != mode;
                self.offset_mode = mode;
                changed
            }
        }
    }

    fn start(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        // resuming from a pause keeps the elapsed time
        if self.playback == Playback::Pause {
            if let Some(paused_at) = self.paused_at.take() {
                // Re-anchor rather than simply adding the paused span: the span is
                // an arbitrary fraction of a second, and carrying it forward would
                // put the countdown back out of step with the wall clock. The time
                // already counted is preserved to the nearest second.
                let started = self.started_at.unwrap_or(paused_at);
                let counted = paused_at - started - self.paused_total;
                // anchoring `now - counted` keeps the displayed value within half a
                // second of where it was paused; anchoring the two terms separately
                // could shift it by a whole second, which reads as a skipped tick.
                // The anchor goes into the paused total, so `startedAt` stays the
                // time the event really started, as upstream shows it.
                self.paused_total = align_to_second(now - counted) - started;
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
        // Anchored to a whole second, not to the instant the operator pressed the
        // button: the displayed countdown changes when the remaining time crosses
        // a second measured from here, so anchoring is what makes it tick in step
        // with the wall clock rather than at whatever fraction of a second the
        // press happened to land on.
        self.started_at = Some(align_to_second(now));
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
        *self = Runtime {
            offset_mode: self.offset_mode,
            ..Runtime::default()
        };
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

    /// Loads an event chosen by id, if it can be played (upstream loadEvent).
    fn load_event(&mut self, rundown: &Rundown, id: &str) -> bool {
        is_loadable(rundown, id) && self.load(rundown, Some(id.to_string()))
    }

    /// The next event, wrapping to the top; under roll, the next one roll
    /// could play (upstream handleLoadNext).
    fn load_next(&mut self, rundown: &Rundown) -> bool {
        let current = self.selected_id.as_deref();
        let next = match self.playback {
            Playback::Roll => next_roll_event(rundown, current),
            _ => next_playable(&playable(rundown), current).cloned(),
        };
        next.is_some_and(|id| self.load_event(rundown, &id))
    }

    /// The previous event; at the top, the first again (upstream handleLoadPrevious).
    fn load_previous(&mut self, rundown: &Rundown) -> bool {
        let previous = previous_playable(&playable(rundown), self.selected_id.as_deref()).cloned();
        previous.is_some_and(|id| self.load_event(rundown, &id))
    }

    fn load_index(&mut self, rundown: &Rundown, index: usize) -> bool {
        let chosen = event_at_index(&timed(rundown), index).cloned();
        chosen.is_some_and(|id| self.load_event(rundown, &id))
    }

    fn load_cue(
        &mut self,
        rundown: &Rundown,
        order: &[String],
        cue: &str,
        allow_current: bool,
    ) -> bool {
        let current_index = self
            .selected_id
            .as_ref()
            .and_then(|id| timed(rundown).iter().position(|e| e == id));
        let chosen = next_with_cue(rundown, order, cue, current_index, allow_current);
        chosen.is_some_and(|id| self.load_event(rundown, &id))
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

    /// Hands the rundown to the wall clock (upstream runtimeState.roll).
    ///
    /// 1. An event already playing simply carries on under roll, keeping how
    ///    late it is.
    /// 2. An armed event is rolled into: joined part-way if it is on air now,
    ///    otherwise counted down to.
    /// 3. Otherwise the clock picks the event: whichever should be on air, or
    ///    the next one to come.
    ///
    /// From then on `on_tick` moves from event to event without the operator.
    fn roll(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        match self.playback {
            Playback::Play if self.selected_id.is_some() => {
                self.playback = Playback::Roll;
                return true;
            }
            Playback::Armed if self.selected_id.is_some() => {
                return self.roll_into_loaded(rundown, now, tz_offset);
            }
            _ => {}
        }

        let clock = time_of_day(now, tz_offset);
        let Some(found) = load_roll(rundown, clock) else {
            return false;
        };
        self.selected_id = Some(found.id);
        self.roll_into_loaded(rundown, now, tz_offset)
    }

    /// Rolls into the selected event: joins it at the right point if the clock
    /// is inside it, otherwise waits for it to come round.
    fn roll_into_loaded(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        let Some(event) = self.selected_id.as_ref().and_then(|id| rundown.get(id)) else {
            return false;
        };
        let time_start = field(event, "timeStart").unwrap_or(0);
        let time_end = field(event, "timeEnd").unwrap_or(time_start);
        let day_offset = field(event, "dayOffset").unwrap_or(0);
        let clock = time_of_day(now, tz_offset);

        self.playback = Playback::Roll;
        self.paused_at = None;
        self.paused_total = 0;
        self.added_time = 0;

        if !is_now(time_start, time_end, clock) {
            // nothing is due yet: wait, counting down to the start
            self.started_at = None;
            self.secondary_target = Some(normalise_roll_start(time_start, clock));
            return true;
        }

        // the event is already under way, so back-date the start to join it at
        // the right point rather than restarting it
        let elapsed = (clock - time_start).rem_euclid(MS_PER_DAY);
        self.started_at = Some(now - elapsed);
        self.secondary_target = None;
        if self.actual_start.is_none() {
            self.backdate_show_start(now - elapsed, day_offset);
        }
        true
    }

    /// The run began when the event roll started in was planned to, not when
    /// roll was pressed, so no lateness is invented (upstream backdateShowStart).
    fn backdate_show_start(&mut self, started: i64, day_offset: i64) {
        self.actual_start = Some(started);
        self.start_day_offset = Some(day_offset);
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

    /// Whether the server has to keep waking. Roll counts even while waiting,
    /// because nothing else will start the event when its time comes.
    pub fn is_running(&self) -> bool {
        matches!(self.playback, Playback::Play | Playback::Roll)
    }

    /// Called on each tick. Applies the loaded event's `endAction` once its time
    /// is up. With `none` the timer simply runs into overtime, which is the
    /// default behaviour an operator expects.
    ///
    /// Returns `true` if the state changed.
    pub fn on_tick(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        if self.playback == Playback::Roll {
            return self.roll_tick(rundown, now, tz_offset);
        }

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

        // "over" is when nothing is left, which for a count-to-end event is a
        // wall-clock question rather than an elapsed-time one
        let inputs = self.inputs_for(Some(event), tz_offset);
        let Some(current) = inputs.current(now) else {
            return false;
        };
        if current > 0 {
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

    /// Roll mode advances itself: it starts the armed event when its time comes,
    /// and moves to the next one when the running event runs out.
    fn roll_tick(&mut self, rundown: &Rundown, now: i64, tz_offset: i64) -> bool {
        let clock = time_of_day(now, tz_offset);

        // waiting for an event's time to come round
        if let Some(target) = self.secondary_target {
            if target - clock > 0 {
                return false;
            }
            // anchor to the instant the event was actually due, not to the tick
            // that happened to notice — the target is already a whole second
            let started = align_to_second(now - (clock - target));
            self.started_at = Some(started);
            self.secondary_target = None;
            if self.actual_start.is_none() {
                let event = self.selected_id.as_ref().and_then(|id| rundown.get(id));
                let day_offset = event.and_then(|e| field(e, "dayOffset")).unwrap_or(0);
                self.backdate_show_start(started, day_offset);
            }
            return true;
        }

        // running: hand over to the next event once this one is spent
        let event = self.selected_id.as_ref().and_then(|id| rundown.get(id));
        let inputs = self.inputs_for(event, tz_offset);
        let Some(current) = inputs.current(now) else {
            return false;
        };
        if current > 0 {
            return false;
        }

        // the next event in order, waiting for it if it is not due yet
        // (upstream rollNext). Re-resolving by the clock instead would pick an
        // event that just ended past midnight again, as the next day's.
        match next_roll_event(rundown, self.selected_id.as_deref()) {
            Some(next) => {
                self.selected_id = Some(next);
                self.roll_into_loaded(rundown, now, tz_offset)
            }
            None => self.roll(rundown, now, tz_offset),
        }
    }

    /// Everything needed to compute the timer, decoupled from the rundown.
    ///
    /// The browser recomputes the timer between server packets using exactly this,
    /// so the maths lives in one place rather than being reimplemented in TypeScript.
    pub fn inputs_for(&self, event: Option<&Value>, tz_offset: i64) -> TimerInputs {
        let time_start = event.and_then(|e| field(e, "timeStart"));
        let time_end = event.and_then(|e| field(e, "timeEnd"));
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
            secondary_timer: None,
            count_to_end: event
                .and_then(|e| e.get("countToEnd"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            time_end,
            crosses_midnight: matches!((time_start, time_end), (Some(s), Some(e)) if s > e),
            tz_offset,
        }
    }

    /// The same, with the roll countdown filled in. `now` and `tz_offset` are
    /// needed because the countdown is against the wall clock.
    pub fn inputs_at(&self, event: Option<&Value>, now: i64, tz_offset: i64) -> TimerInputs {
        let mut inputs = self.inputs_for(event, tz_offset);
        if self.playback == Playback::Roll {
            if let Some(target) = self.secondary_target {
                inputs.secondary_timer = Some(target - time_of_day(now, tz_offset));
            }
        }
        inputs
    }

    // #endregion timer maths =====================================

    /// Walks forward from the loaded event, accumulating the idle time and the
    /// link-continuity that decide when later events are now expected to start.
    ///
    /// Gap and linkage are only counted *after* the loaded event: the loaded one
    /// is already running, so its own slack is spent.
    pub fn look_ahead(&self, rundown: &Rundown) -> LookAhead {
        let mut result = LookAhead::default();

        let events = playable(rundown);
        let Some(selected) = self.selected_id.as_ref() else {
            return result;
        };
        let Some(current_index) = events.iter().position(|id| id == selected) else {
            return result;
        };

        // the last playable event of the group the loaded event belongs to
        let group_id = rundown
            .get(selected)
            .and_then(|e| e.get("parent"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let last_in_group = group_id.as_ref().and_then(|group_id| {
            rundown
                .get(group_id)
                .and_then(|g| g.get("entries"))
                .and_then(Value::as_array)
                .and_then(|children| {
                    children
                        .iter()
                        .filter_map(Value::as_str)
                        .rfind(|id| events.iter().any(|e| e == id))
                        .map(str::to_string)
                })
        });

        let mut accumulated_gap = 0;
        let mut is_linked_to_loaded = true;

        for (index, id) in events.iter().enumerate().skip(current_index) {
            let Some(event) = rundown.get(id) else {
                continue;
            };

            if index != current_index {
                accumulated_gap += field(event, "gap").unwrap_or(0);
                is_linked_to_loaded = is_linked_to_loaded
                    && event
                        .get("linkStart")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);

                // the loaded event cannot be its own next flag
                if result.flag.is_none()
                    && event.get("flag").and_then(Value::as_bool).unwrap_or(false)
                {
                    result.flag = Some(LookAheadTarget {
                        id: id.clone(),
                        accumulated_gap,
                        is_linked_to_loaded,
                    });
                }
            }

            if result.group_end.is_none() && last_in_group.as_deref() == Some(id.as_str()) {
                result.group_end = Some(LookAheadTarget {
                    id: id.clone(),
                    accumulated_gap,
                    is_linked_to_loaded,
                });
            }
        }

        if let Some(last) = events.last() {
            result.rundown_end = Some(LookAheadTarget {
                id: last.clone(),
                accumulated_gap,
                is_linked_to_loaded,
            });
        }

        result
    }

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
        // the client counts "event 3 of 8" over every event, skipped ones too
        let timed = timed(rundown);
        let selected_timed_index = self
            .selected_id
            .as_ref()
            .and_then(|id| timed.iter().position(|e| e == id));
        // the day the run is on now: the day it began, plus each midnight since
        let current_day = self
            .actual_start
            .zip(self.start_day_offset)
            .map(|(began, day)| day + days_between(began, now, tz_offset));

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

        let inputs = self.inputs_at(event_now, now, tz_offset);

        let planned_start = events
            .first()
            .and_then(|id| rundown.get(id))
            .and_then(|e| field(e, "timeStart"));
        // not wrapped: a rundown running past midnight ends on a later day
        let planned_end = planned_start.map(|start| start + rundown.total_duration());

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
                // earlier pauses are in the total; a pause still running adds to it
                let paused_for = self.paused_total
                    + match (self.playback, self.paused_at) {
                        (Playback::Pause, Some(paused_at)) => now - paused_at,
                        _ => 0,
                    };

                let offset_inputs = OffsetInputs {
                    started_at: time_of_day(started, tz_offset),
                    start_day_offset,
                    started_day: start_day_offset + days_between(actual_start, started, tz_offset),
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

        // when the events ahead are now expected to run, given how far the run has
        // slipped and how much idle time is left to absorb it. Only while a run is
        // under way: armed or stopped, nothing is expected yet (upstream
        // getExpectedTimes). Not wrapped, like plannedEnd, so a time after
        // midnight stays after the clock it is compared with.
        let ahead = self.look_ahead(rundown);
        // and a roll still waiting for the show's first event has no run yet
        let active = matches!(
            self.playback,
            Playback::Play | Playback::Pause | Playback::Roll
        ) && self.actual_start.is_some();
        let expected_at = |target: Option<&LookAheadTarget>, add_duration: bool| -> Option<i64> {
            let target = target.filter(|_| active)?;
            let event = rundown.get(&target.id)?;
            let start = expected_start(&ExpectedStartInputs {
                time_start: field(event, "timeStart")?,
                day_offset: field(event, "dayOffset").unwrap_or(0),
                delay: field(event, "delay").unwrap_or(0),
                current_day: current_day.unwrap_or(0),
                total_gap: target.accumulated_gap,
                is_linked_to_loaded: target.is_linked_to_loaded,
                offset: match self.offset_mode {
                    OffsetMode::Absolute => absolute,
                    OffsetMode::Relative => relative,
                },
                mode: self.offset_mode,
                actual_start: self.actual_start.map(|t| time_of_day(t, tz_offset)),
                planned_start,
            });
            let span = match add_duration {
                true => field(event, "duration").unwrap_or(0),
                false => 0,
            };
            Some(start + span)
        };

        // a flag marks a point someone is waiting on, so it is the start that matters
        let expected_flag_start = expected_at(ahead.flag.as_ref(), false);
        // for the ends, it is the last event's start plus how long it runs
        let expected_group_end = expected_at(ahead.group_end.as_ref(), true);
        let expected_rundown_end = expected_at(ahead.rundown_end.as_ref(), true);

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
                "selectedEventIndex": selected_timed_index,
                "numEvents": timed.len(),
                "plannedStart": planned_start,
                "plannedEnd": planned_end,
                "actualStart": self.actual_start.map(|t| time_of_day(t, tz_offset)),
                "actualGroupStart": Value::Null,
                "currentDay": current_day,
            },
            "offset": {
                "absolute": absolute,
                "relative": relative,
                "mode": self.offset_mode,
                "expectedFlagStart": expected_flag_start,
                "expectedGroupEnd": expected_group_end,
                "expectedRundownEnd": expected_rundown_end,
            },
            "eventNow": event_now,
            "eventNext": event_next,
            "eventFlag": ahead.flag.as_ref().and_then(|t| rundown.get(&t.id)),
            "groupNow": event_now
                .and_then(|e| e.get("parent"))
                .and_then(Value::as_str)
                .and_then(|parent| rundown.get(parent)),
        })
    }
}

// #region helpers ================================================

/// A point ahead of the loaded event, with the slack and link-continuity between.
#[derive(Debug, Clone)]
pub struct LookAheadTarget {
    pub id: String,
    /// Idle time between the loaded event and this one, which can absorb lateness.
    pub accumulated_gap: i64,
    /// Whether every event between here and the loaded one starts linked, in
    /// which case there is no slack to absorb anything.
    pub is_linked_to_loaded: bool,
}

/// What lies ahead of the loaded event: the next flag, the end of its group, and
/// the end of the rundown.
#[derive(Debug, Clone, Default)]
pub struct LookAhead {
    pub flag: Option<LookAheadTarget>,
    pub group_end: Option<LookAheadTarget>,
    pub rundown_end: Option<LookAheadTarget>,
}

/// Rounds an instant to the nearest whole second.
///
/// Every real timezone is a whole number of minutes from UTC, so a second
/// boundary in epoch time is also one on the operator's wall clock.
fn align_to_second(epoch_ms: i64) -> i64 {
    (epoch_ms + 500).div_euclid(1000) * 1000
}

/// How many local midnights lie between two instants (upstream daysSinceStart).
fn days_between(from: i64, to: i64, tz_offset: i64) -> i64 {
    (to + tz_offset).div_euclid(MS_PER_DAY) - (from + tz_offset).div_euclid(MS_PER_DAY)
}

pub(crate) fn field(entry: &Value, key: &str) -> Option<i64> {
    entry.get(key).and_then(Value::as_i64)
}

fn wrap_day(value: i64) -> i64 {
    value.rem_euclid(MS_PER_DAY)
}

/// Milliseconds since local midnight for an epoch timestamp.
pub(crate) fn time_of_day(epoch_ms: i64, tz_offset: i64) -> i64 {
    wrap_day(epoch_ms + tz_offset)
}

// #endregion helpers =============================================
