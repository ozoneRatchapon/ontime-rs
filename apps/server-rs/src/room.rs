use serde_json::{json, Value};
use worker::*;

use std::collections::HashMap;
use std::time::Duration;

use ontime_core::api::{self, ApiRequest, Effect, HttpMethod};
use ontime_core::audit::{Actor, Chunk, Note, Retention, Writes};
use ontime_core::audit_describe;
use ontime_core::guard::{fingerprint, Guard};
use ontime_core::project::Project;
use ontime_core::registry::{should_report, Member, RoomReport};
use ontime_core::socket;

use serde::{Deserialize, Serialize};

use crate::audit_store::{self, Batch};
use crate::guard_host::{self, master_token};
use crate::{presence, registry_host};

#[path = "room_integration.rs"]
mod integration;
#[path = "room_snapshots.rs"]
mod snapshots;
#[path = "room_views.rs"]
mod views;

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
    /// Connected with a membership, the room password or the master password,
    /// so it may drive a members-only room. Decided once, at connect.
    #[serde(default)]
    privileged: bool,
    /// Who drives the room from this socket, for the audit log. Decided once,
    /// at connect, like `privileged`.
    #[serde(default = "anyone")]
    actor: Actor,
}

fn anyone() -> Actor {
    Actor::Anyone
}

