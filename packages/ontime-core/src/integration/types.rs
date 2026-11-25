//! What the integration dispatcher hands back to a transport.

use serde_json::Value;

/// A refused action, with upstream's message for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationError {
    pub message: String,
}

impl IntegrationError {
    pub fn new(message: impl Into<String>) -> Self {
        IntegrationError {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for IntegrationError {}

/// Something only the host can do, because it owns the sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCommand {
    Rename { target: String, name: String },
    Redirect { target: String, path: String },
    Identify { target: String, identify: bool },
}

/// What a dispatched action changed, for the host to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Nothing to store or tell anyone: a read, or a command that was already
    /// in effect.
    None,
    /// Playback, the overlay, the aux timers or the teleprompter: store the
    /// project, send `runtime-data` and reconsider the alarm.
    Runtime,
    /// An entry of this rundown was edited: store, tell clients to refetch it,
    /// and send `runtime-data`, which may show the edited event.
    Rundown {
        rundown_id: String,
    },
    Client(ClientCommand),
}

/// The answer to an action: the payload upstream replies with, and what changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub payload: Value,
    pub change: Change,
}

impl Reply {
    pub fn new(payload: Value, change: Change) -> Self {
        Reply { payload, change }
    }

    /// Upstream's `{ payload: 'success' }`.
    pub fn success(change: Change) -> Self {
        Reply::new(Value::String("success".to_string()), change)
    }
}
