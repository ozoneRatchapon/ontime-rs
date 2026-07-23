//! Ontime's backend as a native binary.
//!
//! It shares `ontime-core` with the Cloudflare Worker — the same rundown model,
//! the same timer, the same routing. This crate only supplies what a Worker
//! cannot: a real filesystem to keep the project in, and a process that can hold
//! open sockets itself.
//!
//! Because the routing lives in the core, a route added for one host appears on
//! the other. That is the whole point of the split.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use ontime_core::api::{self, ApiRequest, Effect, HttpMethod};
use ontime_core::project::Project;
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::{Any, CorsLayer};

mod store;

/// One isolated project, matching a Durable Object instance on the Worker.
struct Room {
    project: Project,
    /// Every connected client receives what is published here.
    outbound: broadcast::Sender<String>,
}

impl Room {
    fn new(project: Project) -> Self {
        let (outbound, _) = broadcast::channel(256);
        Self { project, outbound }
    }

    fn publish(&self, packet: Value) {
        // fails only when nobody is listening, which is not an error
        let _ = self.outbound.send(packet.to_string());
    }

    fn broadcast_runtime(&self, now: i64) {
        let Some(rundown) = self.project.current() else {
            return;
        };
        let mut payload = self
            .project
            .runtime
            .snapshot(rundown, now, self.project.timezone_offset);

        if let Some(object) = payload.as_object_mut() {
            if let Some(aux) = self.project.aux_timers.snapshot(now).as_object() {
                object.extend(aux.clone());
            }
            object.insert("message".into(), json!(self.project.message));
        }

        self.publish(json!({ "tag": "runtime-data", "payload": payload }));
    }

    fn broadcast_refetch(&self, target: &str, rundown_id: Option<&str>) {
        let revision = self
            .project
            .current()
            .map(|r| r.revision as i64)
            .unwrap_or(0);
        let payload = match rundown_id {
            Some(id) => json!({ "target": target, "revision": revision, "rundownId": id }),
            None => json!({ "target": target, "revision": revision }),
        };
        self.publish(json!({ "tag": "refetch", "payload": payload }));
    }
}

/// A connected client, as the client list reports it.
#[derive(Clone)]
struct ClientInfo {
    room: String,
    name: String,
    client_type: String,
    identify: bool,
    origin: String,
    path: String,
}

struct AppState {
    rooms: Mutex<HashMap<String, Room>>,
    /// Keyed by client id. Owned here rather than in the core because the
    /// connections belong to the host.
    clients: Mutex<HashMap<String, ClientInfo>>,
    data_dir: PathBuf,
    /// Hash of `SESSION_PASSWORD`, if one is configured.
    master_token: Option<String>,
}

const DEFAULT_ROOM: &str = "default_room";

