mod room;

use worker::*;

pub use room::OntimeProject;

/// Every room is a Durable Object instance; a single project lives in one room.
const DEFAULT_ROOM: &str = "default_room";

/// Rebuilds the response with CORS headers.
///
/// A response that came back from a Durable Object stub carries immutable
/// headers, so they have to be copied into a fresh `Headers` rather than
/// mutated in place.
async fn with_cors(mut res: Response) -> Result<Response> {
    // a websocket upgrade has no body to read and needs no CORS headers
    if res.status_code() == 101 {
        return Ok(res);
    }

    let status = res.status_code();
    let mut headers = Headers::new();
    for (key, value) in res.headers().entries() {
        headers.set(&key, &value)?;
    }
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set(
        "Access-Control-Allow-Methods",
        "GET, POST, PUT, PATCH, DELETE, OPTIONS",
    )?;
    headers.set("Access-Control-Allow-Headers", "*")?;

    let body = res.bytes().await?;
    Ok(ResponseBuilder::new()
        .with_status(status)
        .with_headers(headers)
        .fixed(body))
}

/// A room name has to be safe to put in a URL and stable as a Durable Object key.
fn is_valid_room(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Where a request's room came from, and what is left of the path.
struct Routed {
    room: String,
    /// Path with the `/r/<room>` prefix removed, so the Durable Object always
    /// sees the same shape regardless of how the room was addressed.
    path: String,
}

/// Resolves the room, in order of precedence:
///
/// 1. a `/r/<room>/…` path prefix — used by REST, because the client composes
///    request paths by appending to `serverURL` and cannot carry a query string
/// 2. `?room=` — used by the websocket, which has no path to spare
/// 3. the shared default room
fn route(path: &str, url: &Url) -> Routed {
    if let Some(rest) = path.strip_prefix("/r/") {
        let (room, remainder) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        };
        if is_valid_room(room) {
            return Routed {
                room: room.to_string(),
                path: if remainder.is_empty() {
                    "/".to_string()
                } else {
                    remainder.to_string()
                },
            };
        }
    }

    let room = url
        .query_pairs()
        .find(|(key, _)| key == "room")
        .map(|(_, value)| value.to_string())
        .filter(|room| is_valid_room(room))
        .unwrap_or_else(|| DEFAULT_ROOM.to_string());

    Routed {
        room,
        path: path.to_string(),
    }
}

/// Hands the request to the room's Durable Object, which owns all project state.
///
/// The room prefix is stripped before forwarding so every room's Durable Object
/// receives identical paths — it never needs to know how it was addressed.
async fn forward(mut req: Request, env: &Env, routed: &Routed) -> Result<Response> {
    let stub = env
        .durable_object("ONTIME_PROJECT")?
        .id_from_name(&routed.room)?
        .get_stub()?;

    let original = req.url()?;
    if original.path() == routed.path {
        // nothing to rewrite — notably the websocket upgrade, which must be passed
        // through untouched for the runtime to recognise it
        return stub.fetch_with_request(req).await;
    }

    let mut url = original;
    url.set_path(&routed.path);

    let method = req.method();
    let headers = req.headers().clone();
    let body = match method {
        Method::Get | Method::Head | Method::Options => None,
        // the body cannot be moved across a rebuilt request as a stream
        _ => Some(req.bytes().await.unwrap_or_default().into()),
    };

    let forwarded = Request::new_with_init(
        url.as_str(),
        &RequestInit {
            method,
            headers,
            body,
            ..Default::default()
        },
    )?;

    stub.fetch_with_request(forwarded).await
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    if req.method() == Method::Options {
        return with_cors(Response::empty()?).await;
    }

    let Ok(url) = req.url() else {
        return with_cors(Response::error("Bad request", 400)?).await;
    };
    let path = url.path().to_string();
    let routed = route(&path, &url);

    let result = match routed.path.as_str() {
        "/" | "/api/health" => Response::ok("ontime-server-rs online"),
        // everything the client talks to is project state, so it all goes to the DO
        p if p == "/ws" || p.starts_with("/data/") || p.starts_with("/user/") => {
            forward(req, &env, &routed).await
        }
        _ => Response::error("Not found", 404),
    };

    match result {
        Ok(res) => with_cors(res).await,
        Err(err) => with_cors(Response::error(format!("WORKER_ERROR: {err}"), 500)?).await,
    }
}