/// How long a room trusts what the registry said about a session. Removing
/// someone takes effect within this long, without asking on every request.
const MEMBER_CACHE_MS: i64 = 60_000;

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
    /// When this instance last reported to the registry, and what it said. Lost
    /// on eviction, which only means the next use reports again.
    last_report: Option<(i64, RoomReport)>,
    /// Session token -> (when asked, who they are here). Refusals are kept too,
    /// so a bad token cannot make the room ask the registry on every request.
    members: HashMap<String, (i64, Option<Member>)>,
    /// The newest audit chunk, kept once loaded so each change does not read
    /// it again. Lost on eviction, which only costs one read.
    audit_head: Option<Chunk>,
    /// How often this room takes integration calls. In memory: an eviction
    /// only hands out a fresh burst.
    rate_limit: ontime_core::integration::RateLimit,
    /// Wrong guesses at this room's tokens, per client; in the registry
    /// instance, at the admin routes'. In memory: see `ontime_core::guard`.
    guard: Guard,
    /// The undo history's list of copies, once read. Lost on eviction, which
    /// only costs one read.
    snapshot_index: Option<Vec<i64>>,
    /// Whether anything was written since the newest copy. Assumed so after an
    /// eviction, which at worst costs one copy too many.
    changed_since_copy: bool,
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
            last_report: None,
            members: HashMap::new(),
            audit_head: None,
            rate_limit: Default::default(),
            guard: Guard::default(),
            snapshot_index: None,
            changed_since_copy: true,
        }
    }

    async fn fetch(&mut self, mut req: Request) -> Result<Response> {
        // the registry instance is the same class but holds no project; the
        // Worker never routes these paths to a room, only to `@registry`
        if req.path() == guard_host::GUARD_PATH {
            return guard_host::serve(&mut self.guard, req, now_ms()).await;
        }
        if req.path().starts_with(registry_host::REGISTRY_PATH) {
            return registry_host::serve(&self.state, req).await;
        }
        if let Err(e) = self.hydrate().await {
            return Response::error(format!("ROOM_LOAD_ERR: {e}"), 500);
        }

        // a client locked out for guessing is refused whatever it presents, so
        // a right guess cannot be told from a wrong one; one presenting nothing
        // guesses nothing, and an open room still lets it in
        let now = now_ms();
        let client = guard_host::client(&req);
        let token = Self::presented_token(&req);
        if token.is_some() {
            if let Err(seconds) = self.guard.check(&client, now) {
                return guard_host::too_many(seconds);
            }
        }
        let response = match self.handle(&mut req).await {
            Ok(res) => res,
            Err(e) => return Response::error(format!("ROOM_FETCH_ERR: {e}"), 500),
        };
        // a refusal is a wrong guess only when the token was not one this room
        // knows: a stale token in an open room is not refused, and is not counted
        if let Some(token) = token {
            let refused = matches!(response.status_code(), 401 | 403);
            let master = master_token(&self._env);
            if refused && !api::is_known_token(&self.project, master.as_deref(), &token) {
                self.guard.fail(&client, fingerprint(&token), now);
            }
        }
        Ok(response)
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
            let reply = socket::pong(echo, now_ms(), self.project.timezone_offset);
            let _ = ws.send_with_str(reply.to_string());
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

        let info = ws.deserialize_attachment::<ClientInfo>().ok().flatten();

        // a viewer's screen in a members-only room may watch, never drive
        if self.project.members_only {
            let privileged = info.as_ref().is_some_and(|info| info.privileged);
            if !privileged {
                // sent as a log line, which the editor already shows
                let refusal = json!({
                    "tag": "log",
                    "payload": {
                        "id": uuid::Uuid::now_v7().to_string(),
                        "origin": "SERVER",
                        "time": "",
                        "level": "ERROR",
                        "text": api::NEEDS_SIGN_IN,
                    }
                });
                let _ = ws.send_with_str(refusal.to_string());
                return Ok(());
            }
        }

        // every other tag is an action, shared with the HTTP integration API
        self.integration_socket(&ws, info, tag, payload).await
    }

    /// Fires at the end of the loaded event, or on the heartbeat — see `sync_alarm`.
    async fn alarm(&mut self) -> Result<Response> {
        self.hydrate().await?;

        let now = now_ms();
        // an end action may advance the rundown, and the teleprompter may reach
        // its end; either change has to be durable
        if api::on_alarm(&mut self.project, now) {
            self.persist(None).await?;
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

    /// Writes the project, and with it the audit chunk a change added, in one
    /// call, so a change and its log line cannot disagree.
    async fn persist(&mut self, audit_writes: Option<Writes>) -> Result<()> {
        // once something is stored the room exists, and the creation gate no
        // longer applies to it
        self.is_new = false;

        // the room's undo history: what is stored now, before this write
        self.snapshot_before_write(ontime_core::snapshots::Reason::Edit)
            .await;

        let raw = serde_json::to_string(&self.project)
            .map_err(|e| Error::RustError(format!("Could not serialise project: {e}")))?;
        let mut storage = self.state.storage();
        match audit_writes {
            None => storage.put(STORAGE_KEY, raw).await?,
            Some(writes) => {
                let mut batch = Batch::new();
                batch.put_raw(STORAGE_KEY, raw)?;
                batch.audit(&writes)?;
                batch.commit(&mut storage).await?;
            }
        }
        self.changed_since_copy = true;
        Ok(())
    }

    /// Adds a line to the log in memory. The returned writes go to [`persist`].
    async fn audit(&mut self, actor: Actor, note: Note, now: i64) -> Writes {
        let head = audit_store::head(
            &self.state.storage(),
            &self.project.audit,
            self.audit_head.take(),
        )
        .await;
        let writes = self
            .project
            .audit
            .record(head, actor, note, now, Retention::ROOM);
        self.audit_head = Some(writes.put.clone());
        writes
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
        let clients = self.client_map(leaving);
        let packet = json!({ "tag": "client-list", "payload": clients }).to_string();
        for socket in self.state.get_websockets() {
            let _ = socket.send_with_str(&packet);
        }
    }

    /// Every connected client keyed by id, without the one that is `leaving`.
    fn client_map(&self, leaving: Option<&str>) -> serde_json::Map<String, Value> {
        let mut clients = serde_json::Map::new();
        for socket in self.state.get_websockets() {
            if let Ok(Some(info)) = socket.deserialize_attachment::<ClientInfo>() {
                if leaving == Some(info.id.as_str()) {
                    continue;
                }
                clients.insert(info.id.clone(), info.to_client());
            }
        }
        clients
    }

    fn client_list(&self, leaving: Option<&str>) -> Vec<Value> {
        self.client_map(leaving).into_values().collect()
    }

    /// Tells the registry this room exists and is in use — when something it
    /// lists has changed, otherwise at most once per `REPORT_INTERVAL_MS`. A room
    /// that was never created does not report, so a refused request for a
    /// made-up name never lands in the list.
    ///
    /// Best effort: the registry being unreachable must never fail the room's
    /// own request.
    async fn report_if_due(&mut self, room_name: &str) {
        if self.is_new {
            return;
        }
        let now = now_ms();
        let report = RoomReport {
            room: room_name.to_string(),
            title: self.project.project.title.clone(),
            protected: self.project.password_hash.is_some(),
            rundowns: self.project.rundowns.len(),
            members_only: self.project.members_only,
        };
        if !should_report(self.last_report.as_ref(), &report, now) {
            return;
        }
        if let Err(e) = registry_host::report(&self._env, &report).await {
            console_warn!("could not report {room_name} to the registry: {e}");
            return;
        }
        self.last_report = Some((now, report));
    }

    /// The whole `runtime-data` payload: playback, aux timers and the overlay all
    /// live in the same client store.
    ///
    /// Built in one place because it is sent from two — on connect and on change —
    /// and the two drifting apart is how the aux timers first went missing.
    fn runtime_payload(&self, now: i64) -> Option<Value> {
        api::runtime_payload(&self.project, now)
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

    /// A person's session, if the request carries one: a header for REST, the
    /// query string for a websocket, which cannot set headers.
    fn presented_session(req: &Request) -> Option<String> {
        if let Ok(Some(session)) = req.headers().get(registry_host::SESSION_HEADER) {
            return Some(session.trim().to_string()).filter(|s| !s.is_empty());
        }
        req.url()
            .ok()?
            .query_pairs()
            .find(|(key, _)| key == "session")
            .map(|(_, session)| session.to_string())
            .filter(|s| !s.is_empty())
    }

    /// What the person behind a session is to this room, asking the registry at
    /// most once a minute per session.
    async fn member(&mut self, session: Option<String>, room_name: &str) -> Option<Member> {
        let session = session?;
        let now = now_ms();
        self.members
            .retain(|_, (asked, _)| now - *asked < MEMBER_CACHE_MS);
        if let Some((_, member)) = self.members.get(&session) {
            return member.clone();
        }
        match registry_host::check(&self._env, &session, room_name).await {
            Ok(member) => {
                self.members.insert(session, (now, member.clone()));
                member
            }
            // the registry being unreachable is not an answer worth keeping
            Err(e) => {
                console_warn!("could not check a session for {room_name}: {e}");
                None
            }
        }
    }

    /// Translates the Worker's request into the transport-agnostic one, hands it
    /// to the shared routing in `ontime-core`, and carries out what comes back.
    ///
    /// All the routing lives in the core so that another host cannot drift into
    /// being a second implementation.
    async fn handle(&mut self, req: &mut Request) -> Result<Response> {
        let path = req.path();
        // the core reads query parameters off the path, as `?source=` for a script
        let path_and_query = match req
            .url()
            .ok()
            .and_then(|url| url.query().map(str::to_string))
        {
            Some(query) => format!("{path}?{query}"),
            None => path.clone(),
        };
        let is_upgrade = path.contains("/ws") && !path.contains("/data/");
        let room_name = presence::room_name(req);
        // the integration API takes a token only, so it is decided before any
        // session is looked up
        if integration::is_integration_path(&path) {
            return self.integration_http(req, &room_name).await;
        }
        let member = self.member(Self::presented_session(req), &room_name).await;

        let api_request = ApiRequest {
            method: HttpMethod::parse(req.method().as_ref()),
            path: path_and_query,
            body: body(req).await,
            token: Self::presented_token(req),
            master_token: master_token(&self._env),
            room_is_new: self.is_new,
            member: member.as_ref().map(|member| member.role),
        };
        // decided before the change: setting a password changes what the
        // presented token matches
        let actor = audit_describe::actor(&self.project, &api_request, member.as_ref());

        // the room's custom views (plan 021), stored apart from the project.
        // Before the upgrade test, which a page named `ws-…` would also match.
        if views::is_page_path(&path) {
            return self.page_http(req, &api_request).await;
        }
        if views::is_views_path(&path) {
            return self.views_http(req, &api_request, actor).await;
        }

        // an upgrade never reaches the shared routing, because the socket belongs
        // to the host — but it is gated by the same rule
        if is_upgrade {
            if !api::is_authorised(&self.project, &api_request) {
                return Response::error("Unauthorized", 401);
            }
            if !api::may_create_room(&api_request) {
                return Response::error("This room does not exist", 403);
            }
            let upgraded =
                self.open_websocket(api::is_privileged(&self.project, &api_request), actor)?;
            // there is somebody to broadcast to again, so the heartbeat that was
            // stood down when the last client left has to come back
            self.sync_alarm().await?;
            self.report_if_due(&room_name).await;
            return Ok(upgraded);
        }

        // who is looking at this room right now, for the admin page. Host-side
        // because the sockets belong to the host, not to the shared routing.
        if req.method() == Method::Get && path.ends_with("/data/clients") {
            if !api::is_authorised(&self.project, &api_request) {
                return Response::error("Unauthorized", 401);
            }
            if self.is_new {
                return Response::error("This room does not exist", 404);
            }
            return Response::from_json(&json!({ "clients": self.client_list(None) }));
        }

        // the undo history, for an owner or the admin. Host-side because the
        // copies are stored apart from the project.
        if snapshots::is_snapshot_path(&path) {
            return self.snapshots_http(req, &api_request, actor).await;
        }
        // a change that removes things is copied first, so it can be undone
        if snapshots::is_destructive(&req.method(), &path)
            && api::is_authorised(&self.project, &api_request)
        {
            self.snapshot_before_write(ontime_core::snapshots::Reason::Destructive)
                .await;
        }

        // who changed what, for an owner or the admin. Host-side because the
        // lines are stored apart from the project.
        if req.method() == Method::Get && path.ends_with("/data/activity") {
            if !api::is_authorised(&self.project, &api_request) {
                return Response::error("Unauthorized", 401);
            }
            if !api::is_owner_or_admin(&api_request) {
                return Response::error("Only an owner of this room can read its activity", 403);
            }
            if self.is_new {
                return Response::error("This room does not exist", 404);
            }
            let storage = self.state.storage();
            return audit_store::page(
                &storage,
                &self.project.audit,
                &req.url()?,
                Retention::ROOM,
                now_ms(),
                self.audit_head.as_ref(),
            )
            .await;
        }

        let note = audit_describe::http_note(&self.project, &api_request);
        let now = now_ms();
        let response = api::handle(&mut self.project, &api_request, now);

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
            let writes = match note {
                Some(note) if response.status < 400 => Some(self.audit(actor, note, now).await),
                _ => None,
            };
            self.persist(writes).await?;
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
        if response.status < 400 {
            self.report_if_due(&room_name).await;
        }

        let mut worker_response = match (&response.body, &response.text) {
            (Some(body), _) => Response::from_json(body)?,
            (None, Some(text)) => Response::ok(text.clone())?,
            (None, None) => Response::empty()?,
        };
        worker_response = worker_response.with_status(response.status);
        Ok(worker_response)
    }

    fn open_websocket(&mut self, privileged: bool, actor: Actor) -> Result<Response> {
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
            privileged,
            actor,
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
