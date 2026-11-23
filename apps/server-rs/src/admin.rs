//! `/admin/*` — the operator's view across rooms, gated by the master password.
//!
//! - `POST   /admin/login`         `{ password }` -> `{ token }` (the master token)
//! - `GET    /admin/rooms`         every room the registry knows
//! - `PUT    /admin/rooms/<name>`  `{ owner?, note? }`
//! - `DELETE /admin/rooms/<name>`  forget a room (its data stays)
//!
//! With no `SESSION_PASSWORD` configured the admin surface is closed rather than
//! open: unlike creating a room, listing every room is never a default.

use serde_json::{json, Value};
use worker::*;

use ontime_core::project::{hash_password, token_matches};

use crate::registry_host;

fn master_token(env: &Env) -> Option<String> {
    env.secret("SESSION_PASSWORD")
        .ok()
        .map(|secret| hash_password(&secret.to_string()))
}

fn presented_token(req: &Request) -> Option<String> {
    if let Ok(Some(auth)) = req.headers().get("Authorization") {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            return Some(token.trim().to_string());
        }
    }
    req.url()
        .ok()?
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, token)| token.to_string())
}

pub async fn handle(mut req: Request, env: &Env, path: &str) -> Result<Response> {
    let Some(master) = master_token(env) else {
        return Response::error("The admin page needs SESSION_PASSWORD to be set on the Worker", 403);
    };

    let method = req.method();
    let rest = path.trim_start_matches("/admin").trim_start_matches('/');

    if rest == "login" {
        if method != Method::Post {
            return Response::error("Method not allowed", 405);
        }
        let body = req.json::<Value>().await.unwrap_or(Value::Null);
        let password = body.get("password").and_then(Value::as_str).unwrap_or_default();
        return match token_matches(&master, &hash_password(password)) {
            true => Response::from_json(&json!({ "token": master })),
            false => Response::error("Wrong password", 401),
        };
    }

    let authorised = presented_token(&req).is_some_and(|token| token_matches(&master, &token));
    if !authorised {
        return Response::error("Unauthorized", 401);
    }

    // only the room list is reachable from outside; `report` is for rooms
    if !(rest == "rooms" || rest.starts_with("rooms/")) {
        return Response::error("Not found", 404);
    }

    let body = match method {
        Method::Get | Method::Head | Method::Delete => None,
        _ => Some(req.text().await.unwrap_or_default()),
    };
    registry_host::forward(env, method, rest, body).await
}
