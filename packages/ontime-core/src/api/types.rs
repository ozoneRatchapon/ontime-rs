//! The transport-agnostic request, response and effects a host translates to and from.

use serde_json::{json, Value};

use crate::registry::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Other,
}

impl HttpMethod {
    pub fn parse(method: &str) -> Self {
        match method.to_ascii_uppercase().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            _ => HttpMethod::Other,
        }
    }
}

pub struct ApiRequest {
    pub method: HttpMethod,
    /// Path from the API root. A host prefix is tolerated and trimmed.
    pub path: String,
    /// Parsed body; an empty object when there was none.
    pub body: Value,
    /// Bearer token, however the host chose to carry it.
    pub token: Option<String>,
    /// A master token that overrides a room's own, if the host has one configured.
    pub master_token: Option<String>,
    /// True when this room has never been used before.
    ///
    /// Any name creates a room on first use, which is fine for one operator and
    /// not fine for a public deployment: names can be squatted and abandoned
    /// rooms accumulate. When a master token is configured, creating one is
    /// gated behind it; with no master configured nothing changes.
    pub room_is_new: bool,
    /// What the signed-in person presenting a session is to this room, as the
    /// host looked it up. `None` for anyone not signed in or not a member.
    pub member: Option<Role>,
}

/// Something the host must do once a request has been handled.
///
/// Returned rather than performed, because persisting and broadcasting look
/// entirely different on a Durable Object and on a native server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write the project down.
    Persist,
    /// Tell connected clients that a slice of state changed.
    Refetch {
        target: String,
        rundown_id: Option<String>,
    },
    /// Reconsider when to wake next.
    Reschedule,
}

impl Effect {
    pub(crate) fn refetch(target: &str, rundown_id: Option<String>) -> Self {
        Effect::Refetch {
            target: target.to_string(),
            rundown_id,
        }
    }
}

pub struct ApiResponse {
    pub status: u16,
    /// `None` for a body-less response such as 204, or when `text` is set.
    pub body: Option<Value>,
    /// Set when the body is not JSON, as the CSS override is not.
    pub text: Option<String>,
    pub effects: Vec<Effect>,
}

impl ApiResponse {
    pub fn json<T: serde::Serialize>(value: &T) -> Self {
        Self {
            status: 200,
            body: Some(serde_json::to_value(value).unwrap_or(Value::Null)),
            text: None,
            effects: Vec::new(),
        }
    }

    pub fn status<T: serde::Serialize>(status: u16, value: &T) -> Self {
        Self {
            status,
            ..Self::json(value)
        }
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: None,
            text: Some(body.into()),
            effects: Vec::new(),
        }
    }

    pub fn no_content() -> Self {
        Self {
            status: 204,
            body: None,
            text: None,
            effects: Vec::new(),
        }
    }

    pub fn error(message: impl Into<String>, status: u16) -> Self {
        Self {
            status,
            body: Some(json!({ "message": message.into() })),
            text: None,
            effects: Vec::new(),
        }
    }
}
