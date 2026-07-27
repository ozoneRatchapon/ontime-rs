use serde_json::{json, Value};
use worker::*;

use std::time::Duration;

use ontime_core::api::{self, ApiRequest, Effect, HttpMethod};
use ontime_core::project::{hash_password, Project};

use serde::{Deserialize, Serialize};

/// Per-connection metadata, stored on the socket itself.
///
/// Hibernation evicts the Durable Object between messages, so this cannot live in
/// a field — the attachment survives with the socket, which is exactly what a
/// client list needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientInfo {
    id: String,
    name: String,
    #[serde(default = "unknown_type")]
    client_type: String,
    #[serde(default)]
    identify: bool,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    path: String,
}

fn unknown_type() -> String {
    "ontime".to_string()
}

impl ClientInfo {
    fn to_client(&self) -> Value {
        json!({
            "name": self.name,
            "type": self.client_type,
            "identify": self.identify,
            "origin": self.origin,
            "path": self.path,
        })
    }
}

/// Single storage key holding the whole project document.
/// On the SQLite backend a key and its value together cannot exceed 2 MB, which
/// bounds how large a project can grow; splitting per rundown is the next step
/// if one ever outgrows it.
const STORAGE_KEY: &str = "ontime_project_v2";

/// Epoch milliseconds. `Date::now()` is the Workers clock; `std::time` is unavailable.
fn now_ms() -> i64 {
    Date::now().as_millis() as i64
}

#[durable_object]
pub struct OntimeProject {
    state: State,
    _env: Env,
    project: Project,
    /// Whether `project` has been hydrated from storage in this instance.
    loaded: bool,
    /// True when storage held nothing, meaning this room is being used for the
    /// first time.
    is_new: bool,
}

#[durable_object]
#[async_trait::async_trait(?Send)]
impl DurableObject for OntimeProject {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            _env: env,
            project: Project::default(),
            loaded: false,
            is_new: false,
        }
    }

    async fn fetch(&mut self, mut req: Request) -> Result<Response> {
        if let Err(e) = self.hydrate().await {
            return Response::error(format!("ROOM_LOAD_ERR: {e}"), 500);
        }
        match self.handle(&mut req).await {
            Ok(res) => Ok(res),
            Err(e) => Response::error(format!("ROOM_FETCH_ERR: {e}"), 500),
        }
    }

    async fn websocket_message(
        &mut self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        self.hydrate().await?;

        let WebSocketIncomingMessage::String(text) = message else {
            return Ok(());
        };
        let Ok(packet) = serde_json::from_str::<Value>(&text) else {
            return Ok(());
        };
        let tag = packet
            .get("tag")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let payload = packet.get("payload");

        if tag == "ping" {
            let echo = payload.cloned().unwrap_or(Value::Null);
            let _ = ws.send_with_str(json!({ "tag": "pong", "payload": echo }).to_string());
            return Ok(());
        }

        // client bookkeeping: the socket carries its own record
        if tag == "client-set" || tag == "client-set-path" {
            if let Ok(Some(mut info)) = ws.deserialize_attachment::<ClientInfo>() {
                match (tag, payload) {
                    ("client-set-path", Some(path)) => {
                        if let Some(path) = path.as_str() {
                            info.path = path.to_string();
                        }
                    }
                    ("client-set", Some(patch)) => {
                        if let Some(name) = patch.get("name").and_then(Value::as_str) {
                            info.name = name.to_string();
                        }
                        if let Some(identify) = patch.get("identify").and_then(Value::as_bool) {
                            info.identify = identify;
                        }
                        if let Some(origin) = patch.get("origin").and_then(Value::as_str) {
                            info.origin = origin.to_string();
                        }
                        if let Some(client_type) = patch.get("type").and_then(Value::as_str) {
                            info.client_type = client_type.to_string();
                        }
                    }
                    _ => {}
                }
                let _ = ws.serialize_attachment(&info);
                self.broadcast_client_list(None);
            }
            return Ok(());
        }

        let now = now_ms();
        if api::apply_socket_command(&mut self.project, tag, payload, now) {
            self.persist().await?;
            self.broadcast_runtime(now);
            self.sync_alarm().await?;
        }

        Ok(())
    }

    /// Fires at the end of the loaded event, or on the heartbeat — see `sync_alarm`.
    async fn alarm(&mut self) -> Result<Response> {
        self.hydrate().await?;

        let now = now_ms();
        let Some(rundown) = self.project.current().cloned() else {
            return Response::ok("");
        };

        // an end action may advance the rundown; that change has to be durable
        if self
            .project
            .runtime
            .on_tick(&rundown, now, self.project.timezone_offset)
        {
            self.persist().await?;
        }

        self.broadcast_runtime(now);
        self.sync_alarm().await?;

        Response::ok("")
    }

    async fn websocket_close(
        &mut self,
        ws: WebSocket,
        _code: usize,
        _reason: String,
        _clean: bool,
    ) -> Result<()> {
        let leaving = ws
            .deserialize_attachment::<ClientInfo>()
            .ok()
            .flatten()
            .map(|info| info.id);
        self.broadcast_client_list(leaving.as_deref());
        // with the last client gone there is nobody to broadcast to, so a room
        // left running stops waking
        self.sync_alarm().await?;
        Ok(())
    }
}

