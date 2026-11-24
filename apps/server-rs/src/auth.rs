//! `/auth/*` — a person signing a device in with an invite link, and asking
//! who they are. Public, but every route needs an invite code or a session.
//!
//! - `POST /auth/redeem`  `{ code }` -> `{ token, person }`
//! - `GET  /auth/me`      Bearer session -> `{ person, rooms }`
//! - `POST /auth/logout`  Bearer session
//! - `/auth/rooms/<room>/…` an owner managing their room's operators (Bearer
//!   session), see `ontime_core::directory`
//!
//! The session token travels to the registry in the body or a header, never in
//! a URL.

use serde_json::{json, Value};
use worker::*;

use ontime_core::identity::device_label;

use crate::{bearer_token, registry_host};

pub async fn handle(mut req: Request, env: &Env, path: &str) -> Result<Response> {
    let method = req.method();
    let rest = path.trim_start_matches("/auth").trim_start_matches('/');

    if rest.starts_with("rooms/") {
        let Some(session) = bearer_token(&req) else {
            return Response::error("This device is not signed in", 401);
        };
        let body = match method {
            Method::Post => Some(req.text().await.unwrap_or_default()),
            Method::Get | Method::Delete => None,
            _ => return Response::error("Method not allowed", 405),
        };
        let response =
            registry_host::forward_as(env, method, &format!("auth/{rest}"), body, Some(&session))
                .await?;
        return private(response).await;
    }

    let body = match (method, rest) {
        (Method::Post, "redeem") => {
            let sent = req.json::<Value>().await.unwrap_or(Value::Null);
            let user_agent = req
                .headers()
                .get("User-Agent")
                .ok()
                .flatten()
                .unwrap_or_default();
            json!({ "code": sent.get("code"), "device": device_label(&user_agent) })
        }
        (Method::Get, "me") | (Method::Post, "logout") => match bearer_token(&req) {
            Some(token) => json!({ "token": token }),
            None => return Response::error("This device is not signed in", 401),
        },
        (_, "redeem" | "me" | "logout") => return Response::error("Method not allowed", 405),
        _ => return Response::error("Not found", 404),
    };

    let target = format!("auth/{rest}");
    let response =
        registry_host::forward(env, Method::Post, &target, Some(body.to_string())).await?;
    private(response).await
}

/// Nothing a person is told about themselves may be cached along the way.
async fn private(mut response: Response) -> Result<Response> {
    let mut headers = Headers::new();
    headers.set("content-type", "application/json")?;
    headers.set("cache-control", "no-store")?;
    let status = response.status_code();
    Ok(Response::from_bytes(response.bytes().await?)?
        .with_headers(headers)
        .with_status(status))
}
