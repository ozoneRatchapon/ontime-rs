//! Hosts `ontime_core::guard` on the Worker (plan 019).
//!
//! A room guards its own door in its own memory. The admin routes run in the
//! stateless Worker, which has no memory worth the name, so they ask the
//! registry instance, at [`GUARD_PATH`], to keep their count.

use serde_json::{json, Value};
use worker::*;

use ontime_core::guard::{client_keys, fingerprint, Guard};
use ontime_core::project::hash_password;

use crate::registry_host::{self, REGISTRY_PATH};

/// Where the registry instance keeps the admin routes' count.
pub const GUARD_PATH: &str = "/registry/guard";

/// The master password's token, if one is configured.
pub(crate) fn master_token(env: &Env) -> Option<String> {
    env.secret("SESSION_PASSWORD")
        .ok()
        .map(|secret| hash_password(&secret.to_string()))
}

/// Who is asking, as the guard's keys. Cloudflare sets `CF-Connecting-IP` on every request it
/// passes to the Worker, overwriting anything the caller sent under the name,
/// and the Worker hands the header on to the Durable Object.
pub(crate) fn client(req: &Request) -> Vec<String> {
    let address = req.headers().get("CF-Connecting-IP").ok().flatten();
    client_keys(address.as_deref())
}

/// `429` with the wait in `Retry-After`.
pub(crate) fn too_many(seconds: u64) -> Result<Response> {
    let mut response =
        Response::from_json(&json!({ "message": "Too many requests" }))?.with_status(429);
    response
        .headers_mut()
        .set("Retry-After", &seconds.to_string())?;
    Ok(response)
}

/// `POST /registry/guard` `{ keys, ok, guess }`, inside the registry instance,
/// `guess` being the attempt's fingerprint in hex:
/// `200` to go on, or `429` while `key` is locked out.
pub(crate) async fn serve(guard: &mut Guard, mut req: Request, now: i64) -> Result<Response> {
    if req.method() != Method::Post {
        return Response::error("Method not allowed", 405);
    }
    let body = req.json::<Value>().await.unwrap_or(Value::Null);
    let guess = body
        .get("guess")
        .and_then(Value::as_str)
        .and_then(|hex| u64::from_str_radix(hex, 16).ok());
    let keys: Option<Vec<String>> = body
        .get("keys")
        .and_then(|keys| serde_json::from_value(keys.clone()).ok())
        .filter(|keys: &Vec<String>| !keys.is_empty());
    let (Some(keys), Some(ok), Some(guess)) =
        (keys, body.get("ok").and_then(Value::as_bool), guess)
    else {
        return Response::error("Missing keys, ok or guess", 400);
    };
    match guard.attempt(&keys, ok, guess, now) {
        Ok(()) => Response::from_json(&json!({})),
        Err(seconds) => too_many(seconds),
    }
}

/// Asks the registry whether `client` may have its answer to `attempt`,
/// counting it when `ok` is false. `Err(seconds)` while it is locked out. Only
/// the attempt's fingerprint leaves the Worker.
pub(crate) async fn ask(
    env: &Env,
    client: &[String],
    attempt: &str,
    ok: bool,
) -> Result<std::result::Result<(), u64>> {
    let rest = GUARD_PATH.trim_start_matches(REGISTRY_PATH);
    let guess = format!("{:x}", fingerprint(attempt));
    let body = json!({ "keys": client, "ok": ok, "guess": guess }).to_string();
    let response = registry_host::forward(env, Method::Post, rest, Some(body)).await?;
    match response.status_code() {
        200 => Ok(Ok(())),
        429 => {
            let seconds = response
                .headers()
                .get("Retry-After")?
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            Ok(Err(seconds))
        }
        status => Err(Error::RustError(format!("the guard answered {status}"))),
    }
}
