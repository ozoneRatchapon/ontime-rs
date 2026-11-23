//! Hosts the room registry and lets rooms report into it.
//!
//! The registry is one instance of the same Durable Object class as the rooms,
//! under a name no room can have (`@registry`). That keeps it strongly
//! consistent and needs no extra binding — the room logic simply never runs
//! for it, because its requests are recognised before a project is loaded.
//!
//! The rules live in `ontime_core::directory`; this only loads, stores and
//! carries requests.

use serde_json::Value;
use worker::*;

use ontime_core::directory::{self, Directory};
use ontime_core::registry::{Registry, RoomReport, REGISTRY_NAME};
use ontime_core::identity::Identity;
use uuid::Uuid;

/// The path prefix the registry instance answers under.
pub const REGISTRY_PATH: &str = "/registry/";

const ROOMS_KEY: &str = "registry_v1";
const IDENTITY_KEY: &str = "identity_v1";

fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

/// 244 bits from the platform's crypto RNG (`crypto.getRandomValues`).
fn fresh_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn load<T: serde::de::DeserializeOwned + Default>(storage: &Storage, key: &str) -> T {
    storage
        .get::<String>(key)
        .await
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

async fn store<T: serde::Serialize>(storage: &mut Storage, key: &str, value: &T) -> Result<()> {
    let raw = serde_json::to_string(value)
        .map_err(|e| Error::RustError(format!("Could not serialise {key}: {e}")))?;
    storage.put(key, raw).await
}

/// Serves a registry request inside the registry's own Durable Object.
pub async fn serve(state: &State, mut req: Request) -> Result<Response> {
    let mut storage = state.storage();
    let mut document = Directory {
        rooms: load::<Registry>(&storage, ROOMS_KEY).await,
        identity: load::<Identity>(&storage, IDENTITY_KEY).await,
    };
    let before = document.clone();

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

    let response = directory::handle(&mut document, method.as_ref(), &segments, &body, now_ms(), &mut fresh_secret);

    // each document is written only when it actually moved
    if response.changed && document.rooms != before.rooms {
        store(&mut storage, ROOMS_KEY, &document.rooms).await?;
    }
    if response.changed && document.identity != before.identity {
        store(&mut storage, IDENTITY_KEY, &document.identity).await?;
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