/// Epoch milliseconds.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn is_valid_room(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(4001);
    let data_dir =
        PathBuf::from(std::env::var("ONTIME_DATA_DIR").unwrap_or_else(|_| "./data".into()));

    // the same rule as the Worker: no password configured means open, which is
    // how a machine on a trusted network is expected to run
    let master_token = std::env::var("SESSION_PASSWORD")
        .ok()
        .filter(|p| !p.is_empty())
        .map(|p| ontime_core::project::hash_password(&p));

    if let Err(error) = tokio::fs::create_dir_all(&data_dir).await {
        eprintln!("could not create {}: {error}", data_dir.display());
        std::process::exit(1);
    }

    let state = Arc::new(AppState {
        rooms: Mutex::new(HashMap::new()),
        clients: Mutex::new(HashMap::new()),
        data_dir,
        master_token,
    });

    // one ticker for every room, rather than a timer each: the work is trivial
    // and this keeps the shutdown story simple
    tokio::spawn(tick_rooms(state.clone()));

    let app = Router::new()
        .route(
            "/api/health",
            any(|| async { "ontime-server-native online" }),
        )
        .route("/ws", any(websocket_handler))
        .route("/r/{room}/ws", any(websocket_handler_for_room))
        .route("/{*path}", any(rest_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("ontime-server-native listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

/// Advances anything that has to move on its own: a rolling rundown reaching an
/// event's start, an end action, or simply keeping connected clients current.
///
/// Polls finely but only acts when the shared policy says a wake is due, so this
/// host broadcasts at exactly the same cadence as the Worker. Answering that
/// question here instead of in `ontime-core` is how the two would drift apart.
async fn tick_rooms(state: Arc<AppState>) {
    let mut ticker = tokio::time::interval(tokio::time::Duration::from_millis(50));
    let mut next_wake: HashMap<String, i64> = HashMap::new();

    loop {
        ticker.tick().await;
        let now = now_ms();
        let mut rooms = state.rooms.lock().await;
        let mut to_save = Vec::new();

        for (name, room) in rooms.iter_mut() {
            // nobody subscribed means nobody to broadcast to, so a room left
            // running stops costing anything
            let has_clients = room.outbound.receiver_count() > 0;
            let Some(delay) = api::next_wake_in(&room.project, now, has_clients) else {
                next_wake.remove(name);
                continue;
            };

            let due = next_wake.entry(name.clone()).or_insert(now);
            if now < *due {
                continue;
            }
            *due = now + delay;

            let mut changed = false;
            if let Some(rundown) = room.project.current().cloned() {
                changed = room
                    .project
                    .runtime
                    .on_tick(&rundown, now, room.project.timezone_offset);
            }

            room.broadcast_runtime(now);

            if changed {
                // the wake the state change implies may be sooner than the one
                // just scheduled
                if let Some(next) = api::next_wake_in(&room.project, now, has_clients) {
                    *due = (*due).min(now + next);
                }
                to_save.push((name.clone(), room.project.clone()));
            }
        }

        drop(rooms);
        for (name, project) in to_save {
            let _ = store::save(&state.data_dir, &name, &project).await;
        }
    }
}

/// Loads a room from disk on first use, then keeps it in memory.
///
/// Also reports whether the room is new — nothing on disk and nothing in memory —
/// so the creation gate can be applied.
async fn with_room<T>(
    state: &AppState,
    name: &str,
    action: impl FnOnce(&mut Room) -> T,
) -> (T, bool) {
    let mut rooms = state.rooms.lock().await;
    let mut is_new = false;
    if !rooms.contains_key(name) {
        is_new = !store::exists(&state.data_dir, name).await;
        let project = store::load(&state.data_dir, name).await;
        rooms.insert(name.to_string(), Room::new(project));
    }
    let room = rooms.get_mut(name).expect("just inserted");
    let result = action(room);
    (result, is_new)
}

fn room_from_path(path: &str) -> (String, String) {
    if let Some(rest) = path.strip_prefix("/r/") {
        let (room, remainder) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        };
        if is_valid_room(room) {
            let remainder = if remainder.is_empty() { "/" } else { remainder };
            return (room.to_string(), remainder.to_string());
        }
    }
    (DEFAULT_ROOM.to_string(), path.to_string())
}

fn token_from(headers: &HeaderMap, query: &HashMap<String, String>) -> Option<String> {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            return Some(token.trim().to_string());
        }
    }
    if let Some(token) = query.get("token") {
        return Some(token.clone());
    }
    // the same cookie shape the original server sets
    let cookies = headers.get("cookie").and_then(|v| v.to_str().ok())?;
    for cookie in cookies.split(';') {
        // skip past other cookies rather than giving up on the first one
        let Some(value) = cookie.trim().strip_prefix("token=") else {
            continue;
        };
        let decoded = value.trim_matches('"');
        if let Ok(parsed) = serde_json::from_str::<Value>(decoded) {
            if let Some(token) = parsed.get("token").and_then(Value::as_str) {
                return Some(token.to_string());
            }
        }
        return Some(decoded.to_string());
    }
    None
}