impl OntimeProject {
    /// Loads the project from storage once per instance.
    /// `DurableObject::new` is synchronous, so this cannot happen in the constructor —
    /// without it every eviction silently reset the project to its defaults.
    async fn hydrate(&mut self) -> Result<()> {
        if self.loaded {
            return Ok(());
        }
        self.loaded = true;

        // stored as a JSON string rather than a struct: the document nests maps and
        // free-form `serde_json::Value`, which do not round trip cleanly through
        // serde_wasm_bindgen's JS representation
        let stored = self
            .state
            .storage()
            .get::<String>(STORAGE_KEY)
            .await
            .ok()
            .and_then(|raw| serde_json::from_str::<Project>(&raw).ok());

        match stored {
            Some(mut project) => {
                project.ensure_consistent();
                self.project = project;
            }
            // no value yet (first run) or a document written by an older shape
            None => {
                self.project = Project::default();
                self.is_new = true;
            }
        }
        Ok(())
    }

    async fn persist(&mut self) -> Result<()> {
        // once something is stored the room exists, and the creation gate no
        // longer applies to it
        self.is_new = false;

        let raw = serde_json::to_string(&self.project)
            .map_err(|e| Error::RustError(format!("Could not serialise project: {e}")))?;
        self.state.storage().put(STORAGE_KEY, raw).await
    }

    /// Tells every connected client to refetch a slice of server state.
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
        let packet = json!({ "tag": "refetch", "payload": payload }).to_string();

