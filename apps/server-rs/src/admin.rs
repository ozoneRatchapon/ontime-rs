//! `/admin/*` — the operator's view across rooms, gated by the master password.
//!
//! - `POST   /admin/login`         `{ password }` -> `{ token }` (the master token)
//! - `GET    /admin/rooms`         every room the registry knows
//! - `PUT    /admin/rooms/<name>`  `{ owner?, note? }`
//! - `PUT    /admin/rooms/<name>`  also `{ owners: [personId] }`
//! - `DELETE /admin/rooms/<name>`  forget a room (its data stays)
//! - `GET    /admin/people`        people, their devices and pending invites
//! - `POST   /admin/people`        `{ name }`
//! - `POST   /admin/people/<id>/invite`    a new invite code
//! - `DELETE /admin/people/<id>/sessions`  sign them out everywhere
//! - `DELETE /admin/people/<id>`           remove them
//! - `GET    /admin/activity`      people events, newest first: `?before=&limit=`
//!
//! Wrong guesses at the password or the token are counted per client by
//! `guard_host`, and a client past its allowance waits (plan 019).
//!
//! With no `SESSION_PASSWORD` configured the admin surface is closed rather than
//! open: unlike creating a room, listing every room is never a default.

use serde_json::{json, Value};
use worker::*;

use ontime_core::project::{hash_password, token_matches};

use crate::guard_host::{self, master_token};
use crate::{bearer_token, registry_host};

fn presented_token(req: &Request) -> Option<String> {
    if let Some(token) = bearer_token(req) {
        return Some(token);
    }
    req.url()
        .ok()?
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, token)| token.to_string())
}

pub async fn handle(mut req: Request, env: &Env, path: &str) -> Result<Response> {
    let Some(master) = master_token(env) else {
        return Response::error(
            "The admin page needs SESSION_PASSWORD to be set on the Worker",
            403,
        );
    };

    let method = req.method();
    let rest = path.trim_start_matches("/admin").trim_start_matches('/');

    if rest == "login" {
        if method != Method::Post {
            return Response::error("Method not allowed", 405);
        }
        let body = req.json::<Value>().await.unwrap_or(Value::Null);
        let password = body
            .get("password")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let ok = token_matches(&master, &hash_password(password));
        if let Err(seconds) = guard_host::ask(env, &guard_host::client(&req), password, ok).await? {
            return guard_host::too_many(seconds);
        }
        return match ok {
            true => Response::from_json(&json!({ "token": master })),
            false => Response::error("Wrong password", 401),
        };
    }

    // the master token is as good as the password, so a guess at it is
    // counted the same; asking without one guesses nothing
    let Some(token) = presented_token(&req) else {
        return Response::error("Unauthorized", 401);
    };
    let authorised = token_matches(&master, &token);
    if let Err(seconds) =
        guard_host::ask(env, &guard_host::client(&req), &token, authorised).await?
    {
        return guard_host::too_many(seconds);
    }
    if !authorised {
        return Response::error("Unauthorized", 401);
    }

    // only the room list, people and their activity are reachable from here;
    // `report` is for rooms and `auth` has its own front door
    let first = rest.split('/').next().unwrap_or_default();
    if !matches!(first, "rooms" | "people" | "activity") {
        return Response::error("Not found", 404);
    }
    if rest == "activity" {
        if method != Method::Get {
            return Response::error("Method not allowed", 405);
        }
        let query = req
            .url()?
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        return registry_host::forward(env, method, &format!("activity{query}"), None).await;
    }

    let body = match method {
        Method::Get | Method::Head | Method::Delete => None,
        _ => Some(req.text().await.unwrap_or_default()),
    };
    registry_host::forward(env, method, rest, body).await
}
