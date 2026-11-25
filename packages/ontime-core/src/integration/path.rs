//! HTTP requests into `(action, payload)`, as upstream's `integration.router.ts`
//! and `integrationPayloadFromPath` build them.
//!
//! `GET /api/start/index/3` is `("start", {"index": "3"})`; with a query,
//! `GET /api/message/timer?text=hi` is `("message", {"timer": {"text": "hi"}})`.
//! Path segments are taken as they arrive, still percent-encoded (Express's
//! `req.path`); query values are decoded.

use serde_json::{Map, Value};

use super::js::decode_query_component;
use super::types::IntegrationError;

/// Query parameters the host uses itself, which never reach an action: the
/// room's token and name. Upstream has neither, so a `?token=` would otherwise
/// turn `start` into "start with a payload" and fail.
const TRANSPORT_PARAMETERS: &[&str] = &["token", "room"];

/// What `GET /api/...` asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum HttpCall {
    /// `GET /api` or `/api/`: the greeting that tells a client it found us.
    Hello,
    Action {
        tag: String,
        payload: Option<Value>,
    },
}

pub const HELLO_MESSAGE: &str = "You have reached Ontime API server";

/// `path` is what follows `/api`, such as `/start/next`; `query` is the raw
/// query string without its `?`.
pub fn http_call(path: &str, query: Option<&str>) -> Result<HttpCall, IntegrationError> {
    if path.is_empty() || path == "/" {
        return Ok(HttpCall::Hello);
    }
    let action = path.strip_prefix('/').unwrap_or(path);
    if action.is_empty() {
        return Err(IntegrationError::new("No action found"));
    }

    let query = query.and_then(query_object);
    let mut segments = action.split('/');
    let tag = segments.next().unwrap_or_default().to_string();
    let rest: Vec<&str> = segments.collect();
    let payload = match rest.is_empty() {
        true => query,
        false => Some(payload_from_path(&rest, query)),
    };
    Ok(HttpCall::Action { tag, payload })
}

/// Upstream `integrationPayloadFromPath`: nests the path into objects and puts
/// `value` at the end, or the last segment when there is no value. `null` when
/// that builds no object.
pub fn payload_from_path(path: &[&str], value: Option<Value>) -> Value {
    if let [key] = path {
        return match value {
            None => Value::String(key.to_string()),
            Some(value) => Value::Object(Map::from_iter([(key.to_string(), value)])),
        };
    }

    let (innermost, keys) = match value {
        Some(value) => (Some(value), path),
        None => match path.split_last() {
            Some((last, keys)) => (Some(Value::String(last.to_string())), keys),
            None => (None, path),
        },
    };
    let nested = keys.iter().rev().fold(innermost, |inner, key| {
        let mut object = Map::new();
        // JSON.stringify drops an undefined value, so `{key: undefined}` is `{}`
        if let Some(inner) = inner {
            object.insert(key.to_string(), inner);
        }
        Some(Value::Object(object))
    });
    match nested {
        Some(object @ Value::Object(_)) => object,
        _ => Value::Null,
    }
}

/// The query as Express hands it over: decoded values, a repeated key as a
/// list. `None` when empty, as upstream's `isEmptyObject` check. Bracket
/// keys (`a[b]=c`) are kept as plain keys; nothing in the API uses them.
fn query_object(query: &str) -> Option<Value> {
    let mut object = Map::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode_query_component(key);
        if key.is_empty() || TRANSPORT_PARAMETERS.contains(&key.as_str()) {
            continue;
        }
        let value = Value::String(decode_query_component(value));
        match object.get_mut(&key) {
            Some(Value::Array(list)) => list.push(value),
            Some(existing) => *existing = Value::Array(vec![existing.take(), value]),
            None => {
                object.insert(key, value);
            }
        }
    }
    (!object.is_empty()).then_some(Value::Object(object))
}
