//! The integration API: one way in for everything outside the editor.
//!
//! Bitfocus Companion, a Stream Deck, a show-control script or `curl` drive a
//! room with `(action, payload)` pairs, as upstream's `api-integration`:
//!
//! ```text
//! HTTP  GET /api/start/index/3      ─┐  http_call   ─┐
//! WS    {"tag":"start",             ─┼───────────────┼─► parse ─► Action ─► dispatch ─► Reply
//!        "payload":{"index":3}}      ┘               ┘                  (project)   (payload, Change)
//! ```
//!
//! A transport only turns its own input into `(tag, payload)`; parsing,
//! validation and the error messages are shared, so the two cannot accept
//! different things. `dispatch` changes nothing outside the project: what has
//! to happen next (store, broadcast, message a socket) comes back as a
//! [`Change`] for the host to carry out.

mod action;
mod change;
mod colour;
mod dispatch;
pub mod js;
mod path;
mod types;

pub use action::{parse, Action};
pub use change::patch_for as change_patch;
pub use dispatch::dispatch;
pub use path::{http_call, payload_from_path, HttpCall, HELLO_MESSAGE};
pub use types::{Change, ClientCommand, IntegrationError, Reply};

use serde_json::Value;

use crate::project::Project;

/// Parses and carries out one action: what every transport calls.
pub fn handle(
    project: &mut Project,
    tag: &str,
    payload: Option<&Value>,
    now: i64,
) -> Result<Reply, IntegrationError> {
    dispatch(project, parse(tag, payload)?, now)
}
