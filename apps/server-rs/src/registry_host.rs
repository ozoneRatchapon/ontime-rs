//! Hosts the room registry and lets rooms report into it.
//!
//! The registry is one instance of the same Durable Object class as the rooms,
//! under a name no room can have (`@registry`). That keeps it strongly
//! consistent and needs no extra binding — the room logic simply never runs
//! for it, because its requests are recognised before a project is loaded.
//!
//! The rules live in `ontime_core::registry`; this only loads, stores and
//! carries requests.

use serde_json::Value;
use worker::*;

use ontime_core::registry::{self, Registry, RoomReport, REGISTRY_NAME};

/// The path prefix the registry instance answers under.
pub const REGISTRY_PATH: &str = "/registry/";

const STORAGE_KEY: &str = "registry_v1";

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// Serves a registry request inside the registry's own Durable Object.
pub async fn serve(state: &State, mut req: Request) -> Result<Response> {
    let mut storage = state.storage();
    let mut document: Registry = storage
        .get::<String>(STORAGE_KEY)
        .await
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    let path = req.path();
    let segments: Vec<&str> = path
        .trim_start_matches(REGISTRY_PATH)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    let method = req.method();
    let body = match method {
        Method::Get | Method::Head | Method::Delete => Value::Null,
        _ => req.json::<Value>().await.unwrap_or(Value::Null),
    };

    let response = registry::handle(&mut document, method.as_ref(), &segments, &body, now_ms());

    if response.changed {
        let raw = serde_json::to_string(&document)
            .map_err(|e| Error::RustError(format!("Could not serialise the registry: {e}")))?;
        storage.put(STORAGE_KEY, raw).await?;
    }

    Ok(Response::from_json(&response.body)?.with_status(response.status))
}

fn registry_stub(env: &Env) -> Result<Stub> {
    env.durable_object("ONTIME_PROJECT")?
        .id_from_name(REGISTRY_NAME)?
        .get_stub()
}

/// Passes a request to the registry, at `/registry/<rest>`.
pub async fn forward(env: &Env, method: Method, rest: &str, body: Option<String>) -> Result<Response> {
    let mut headers = Headers::new();
    headers.set("content-type", "application/json")?;
    let request = Request::new_with_init(
        &format!("https://registry.internal{REGISTRY_PATH}{}", rest.trim_start_matches('/')),
        &RequestInit {
            method,
            headers,
            body: body.map(Into::into),
            ..Default::default()
        },
    )?;
    registry_stub(env)?.fetch_with_request(request).await
}

/// A room telling the registry about itself.
pub async fn report(env: &Env, report: &RoomReport) -> Result<()> {
    let body = serde_json::to_string(report)
        .map_err(|e| Error::RustError(format!("Could not serialise the report: {e}")))?;
    forward(env, Method::Post, "report", Some(body)).await?;
    Ok(())
}
