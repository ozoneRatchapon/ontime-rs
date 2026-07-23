//! Browser bindings for the ontime timer engine.
//!
//! The server sends playback state only when it *changes*. Between those packets
//! the browser advances the timer itself by calling [`OntimeTimer::tick`] once per
//! animation frame, running the same `ontime-core` code the server runs — so the
//! two cannot drift apart the way a reimplementation in TypeScript would.

use ontime_core::runtime::{Playback, TimerInputs};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

/// Milliseconds in a day; a time of day is always within this range.
const MS_PER_DAY: i64 = 86_400_000;

/// How quickly the clock estimate follows a worse sample, as a divisor.
///
/// A packet's `serverNow - clientNow` is biased by however long the packet took
/// to arrive, and that bias is always in the same direction. The largest sample
/// therefore came from the fastest packet and is the best estimate, so a better
/// one is taken at once while a worse one is approached slowly — otherwise one
/// congested packet would drag the whole clock backwards.
const SKEW_DECAY: i64 = 16;

/// Folds a new observation into the running estimate of how far the browser's
/// clock is from the server's.
fn blend_skew(current: Option<i64>, sample: i64) -> i64 {
    match current {
        None => sample,
        Some(current) if sample > current => sample,
        Some(current) => {
            let gap = sample - current;
            let step = gap / SKEW_DECAY;
            // integer division stalls once the gap is smaller than the divisor,
            // which would leave the estimate permanently a few milliseconds short
            current + if step == 0 { gap.signum() } else { step }
        }
    }
}

#[wasm_bindgen]
pub struct OntimeTimer {
    inputs: Option<TimerInputs>,
    tz_offset: i64,
    /// `serverNow - clientNow`, estimated across packets rather than taken from
    /// the last one. Applied to every subsequent frame so a client whose system
    /// clock is off still counts the same event boundaries as the server.
    skew: i64,
    /// False until the first packet, so the first sample is taken as-is.
    has_skew: bool,
}

impl Default for OntimeTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl OntimeTimer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inputs: None,
            tz_offset: 0,
            skew: 0,
            has_skew: false,
        }
    }

    /// Feeds the `_engine` block of a `runtime-data` packet.
    ///
    /// `client_now` is the browser's `Date.now()` at the moment the packet arrived.
    /// Returns `false` if the block could not be read, in which case the caller
    /// should keep rendering whatever the server sent.
    pub fn update(&mut self, engine_json: &str, client_now: f64) -> bool {
        let Ok(engine) = serde_json::from_str::<Value>(engine_json) else {
            return false;
        };
        let Some(inputs) = engine.get("inputs") else {
            return false;
        };
        let Ok(inputs) = serde_json::from_value::<TimerInputs>(inputs.clone()) else {
            return false;
        };

        let server_now = engine.get("serverNow").and_then(Value::as_i64).unwrap_or(0);
        let sample = server_now - client_now as i64;
        self.skew = blend_skew(self.has_skew.then_some(self.skew), sample);
        self.has_skew = true;
        self.tz_offset = engine.get("tzOffset").and_then(Value::as_i64).unwrap_or(0);
        self.inputs = Some(inputs);
        true
    }

    /// The `TimerState` for this frame, as JSON. Empty string when no packet has
    /// been received yet.
    pub fn tick(&self, client_now: f64) -> String {
        let Some(inputs) = &self.inputs else {
            return String::new();
        };
        inputs
            .timer_state(self.now(client_now), self.tz_offset)
            .to_string()
    }

    /// Milliseconds since local midnight, for the wall clock display.
    pub fn clock(&self, client_now: f64) -> f64 {
        (self.now(client_now) + self.tz_offset).rem_euclid(MS_PER_DAY) as f64
    }

    /// Whether the timer is advancing, so the caller knows whether there is
    /// anything to recompute between packets.
    pub fn is_running(&self) -> bool {
        self.inputs
            .as_ref()
            .map(|i| matches!(i.playback, Playback::Play | Playback::Roll))
            .unwrap_or(false)
    }

    /// Diagnostics: how far the browser clock is from the server's, in milliseconds.
    pub fn skew(&self) -> f64 {
        self.skew as f64
    }

    fn now(&self, client_now: f64) -> i64 {
        client_now as i64 + self.skew
    }
}

/// Version marker, so the client can confirm which engine build it loaded.
#[wasm_bindgen]
pub fn engine_info() -> String {
    json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{blend_skew, SKEW_DECAY};

    #[test]
    fn the_first_sample_is_taken_as_it_comes() {
        assert_eq!(blend_skew(None, -120), -120);
    }

    #[test]
    fn a_faster_packet_is_believed_at_once() {
        // less latency means serverNow - clientNow is larger and closer to the truth
        assert_eq!(blend_skew(Some(-120), -40), -40);
    }

    #[test]
    fn a_slower_packet_barely_moves_the_estimate() {
        // one congested packet must not drag the clock backwards
        let moved = blend_skew(Some(-40), -400);
        // it closes a sixteenth of the gap, not the whole of it
        assert_eq!(moved, -40 + (-400 - -40) / SKEW_DECAY);
        assert!(
            (moved - -40).abs() < (-400 - -40i64).abs() / 8,
            "a slow packet moved the estimate to {moved}"
        );
    }

    #[test]
    fn repeated_slow_packets_still_converge_so_real_drift_is_followed() {
        let mut skew = Some(0);
        for _ in 0..200 {
            skew = Some(blend_skew(skew, -500));
        }
        assert!(
            (skew.unwrap() - -500).abs() < 5,
            "did not converge, ended at {:?}",
            skew
        );
    }

    #[test]
    fn jitter_around_a_stable_clock_settles_near_the_best_sample() {
        // latency varies between 20ms and 300ms; the truth is the fastest sample
        let samples = [-300, -80, -250, -20, -180, -40, -220, -30];
        let mut skew = None;
        for sample in samples {
            skew = Some(blend_skew(skew, sample));
        }
        let settled = skew.unwrap();
        assert!(
            settled >= -60,
            "settled at {settled}, further from the truth than the best sample"
        );
    }
}
