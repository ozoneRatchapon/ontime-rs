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

/// Longest name an aux timer may carry, as upstream's `auxTimerNameMaxLength`.
/// Counted in characters, so a Thai name gets the same room as a Latin one.
pub const AUX_TIMER_NAME_MAX_LENGTH: usize = 30;

/// One name per aux timer, in order. An empty string means unnamed, and the
/// client shows its default label.
pub type AuxTimerNames = [String; 3];

fn sanitise_aux_timer_name(value: Option<&Value>) -> String {
    match value.and_then(Value::as_str) {
        Some(name) => name
            .trim()
            .chars()
            .take(AUX_TIMER_NAME_MAX_LENGTH)
            .collect(),
        None => String::new(),
    }
}

/// A name for each of the three timers from whatever was stored or sent, so a
/// missing or malformed list (a room saved before names existed) never fails.
pub fn sanitise_aux_timer_names(names: &Value) -> AuxTimerNames {
    let list = names.as_array();
    let at = |index: usize| sanitise_aux_timer_name(list.and_then(|list| list.get(index)));
    [at(0), at(1), at(2)]
}

/// serde hook for `Settings::aux_timer_names`: always yields three clean names.
pub fn deserialize_aux_timer_names<'de, D>(deserializer: D) -> Result<AuxTimerNames, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Value::deserialize(deserializer)?;
    Ok(sanitise_aux_timer_names(&raw))
}

/// Rounds an instant to the nearest whole second.
///
/// An aux timer sits next to the wall clock on screen, so its displayed second
/// has to change at the same moment the clock's does. Anchoring the start to a
/// second boundary is what achieves that; anchoring to the instant the operator
/// pressed the button leaves the two visibly out of step.
fn align_to_second(epoch_ms: i64) -> i64 {
    (epoch_ms + 500).div_euclid(1000) * 1000
}

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
        let elapsed = match self.playback {
            SimplePlayback::Start => now - started_at,
            SimplePlayback::Pause => self.paused_at.unwrap_or(started_at) - started_at,
            SimplePlayback::Stop => 0,
        };
        // rounding to the nearest second can put the anchor briefly in the
        // future, which would read as more time than the timer was set to
        elapsed.max(0)
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
            self.started_at = Some(align_to_second(now));
        }
        self.direction = direction;
    }

    pub fn start(&mut self, now: i64) {
        match self.playback {
            SimplePlayback::Start => return,
            // resume without losing what was already counted, re-anchoring so
            // the display stays in step with the clock
            SimplePlayback::Pause => {
                let counted = self.paused_at.unwrap_or(now) - self.started_at.unwrap_or(now);
                self.started_at = Some(align_to_second(now - counted));
                self.paused_at = None;
            }
            SimplePlayback::Stop => {
                self.started_at = Some(align_to_second(now));
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
    pub fn state(&self, now: i64, name: &str) -> Value {
        json!({
            "duration": self.duration,
            "current": self.current(now),
            "playback": self.playback,
            "direction": self.direction,
            "name": name,
        })
    }
}

/// What an `auxtimer` action asks of one timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxCommand {
    Start,
    Pause,
    Stop,
    SetTime(i64),
    AddTime(i64),
    Direction(SimpleDirection),
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

    /// Carries out one command on timer `index` (1, 2 or 3) and returns its
    /// state afterwards, which is what upstream replies with.
    pub fn run(
        &mut self,
        index: usize,
        command: AuxCommand,
        now: i64,
        name: &str,
    ) -> Option<Value> {
        let timer = self.get_mut(index)?;
        match command {
            AuxCommand::Start => timer.start(now),
            AuxCommand::Pause => timer.pause(now),
            AuxCommand::Stop => timer.stop(),
            AuxCommand::SetTime(millis) => timer.set_time(millis),
            AuxCommand::AddTime(millis) => timer.add_time(millis),
            AuxCommand::Direction(direction) => timer.set_direction(direction, now),
        }
        Some(timer.state(now, name))
    }

    /// The `auxtimer1`/`auxtimer2`/`auxtimer3` slice of the runtime store.
    /// The names live in the settings, so the caller passes them in.
    pub fn snapshot(&self, now: i64, names: &AuxTimerNames) -> Value {
        json!({
            "auxtimer1": self.timers[0].state(now, &names[0]),
            "auxtimer2": self.timers[1].state(now, &names[1]),
            "auxtimer3": self.timers[2].state(now, &names[2]),
        })
    }
}
