//! Websocket packets the room answers on its own, without touching the project.
use serde_json::{json, Value};

use crate::runtime::time_of_day;

/// Reply to a Ping: the client's payload back, plus the room's time of day
/// (ms since local midnight) so the client can measure its clock offset.
pub fn pong(echo: Value, now: i64, tz_offset: i64) -> Value {
    json!({ "tag": "pong", "payload": echo, "clock": time_of_day(now, tz_offset) })
}
