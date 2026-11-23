//! A room learning its own name, which it needs to report to the registry.

use worker::*;

use ontime_core::registry::{is_valid_room, DEFAULT_ROOM};

use crate::ROOM_HEADER;

/// The name this room was addressed by.
///
/// A Durable Object is created from its name but never told it, so the Worker
/// stamps it on every request it rebuilds. The one request it passes through
/// untouched — a websocket upgrade at `/ws?room=` — carries it in the query,
/// exactly as the Worker routed it.
pub fn room_name(req: &Request) -> String {
    if let Ok(Some(name)) = req.headers().get(ROOM_HEADER) {
        if is_valid_room(&name) {
            return name;
        }
    }
    req.url()
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "room")
                .map(|(_, value)| value.to_string())
        })
        .filter(|name| is_valid_room(name))
        .unwrap_or_else(|| DEFAULT_ROOM.to_string())
}
