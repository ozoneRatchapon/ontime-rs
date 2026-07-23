//! Auxiliary timers: three independent stopwatches, unrelated to the rundown.
//!
//! Like the playback runtime, state is kept as epoch timestamps and `current` is
//! derived on demand rather than mutated on a tick. That keeps a paused timer
//! exact regardless of how often anything asks for it, and lets the value be
//! recomputed after an eviction without having missed any ticks.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Default when a timer has never been given a duration: five minutes,
/// matching the original's `auxTimerDefault`.
pub const DEFAULT_DURATION: i64 = 5 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SimplePlayback {
    Start,
    Pause,
    #[default]
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SimpleDirection {
    CountUp,
    #[default]
    CountDown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxTimer {
    pub duration: i64,
    pub direction: SimpleDirection,
    pub playback: SimplePlayback,
    started_at: Option<i64>,
    paused_at: Option<i64>,
    /// What `stop` returns to.
    initial_duration: i64,
}

impl Default for AuxTimer {
    fn default() -> Self {
        Self {
            duration: DEFAULT_DURATION,
            direction: SimpleDirection::default(),
            playback: SimplePlayback::default(),
            started_at: None,
            paused_at: None,
            initial_duration: DEFAULT_DURATION,
        }
    }
}

impl AuxTimer {
    pub fn is_running(&self) -> bool {
        self.playback == SimplePlayback::Start
    }

    /// Milliseconds counted since the timer started, excluding paused time.
    fn elapsed(&self, now: i64) -> i64 {
        let Some(started_at) = self.started_at else {
            return 0;
        };
        match self.playback {
            SimplePlayback::Start => now - started_at,
            SimplePlayback::Pause => self.paused_at.unwrap_or(started_at) - started_at,
            SimplePlayback::Stop => 0,
        }
    }

    /// The displayed value. Counts down towards zero and past it, or up from the
    /// duration, depending on direction.
    pub fn current(&self, now: i64) -> i64 {
        let elapsed = self.elapsed(now);
        match self.direction {
            SimpleDirection::CountDown => self.duration - elapsed,
            SimpleDirection::CountUp => self.duration + elapsed,
        }
    }

    pub fn set_time(&mut self, millis: i64) {
        self.duration = millis;
        self.initial_duration = millis;
        self.started_at = None;
        self.paused_at = None;
        self.playback = SimplePlayback::Stop;
    }

    /// Extends (or shortens) the timer. `current` follows because it is derived
    /// from `duration`, so this works whether or not the timer is running.
    pub fn add_time(&mut self, millis: i64) {
        self.duration += millis;
    }

    pub fn set_direction(&mut self, direction: SimpleDirection, now: i64) {
        // while running, rebase so the displayed value does not jump: the time
        // counted so far is folded into the duration and the clock restarts
        if self.playback == SimplePlayback::Start {
            self.duration = self.current(now);
            self.started_at = Some(now);
        }
        self.direction = direction;
    }

    pub fn start(&mut self, now: i64) {
        match self.playback {
            SimplePlayback::Start => return,
            // resume without losing what was already counted
            SimplePlayback::Pause => {
                let counted = self.paused_at.unwrap_or(now) - self.started_at.unwrap_or(now);
                self.started_at = Some(now - counted);
                self.paused_at = None;
            }
            SimplePlayback::Stop => {
                self.started_at = Some(now);
                self.paused_at = None;
            }
        }
        self.playback = SimplePlayback::Start;
    }

    pub fn pause(&mut self, now: i64) {
        if self.playback != SimplePlayback::Start {
            return;
        }
        self.playback = SimplePlayback::Pause;
        self.paused_at = Some(now);
    }

    pub fn stop(&mut self) {
        self.playback = SimplePlayback::Stop;
        self.duration = self.initial_duration;
        self.started_at = None;
        self.paused_at = None;
    }

    /// `SimpleTimerState`, as the client's runtime store holds it.
    pub fn state(&self, now: i64) -> Value {
        json!({
            "duration": self.duration,
            "current": self.current(now),
            "playback": self.playback,
            "direction": self.direction,
        })
    }
}

/// The three timers the client exposes, addressed by the indices 1, 2 and 3.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuxTimers {
    pub timers: [AuxTimer; 3],
}

impl AuxTimers {
    fn get_mut(&mut self, index: usize) -> Option<&mut AuxTimer> {
        // the client counts from one; anything else is ignored rather than
        // silently applied to the wrong timer
        self.timers.get_mut(index.checked_sub(1)?)
    }

    pub fn any_running(&self) -> bool {
        self.timers.iter().any(AuxTimer::is_running)
    }

    /// Applies an `auxtimer` websocket payload: `{ "1": "start" }`,
    /// `{ "2": { "direction": "count-up" } }`, `{ "3": { "duration": 60000 } }`.
    ///
    /// Returns true when something changed and clients should be told.
    pub fn apply(&mut self, payload: &Value, now: i64) -> bool {
        let Some(commands) = payload.as_object() else {
            return false;
        };

        let mut changed = false;
        for (key, command) in commands {
            let Ok(index) = key.parse::<usize>() else {
                continue;
            };
            let Some(timer) = self.get_mut(index) else {
                continue;
            };

            if let Some(action) = command.as_str() {
                match action {
                    "start" => timer.start(now),
                    "pause" => timer.pause(now),
                    "stop" => timer.stop(),
                    _ => continue,
                }
                changed = true;
                continue;
            }

            let Some(patch) = command.as_object() else {
                continue;
            };
            if let Some(direction) = patch.get("direction").and_then(Value::as_str) {
                let direction = match direction {
                    "count-up" => SimpleDirection::CountUp,
                    "count-down" => SimpleDirection::CountDown,
                    _ => continue,
                };
                timer.set_direction(direction, now);
                changed = true;
            }
            if let Some(duration) = patch.get("duration").and_then(Value::as_i64) {
                timer.set_time(duration);
                changed = true;
            }
            if let Some(added) = patch.get("addtime").and_then(Value::as_i64) {
                timer.add_time(added);
                changed = true;
            }
        }

        changed
    }

    /// The `auxtimer1`/`auxtimer2`/`auxtimer3` slice of the runtime store.
    pub fn snapshot(&self, now: i64) -> Value {
        json!({
            "auxtimer1": self.timers[0].state(now),
            "auxtimer2": self.timers[1].state(now),
            "auxtimer3": self.timers[2].state(now),
        })
    }
}