async fn rest_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let full_path = format!("/{path}");
    let (room_name, api_path) = room_from_path(&full_path);

    let room_is_new = !store::exists(&state.data_dir, &room_name).await
        && !state.rooms.lock().await.contains_key(&room_name);

    let request = ApiRequest {
        method: HttpMethod::parse(method.as_str()),
        path: api_path,
        body: serde_json::from_slice::<Value>(&body)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({})),
        token: token_from(&headers, &query),
        master_token: state.master_token.clone(),
        room_is_new,
    };

    let (response, _) = with_room(&state, &room_name, |room| {
        let response = api::handle(&mut room.project, &request);

        let mut persist = false;
        let mut refetched = false;
        for effect in &response.effects {
            match effect {
                Effect::Persist => persist = true,
                Effect::Refetch { target, rundown_id } => {
                    persist = true;
                    refetched = true;
                    room.broadcast_refetch(target, rundown_id.as_deref());
                }
                Effect::Reschedule => {}
            }
        }
        if refetched {
            room.broadcast_runtime(now_ms());
        }

        (response, persist, room.project.clone())
    })
    .await;

    let (response, persist, project) = response;
    if persist {
        let _ = store::save(&state.data_dir, &room_name, &project).await;
    }

    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    match (response.body, response.text) {
        (Some(body), _) => (status, axum::Json(body)).into_response(),
        (None, Some(text)) => (status, text).into_response(),
        (None, None) => status.into_response(),
    }
}

async fn websocket_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let room = query
        .get("room")
        .filter(|r| is_valid_room(r))
        .cloned()
        .unwrap_or_else(|| DEFAULT_ROOM.to_string());
    upgrade_socket(state, room, query, headers, upgrade).await
}

async fn websocket_handler_for_room(
    State(state): State<Arc<AppState>>,
    Path(room): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let room = if is_valid_room(&room) {
        room
    } else {
        DEFAULT_ROOM.to_string()
    };
    upgrade_socket(state, room, query, headers, upgrade).await
}