        for socket in self.state.get_websockets() {
            let _ = socket.send_with_str(&packet);
        }
    }

    /// Announces who is connected. Built from the sockets rather than a field,
    /// because hibernation evicts the object between messages.
    ///
    /// `leaving` is the id of a socket that is closing: it is still present in
    /// `get_websockets()` while `websocket_close` runs, so it has to be excluded
    /// explicitly or a disconnect never shows up.
    fn broadcast_client_list(&self, leaving: Option<&str>) {
        let mut clients = serde_json::Map::new();
        for socket in self.state.get_websockets() {
            if let Ok(Some(info)) = socket.deserialize_attachment::<ClientInfo>() {
                if leaving == Some(info.id.as_str()) {
                    continue;
                }
                clients.insert(info.id.clone(), info.to_client());
            }
        }

        let packet = json!({ "tag": "client-list", "payload": clients }).to_string();
        for socket in self.state.get_websockets() {
            let _ = socket.send_with_str(&packet);
        }
    }

    /// The whole `runtime-data` payload: playback, aux timers and the overlay all
    /// live in the same client store.
    ///
    /// Built in one place because it is sent from two — on connect and on change —
    /// and the two drifting apart is how the aux timers first went missing.
    fn runtime_payload(&self, now: i64) -> Option<Value> {
        let rundown = self.project.current()?;
        let mut payload = self
            .project
            .runtime
            .snapshot(rundown, now, self.project.timezone_offset);

        if let Some(obj) = payload.as_object_mut() {
            if let Some(aux) = self.project.aux_timers.snapshot(now).as_object() {
                obj.extend(aux.clone());
            }
            obj.insert("message".into(), json!(self.project.message));
        }

        Some(payload)
    }

    /// Sends the current playback state to every connected client.
    fn broadcast_runtime(&self, now: i64) {
        let Some(payload) = self.runtime_payload(now) else {
            return;
        };
        let packet = json!({ "tag": "runtime-data", "payload": payload }).to_string();

        for socket in self.state.get_websockets() {
            let _ = socket.send_with_str(&packet);
        }
    }

    /// Schedules the next wake-up, and only while something is actually running.
    ///
    /// The browser advances the timer itself from the `_engine` block, so the server
    /// no longer has to tick once a second. It wakes for two reasons:
    ///
    /// - at the exact moment the loaded event runs out, to apply its `endAction`
    /// - every `heartbeat_ms` as a safety net for any client that is not running
    ///   the WASM engine (an old cached bundle, or something talking to the socket
    ///   directly) and would otherwise show a frozen timer
    ///
    /// Idle costs nothing at all, which is what keeps the free tier safe.
    async fn sync_alarm(&mut self) -> Result<()> {
        let Some(delay) = self.next_wake() else {
            // delete_alarm errors when none is set, which is not a failure here
            let _ = self.state.storage().delete_alarm().await;
            return Ok(());
        };
        self.state.storage().set_alarm(delay).await
    }

    fn next_wake(&self) -> Option<Duration> {
        // with no sockets attached there is nobody to broadcast to, so a room
        // whose timer was left running stops costing anything
        let has_clients = !self.state.get_websockets().is_empty();
        api::next_wake_in(&self.project, now_ms(), has_clients)
            .map(|ms| Duration::from_millis(ms as u64))
    }

    /// The token a request is presenting, from any of the places one can travel.
    ///
    /// A websocket cannot set headers, so it uses the query string; REST uses a
    /// bearer header. The cookie form matches the original server, so a browser
    /// already logged in against `apps/server` keeps working.
    fn presented_token(req: &Request) -> Option<String> {
        if let Ok(Some(auth)) = req.headers().get("Authorization") {
            if let Some(token) = auth.strip_prefix("Bearer ") {
                return Some(token.trim().to_string());
            }
        }

        if let Ok(url) = req.url() {
            if let Some((_, token)) = url.query_pairs().find(|(key, _)| key == "token") {
                return Some(token.to_string());
            }
        }

        // cookie value is `{"token":"…","scope":"rw"}`
        if let Ok(Some(cookies)) = req.headers().get("Cookie") {
            for cookie in cookies.split(';') {
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
        }

        None
    }

    /// Translates the Worker's request into the transport-agnostic one, hands it
    /// to the shared routing in `ontime-core`, and carries out what comes back.
    ///
    /// All the routing lives in the core so that another host cannot drift into
    /// being a second implementation.
    async fn handle(&mut self, req: &mut Request) -> Result<Response> {
        let path = req.path();
        let is_upgrade = path.contains("/ws") && !path.contains("/data/");

        let api_request = ApiRequest {
            method: HttpMethod::parse(req.method().as_ref()),
            path,
            body: body(req).await,
            token: Self::presented_token(req),
            master_token: self
                ._env
                .secret("SESSION_PASSWORD")
                .ok()
                .map(|secret| hash_password(&secret.to_string())),
            room_is_new: self.is_new,
        };

        // an upgrade never reaches the shared routing, because the socket belongs
        // to the host — but it is gated by the same rule
        if is_upgrade {
            if !api::is_authorised(&self.project, &api_request) {
                return Response::error("Unauthorized", 401);
            }
            if !api::may_create_room(&api_request) {
                return Response::error("This room does not exist", 403);
            }
            let upgraded = self.open_websocket()?;
            // there is somebody to broadcast to again, so the heartbeat that was
            // stood down when the last client left has to come back
            self.sync_alarm().await?;
            return Ok(upgraded);
        }

        let response = api::handle(&mut self.project, &api_request, now_ms());

        let mut should_persist = false;
        let mut refetches = Vec::new();
        let mut reschedule = false;
        for effect in &response.effects {
            match effect {
                Effect::Persist => should_persist = true,
                Effect::Refetch { target, rundown_id } => {
                    should_persist = true;
                    refetches.push((target.clone(), rundown_id.clone()));
                }
                Effect::Reschedule => reschedule = true,
            }
        }

        if should_persist {
            self.persist().await?;
        }
        for (target, rundown_id) in &refetches {
            self.broadcast_refetch(target, rundown_id.as_deref());
        }
        if !refetches.is_empty() {
            // an edit can change the loaded event's duration or title
            self.broadcast_runtime(now_ms());
        }
        if reschedule || !refetches.is_empty() {
            self.sync_alarm().await?;
        }

        let mut worker_response = match (&response.body, &response.text) {
            (Some(body), _) => Response::from_json(body)?,
            (None, Some(text)) => Response::ok(text.clone())?,
            (None, None) => Response::empty()?,
        };
        worker_response = worker_response.with_status(response.status);
        Ok(worker_response)
    }

    fn open_websocket(&mut self) -> Result<Response> {
        let pair = WebSocketPair::new()?;
        self.state.accept_web_socket(&pair.server);

        let id = uuid::Uuid::now_v7().to_string();
        let name = format!("client-{}", &id[..8]);
        let info = ClientInfo {
            id: id.clone(),
            name: name.clone(),
            client_type: unknown_type(),
            identify: false,
            origin: String::new(),
            path: String::new(),
        };
        let _ = pair.server.serialize_attachment(&info);

        let init = json!({
            "tag": "client-init",
            "payload": { "clientId": id, "clientName": name }
        });
        let _ = pair.server.send_with_str(init.to_string());

        // a client that connects mid-show needs the playback state straight away,
        // not on the next tick (and there is no tick at all when nothing is running)
        if let Some(payload) = self.runtime_payload(now_ms()) {
            let packet = json!({ "tag": "runtime-data", "payload": payload });
            let _ = pair.server.send_with_str(packet.to_string());
        }

        Response::from_websocket(pair.client)
    }
}

// #region helpers ================================================

/// Reads a JSON body, treating an empty or malformed one as `{}` —
/// several endpoints are called with no body at all.
async fn body(req: &mut Request) -> Value {
    match req.json::<Value>().await {
        Ok(value) if value.is_object() => value,
        _ => json!({}),
    }
}

// #endregion helpers =============================================
