//! Browser bindings for the ontime timer engine.
//!
//! The server sends playback state only when it *changes*. Between those packets
//! the browser advances the timer itself by calling [`OntimeTimer::tick`] once per
//! animation frame, running the same `ontime-core` code the server runs — so the
//! two cannot drift apart the way a reimplementation in TypeScript would.

use ontime_core::runtime::TimerInputs;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

/// Milliseconds in a day; a time of day is always within this range.
const MS_PER_DAY: i64 = 86_400_000;

#[wasm_bindgen]
pub struct OntimeTimer {
    inputs: Option<TimerInputs>,
    tz_offset: i64,
    /// `serverNow - clientNow`, measured when the last packet arrived.
    /// Applied to every subsequent frame so a client whose system clock is off
    /// still counts the same event boundaries as the server.
    skew: i64,
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
        self.skew = server_now - client_now as i64;
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

    /// Whether the timer is advancing. When it is not, the caller can stop
    /// requesting frames.
    pub fn is_running(&self) -> bool {
        self.inputs
            .as_ref()
            .and_then(|i| i.elapsed(self.now(0.0)).map(|_| i))
            .map(|i| i.ends_at().is_some())
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