async fn upgrade_socket(
    state: Arc<AppState>,
    room_name: String,
    query: HashMap<String, String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let room_is_new = !store::exists(&state.data_dir, &room_name).await
        && !state.rooms.lock().await.contains_key(&room_name);

    let request = ApiRequest {
        method: HttpMethod::Get,
        path: "/ws".to_string(),
        body: json!({}),
        token: token_from(&headers, &query),
        master_token: state.master_token.clone(),
        room_is_new,
    };

    // an upgrade never reaches the shared routing, so it is gated here by the
    // same rule the routing uses
    let (authorised, _) = with_room(&state, &room_name, |room| {
        api::is_authorised(&room.project, &request)
    })
    .await;
    if !authorised {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    if !api::may_create_room(&request) {
        return (StatusCode::FORBIDDEN, "This room does not exist").into_response();
    }

    upgrade.on_upgrade(move |socket| serve_socket(state, room_name, socket))
}

async fn serve_socket(state: Arc<AppState>, room_name: String, socket: WebSocket) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sink, mut stream) = socket.split();

    let (mut receiver, initial, client_id) = {
        let mut rooms = state.rooms.lock().await;
        if !rooms.contains_key(&room_name) {
            let project = store::load(&state.data_dir, &room_name).await;
            rooms.insert(room_name.clone(), Room::new(project));
        }
        let room = rooms.get_mut(&room_name).expect("just inserted");

        let id = format!("{:x}-{}", now_ms(), room.outbound.receiver_count());
        let init = json!({
            "tag": "client-init",
            "payload": { "clientId": id, "clientName": format!("client-{id}") }
        });

        let mut packets = vec![init];
        if let Some(rundown) = room.project.current() {
            let now = now_ms();
            let mut payload =
                room.project
                    .runtime
                    .snapshot(rundown, now, room.project.timezone_offset);
            if let Some(object) = payload.as_object_mut() {
                if let Some(aux) = room.project.aux_timers.snapshot(now).as_object() {
                    object.extend(aux.clone());
                }
                object.insert("message".into(), json!(room.project.message));
            }
            packets.push(json!({ "tag": "runtime-data", "payload": payload }));
        }

        (room.outbound.subscribe(), packets, id)
    };

    {
        let mut clients = state.clients.lock().await;
        clients.insert(
            client_id.clone(),
            ClientInfo {
                room: room_name.clone(),
                name: format!("client-{client_id}"),
                client_type: "ontime".to_string(),
                identify: false,
                origin: String::new(),
                path: String::new(),
            },
        );
    }
    broadcast_client_list(&state, &room_name, None).await;

    for packet in initial {
        if sink.send(Message::text(packet.to_string())).await.is_err() {
            break;
        }
    }

    let forward_state = state.clone();
    let forward_room = room_name.clone();

    loop {
        tokio::select! {
            outgoing = receiver.recv() => {
                match outgoing {
                    Ok(packet) => {
                        if sink.send(Message::text(packet)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = stream.next() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };

                if let Some(reply) = handle_socket_message(&forward_state, &forward_room, &client_id, &text).await {
                    if sink.send(Message::text(reply)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    state.clients.lock().await.remove(&client_id);
    broadcast_client_list(&state, &room_name, Some(&client_id)).await;
}

/// Announces who is connected to a room.
///
/// `leaving` is excluded, for the moment a client is on its way out.
async fn broadcast_client_list(state: &AppState, room_name: &str, leaving: Option<&str>) {
    let clients = state.clients.lock().await;
    let mut listed = serde_json::Map::new();
    for (id, client) in clients.iter() {
        if client.room != room_name || leaving == Some(id.as_str()) {
            continue;
        }
        listed.insert(
            id.clone(),
            json!({
                "name": client.name,
                "type": client.client_type,
                "identify": client.identify,
                "origin": client.origin,
                "path": client.path,
            }),
        );
    }
    drop(clients);

    let rooms = state.rooms.lock().await;
    if let Some(room) = rooms.get(room_name) {
        room.publish(json!({ "tag": "client-list", "payload": listed }));
    }
}

/// Applies a websocket command. Returns a direct reply, when one is owed.
async fn handle_socket_message(
    state: &AppState,
    room_name: &str,
    client_id: &str,
    text: &str,
) -> Option<String> {
    let packet = serde_json::from_str::<Value>(text).ok()?;
    let tag = packet
        .get("tag")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = packet.get("payload");

    if tag == "ping" {
        let echo = payload.cloned().unwrap_or(Value::Null);
        return Some(json!({ "tag": "pong", "payload": echo }).to_string());
    }

    let now = now_ms();

    // client bookkeeping is the one thing a host has to own itself, because the
    // connections are its own
    if tag == "client-set" || tag == "client-set-path" {
        let mut clients = state.clients.lock().await;
        if let Some(client) = clients.get_mut(client_id) {
            match (tag, payload) {
                ("client-set-path", Some(path)) => {
                    if let Some(path) = path.as_str() {
                        client.path = path.to_string();
                    }
                }
                ("client-set", Some(patch)) => {
                    if let Some(name) = patch.get("name").and_then(Value::as_str) {
                        client.name = name.to_string();
                    }
                    if let Some(identify) = patch.get("identify").and_then(Value::as_bool) {
                        client.identify = identify;
                    }
                    if let Some(origin) = patch.get("origin").and_then(Value::as_str) {
                        client.origin = origin.to_string();
                    }
                    if let Some(kind) = patch.get("type").and_then(Value::as_str) {
                        client.client_type = kind.to_string();
                    }
                }
                _ => {}
            }
        }
        drop(clients);
        broadcast_client_list(state, room_name, None).await;
        return None;
    }

    // everything else — playback, the overlay, the aux timers and the report —
    // goes through the shared path, so this host cannot implement three of them
    // and quietly lose the fourth
    let ((changed, project), _) = with_room(state, room_name, |room| {
        let changed = api::apply_socket_command(&mut room.project, tag, payload, now);
        if changed {
            room.broadcast_runtime(now);
        }
        (changed, room.project.clone())
    })
    .await;

    if changed {
        let _ = store::save(&state.data_dir, room_name, &project).await;
    }

    None
}
