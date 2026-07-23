use serde_json::{json, Value};
use worker::*;

use std::time::Duration;

use ontime_core::project::{hash_password, token_matches, Project};
use ontime_core::rundown::Insert;
use ontime_core::runtime::Command;

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

        // playback commands; tags this engine does not implement are ignored
        let Some(command) = Command::parse(tag, payload) else {
            return Ok(());
        };

        let now = now_ms();
        let Some(rundown) = self.project.current().cloned() else {
            return Ok(());
        };

        if self.project.runtime.apply(command, &rundown, now) {
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
        if self.project.runtime.on_tick(&rundown, now) {
            self.persist().await?;
        }

        self.broadcast_runtime(now);
        self.sync_alarm().await?;

        Response::ok("")
    }

    async fn websocket_close(
        &mut self,
        _ws: WebSocket,
        _code: usize,
        _reason: String,
        _clean: bool,
    ) -> Result<()> {
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
                self.persist().await?;
            }
        }
        Ok(())
    }

    async fn persist(&mut self) -> Result<()> {
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

    /// Sends the current playback state to every connected client.
    fn broadcast_runtime(&self, now: i64) {
        let Some(rundown) = self.project.current() else {
            return;
        };
        let payload = self
            .project
            .runtime
            .snapshot(rundown, now, self.project.timezone_offset);
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
        let runtime = &self.project.runtime;
        if !runtime.is_running() {
            return None;
        }

        let rundown = self.project.current()?;
        let event = runtime.selected_id.as_ref().and_then(|id| rundown.get(id));
        let inputs = runtime.inputs_for(event);

        let has_end_action = event
            .and_then(|e| e.get("endAction"))
            .and_then(Value::as_str)
            .is_some_and(|action| action != "none");

        let until_end = match (has_end_action, inputs.ends_at()) {
            (true, Some(ends_at)) => Some((ends_at - now_ms()).max(0) as u64),
            _ => None,
        };

        // 0 or negative disables the safety net entirely: only the end-of-event
        // wake remains, which is the cheapest the server can run
        let heartbeat = self.project.heartbeat_ms;
        let delay_ms = match (until_end, heartbeat > 0) {
            (Some(until_end), true) => until_end.min(heartbeat as u64),
            (Some(until_end), false) => until_end,
            (None, true) => heartbeat as u64,
            (None, false) => return None,
        };

        // a floor stops a rundown of already-expired events spinning the alarm
        Some(Duration::from_millis(delay_ms.max(50)))
    }

    /// Persists and notifies clients. Every mutating handler ends with this.
    async fn commit(&mut self, target: &str, rundown_id: Option<String>) -> Result<()> {
        self.persist().await?;
        self.broadcast_refetch(target, rundown_id.as_deref());
        // an edit can change the loaded event's duration or title
        self.broadcast_runtime(now_ms());
        Ok(())
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

    /// Whether the request may act on this room.
    ///
    /// A room with no password is open, which is how the original behaves when
    /// `SESSION_PASSWORD` is unset — so adding this cannot lock anyone out of an
    /// existing deployment. The `SESSION_PASSWORD` secret, when configured, is a
    /// master key: without it a forgotten room password would be unrecoverable.
    fn is_authorised(&self, req: &Request) -> bool {
        let Some(room_hash) = self.project.password_hash.as_deref() else {
            return true;
        };

        let Some(presented) = Self::presented_token(req) else {
            return false;
        };

        if token_matches(room_hash, &presented) {
            return true;
        }

        match self._env.secret("SESSION_PASSWORD") {
            Ok(master) => token_matches(&hash_password(&master.to_string()), &presented),
            Err(_) => false,
        }
    }

    async fn handle(&mut self, req: &mut Request) -> Result<Response> {
        let raw = req.path();
        let path = raw.split('?').next().unwrap_or(&raw).trim_end_matches('/');
        let method = req.method();

        // the worker forwards the original URL, so trim anything before the API root
        let path = ["/data/", "/api/", "/user/", "/ws"]
            .iter()
            .find_map(|prefix| path.find(prefix).map(|i| &path[i..]))
            .unwrap_or(path);

        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        // Setting the password is allowed while the room is still open, so a new
        // room can be claimed; afterwards it needs the current token like anything else.
        let is_claim = matches!(segments.as_slice(), ["data", "session", "password"])
            && self.project.password_hash.is_none();

        if !is_claim && !self.is_authorised(req) {
            return Response::error("Unauthorized", 401);
        }

        match segments.as_slice() {
            ["ws"] => self.open_websocket(),

            ["data", "rundowns", rest @ ..] => self.rundowns(req, &method, rest).await,
            ["data", "project", rest @ ..] => self.project_data(req, &method, rest).await,
            ["data", "settings", rest @ ..] => self.settings(req, &method, rest).await,
            ["data", "view-settings"] => self.view_settings(req, &method).await,
            ["data", "custom-fields", rest @ ..] => self.custom_fields(req, &method, rest).await,
            ["data", "url-presets", rest @ ..] => self.url_presets(req, &method, rest).await,
            // automations drive OSC/HTTP integrations, which a Worker cannot host;
            // the settings are stored and served, but never acted on
            ["data", "automations", ..] if method == Method::Get => {
                Response::from_json(&self.project.automation)
            }
            ["data", "automations", ..] => {
                Response::error("Automations are not supported on the cloud backend", 501)
            }
            ["data", "custom-views", ..] => Response::from_json(&json!({ "views": [] })),
            ["data", "report", ..] => Response::from_json(&json!({})),
            ["data", "session", "password"] => {
                let body = body(req).await;
                match body.get("password").and_then(Value::as_str) {
                    Some(password) if !password.is_empty() => {
                        let hash = hash_password(password);
                        self.project.password_hash = Some(hash.clone());
                        self.persist().await?;
                        // the hash doubles as the bearer token; it is returned once,
                        // here, because there is no way to recover it from the hash
                        Response::from_json(&json!({ "protected": true, "token": hash }))
                    }
                    // an explicit null clears it, reopening the room
                    _ if body.get("password").map(Value::is_null).unwrap_or(false) => {
                        self.project.password_hash = None;
                        self.persist().await?;
                        Response::from_json(&json!({ "protected": false }))
                    }
                    _ => Response::error("Missing password", 400),
                }
            }
            // GetInfo: the client reads every field, so none may be omitted
            ["data", "session", "info"] => Response::from_json(&json!({
                "networkInterfaces": [],
                "version": self.project.settings.version,
                "serverPort": 443,
                "publicDir": "",
            })),
            // share links are built from the client's own origin, which a Worker
            // serving no HTML cannot know
            ["data", "session", "url"] => {
                Response::error("Share links are not supported on the cloud backend", 501)
            }
            ["data", "session", ..] => Response::from_json(&json!({ "clients": {} })),
            ["data", "assets", "css"] if method == Method::Get => {
                Response::ok(self.project.css_override.clone())
            }
            ["data", "assets", "css"] => {
                let css = body(req)
                    .await
                    .get("css")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.project.css_override = css;
                self.commit("css-override", None).await?;
                Ok(Response::empty()?.with_status(204))
            }
            ["data", "assets", "css", "restore"] => {
                self.project.css_override.clear();
                self.commit("css-override", None).await?;
                Response::ok("")
            }
            ["data", "assets", ..] => {
                Response::error("Asset uploads are not supported on the cloud backend", 501)
            }
            // ProjectFileListResponse — the project *file browser*, not a dump of
            // the project. A Worker has no filesystem, so the list is empty.
            ["data", "db", "all"] => Response::from_json(&json!({
                "files": [],
                "lastLoadedProject": "",
            })),

            ["user", "translations", "translations.json"] => Response::from_json(&json!({})),

            // modules that depend on the filesystem, long-lived sockets or OAuth,
            // none of which a Worker provides — answered explicitly so the client
            // surfaces a real message instead of a bare 404
            ["data", "db", ..] => {
                Response::error("Project files are not supported on the cloud backend", 501)
            }
            ["data", "excel", ..] => Response::error(
                "Excel import/export is not supported on the cloud backend",
                501,
            ),
            ["data", "sheets", ..] => Response::error(
                "Google Sheets sync is not supported on the cloud backend",
                501,
            ),

            _ => Response::error(format!("Not implemented: {method} {path}"), 404),
        }
    }

    fn open_websocket(&mut self) -> Result<Response> {
        let pair = WebSocketPair::new()?;
        self.state.accept_web_socket(&pair.server);

        let init = json!({
            "tag": "client-init",
            "payload": {
                "clientId": uuid::Uuid::now_v7().to_string(),
                "clientName": "ontime-cloud",
            }
        });
        let _ = pair.server.send_with_str(init.to_string());

        // a client that connects mid-show needs the playback state straight away,
        // not on the next tick (and there is no tick at all when nothing is running)
        if let Some(rundown) = self.project.current() {
            let payload =
                self.project
                    .runtime
                    .snapshot(rundown, now_ms(), self.project.timezone_offset);
            let packet = json!({ "tag": "runtime-data", "payload": payload });
            let _ = pair.server.send_with_str(packet.to_string());
        }

        Response::from_websocket(pair.client)
    }

    // #region /data/rundowns =====================================

    async fn rundowns(
        &mut self,
        req: &mut Request,
        method: &Method,
        rest: &[&str],
    ) -> Result<Response> {
        match (method, rest) {
            // --- project rundowns ---
            (Method::Get, []) => Response::from_json(&self.project.rundown_list()),

            (Method::Post, []) => {
                let body = body(req).await;
                let title = body
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled")
                    .to_string();
                let id = uuid::Uuid::now_v7().to_string();
                self.project
                    .rundowns
                    .insert(id.clone(), ontime_core::Rundown::new(id, title));
                self.commit("project-rundowns", None).await?;
                Response::from_json(&self.project.rundown_list())
            }

            (Method::Get, ["current"]) => match self.project.current() {
                Some(rundown) => Response::from_json(rundown),
                None => Response::error("No rundown loaded", 404),
            },

            (Method::Post, ["import"]) => {
                Response::error("Import is not supported on the cloud backend", 501)
            }

            // --- a single rundown ---
            (Method::Get, [id]) => match self.project.rundowns.get(*id) {
                Some(rundown) => Response::from_json(rundown),
                None => Response::error("Rundown not found", 404),
            },

            (Method::Patch, [id]) => {
                let body = body(req).await;
                let Some(title) = body.get("title").and_then(Value::as_str) else {
                    return Response::error("Missing title", 400);
                };
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                rundown.title = title.to_string();
                self.commit("project-rundowns", None).await?;
                Response::from_json(&self.project.rundown_list())
            }

            (Method::Delete, [id]) => {
                if self.project.rundowns.len() <= 1 {
                    return Response::error("Cannot delete the only rundown", 400);
                }
                self.project.rundowns.remove(*id);
                self.project.ensure_consistent();
                self.commit("project-rundowns", None).await?;
                Response::from_json(&self.project.rundown_list())
            }

            (Method::Post, [id, "load"]) => {
                if !self.project.rundowns.contains_key(*id) {
                    return Response::error("Rundown not found", 404);
                }
                self.project.loaded = id.to_string();
                self.commit("all", Some(id.to_string())).await?;
                Response::from_json(&self.project.rundown_list())
            }

            (Method::Post, [id, "duplicate"]) => {
                let Some(source) = self.project.rundowns.get(*id).cloned() else {
                    return Response::error("Rundown not found", 404);
                };
                let new_id = uuid::Uuid::now_v7().to_string();
                let mut copy = source;
                copy.id = new_id.clone();
                copy.title = format!("{} (copy)", copy.title);
                self.project.rundowns.insert(new_id, copy);
                self.commit("project-rundowns", None).await?;
                Response::from_json(&self.project.rundown_list())
            }

            // --- entries ---
            (Method::Post, [id, "entry"]) => {
                let payload = body(req).await;
                let at = Insert::from_payload(&payload);
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                let entry = rundown.add_entry(&payload, at);
                self.commit("rundown", Some(id.to_string())).await?;
                Ok(Response::from_json(&entry)?.with_status(201))
            }

            (Method::Put, [id, "entry"]) => {
                let payload = body(req).await;
                let Some(entry_id) = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    return Response::error("Missing entry id", 400);
                };
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                let Some(entry) = rundown.edit_entry(&entry_id, &payload) else {
                    return Response::error("Entry not found", 404);
                };
                self.commit("rundown", Some(id.to_string())).await?;
                Response::from_json(&entry)
            }

            (Method::Put, [id, "batch"]) => {
                let payload = body(req).await;
                let ids = string_list(payload.get("ids"));
                let data = payload.get("data").cloned().unwrap_or(json!({}));
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                rundown.batch_edit(&ids, &data);
                self.respond_with_rundown(id).await
            }

            (Method::Delete, [id, "entries"]) => {
                let payload = body(req).await;
                let ids = string_list(payload.get("ids"));
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                rundown.delete_entries(&ids);
                self.respond_with_rundown(id).await
            }

            (Method::Delete, [id, "all"]) => {
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                rundown.delete_all();
                self.respond_with_rundown(id).await
            }

            (Method::Patch, [id, "reorder"]) => {
                let payload = body(req).await;
                let entry_id = payload
                    .get("entryId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let destination = payload
                    .get("destinationId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let order = payload
                    .get("order")
                    .and_then(Value::as_str)
                    .unwrap_or("after")
                    .to_string();
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                if !rundown.reorder(&entry_id, &destination, &order) {
                    return Response::error("Could not reorder entry", 400);
                }
                self.respond_with_rundown(id).await
            }

            (Method::Patch, [id, "swap"]) => {
                let payload = body(req).await;
                let from = payload
                    .get("from")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let to = payload
                    .get("to")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                if !rundown.swap(&from, &to) {
                    return Response::error("Could not swap entries", 400);
                }
                self.respond_with_rundown(id).await
            }

            (Method::Patch, [id, "renumber"]) => {
                let payload = body(req).await;
                let start_at = payload
                    .get("startAt")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                let increment = payload
                    .get("increment")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                rundown.renumber(start_at, increment);
                self.respond_with_rundown(id).await
            }

            (Method::Patch, [id, "applydelay", delay_id]) => {
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                if !rundown.apply_delay(delay_id) {
                    return Response::error("Delay not found", 404);
                }
                self.respond_with_rundown(id).await
            }

            (Method::Post, [id, "clone", entry_id]) => {
                let payload = body(req).await;
                let at = Insert::from_payload(&payload);
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                if rundown.clone_entry(entry_id, at).is_none() {
                    return Response::error("Entry not found", 404);
                }
                self.respond_with_rundown(id).await
            }

            (Method::Post, [id, "group"]) => {
                let payload = body(req).await;
                let ids = string_list(payload.get("ids"));
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                if rundown.group_entries(&ids).is_none() {
                    return Response::error("Could not group entries", 400);
                }
                self.respond_with_rundown(id).await
            }

            (Method::Post, [id, "ungroup", group_id]) => {
                let Some(rundown) = self.project.get_mut(id) else {
                    return Response::error("Rundown not found", 404);
                };
                if !rundown.ungroup(group_id) {
                    return Response::error("Group not found", 404);
                }
                self.respond_with_rundown(id).await
            }

            _ => Response::error("Not implemented", 404),
        }
    }

    /// Persists, notifies and returns the whole rundown — the shape most
    /// entry-mutating endpoints reply with.
    async fn respond_with_rundown(&mut self, id: &str) -> Result<Response> {
        self.commit("rundown", Some(id.to_string())).await?;
        match self.project.rundowns.get(id) {
            Some(rundown) => Response::from_json(rundown),
            None => Response::error("Rundown not found", 404),
        }
    }

    // #endregion /data/rundowns ==================================
    // #region project settings ===================================

    async fn project_data(
        &mut self,
        req: &mut Request,
        method: &Method,
        rest: &[&str],
    ) -> Result<Response> {
        if !rest.is_empty() {
            return Response::error("Not implemented", 404);
        }
        match method {
            Method::Get => Response::from_json(&self.project.project),
            Method::Post => {
                let patch = body(req).await;
                let mut merged = serde_json::to_value(&self.project.project)?;
                shallow_merge(&mut merged, &patch);
                self.project.project = serde_json::from_value(merged)
                    .map_err(|e| Error::RustError(format!("Invalid project data: {e}")))?;
                self.commit("project-data", None).await?;
                Response::from_json(&self.project.project)
            }
            _ => Response::error("Not implemented", 404),
        }
    }

    async fn settings(
        &mut self,
        req: &mut Request,
        method: &Method,
        rest: &[&str],
    ) -> Result<Response> {
        match (method, rest) {
            (Method::Get, []) => {
                let mut settings = serde_json::to_value(&self.project.settings)?;
                // not part of the client's Settings type — a Worker has no local
                // timezone, so the operator's UTC offset has to be configurable
                if let Some(obj) = settings.as_object_mut() {
                    obj.insert("timezoneOffset".into(), json!(self.project.timezone_offset));
                    obj.insert("heartbeatMs".into(), json!(self.project.heartbeat_ms));
                }
                Response::from_json(&settings)
            }
            (Method::Post, []) => {
                let patch = body(req).await;
                if let Some(offset) = patch.get("timezoneOffset").and_then(Value::as_i64) {
                    self.project.timezone_offset = offset;
                }
                if let Some(heartbeat) = patch.get("heartbeatMs").and_then(Value::as_i64) {
                    // clamped: below 250ms the wake-ups cost more than they help,
                    // above a minute a stale client looks broken
                    self.project.heartbeat_ms = if heartbeat <= 0 {
                        0
                    } else {
                        heartbeat.clamp(250, 60_000)
                    };
                    self.sync_alarm().await?;
                }
                let mut merged = serde_json::to_value(&self.project.settings)?;
                shallow_merge(&mut merged, &patch);
                self.project.settings = serde_json::from_value(merged)
                    .map_err(|e| Error::RustError(format!("Invalid settings: {e}")))?;
                self.commit("settings", None).await?;
                Response::from_json(&self.project.settings)
            }
            // PortInfo. The cloud backend has no configurable port; report the edge one.
            (Method::Get, ["serverport"]) => {
                Response::from_json(&json!({ "port": 443, "pendingRestart": false }))
            }
            (_, ["welcomedialog"]) => Response::from_json(&json!({ "status": "ok" })),
            _ => Response::error("Not implemented", 404),
        }
    }

    async fn view_settings(&mut self, req: &mut Request, method: &Method) -> Result<Response> {
        match method {
            Method::Get => Response::from_json(&self.project.view_settings),
            Method::Post => {
                let patch = body(req).await;
                let mut merged = serde_json::to_value(&self.project.view_settings)?;
                shallow_merge(&mut merged, &patch);
                self.project.view_settings = serde_json::from_value(merged)
                    .map_err(|e| Error::RustError(format!("Invalid view settings: {e}")))?;
                self.commit("view-settings", None).await?;
                Response::from_json(&self.project.view_settings)
            }
            _ => Response::error("Not implemented", 404),
        }
    }

    async fn custom_fields(
        &mut self,
        req: &mut Request,
        method: &Method,
        rest: &[&str],
    ) -> Result<Response> {
        match (method, rest) {
            (Method::Get, []) => Response::from_json(&self.project.custom_fields),
            (Method::Post, []) => {
                let field = body(req).await;
                let Some(key) = field
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    return Response::error("Missing label", 400);
                };
                if let Some(fields) = self.project.custom_fields.as_object_mut() {
                    fields.insert(key, field);
                }
                self.commit("custom-fields", None).await?;
                Response::from_json(&self.project.custom_fields)
            }
            (Method::Put, [key]) => {
                let field = body(req).await;
                if let Some(fields) = self.project.custom_fields.as_object_mut() {
                    fields.insert((*key).to_string(), field);
                }
                self.commit("custom-fields", None).await?;
                Response::from_json(&self.project.custom_fields)
            }
            (Method::Delete, [key]) => {
                if let Some(fields) = self.project.custom_fields.as_object_mut() {
                    fields.remove(*key);
                }
                self.commit("custom-fields", None).await?;
                Response::from_json(&self.project.custom_fields)
            }
            _ => Response::error("Not implemented", 404),
        }
    }

    async fn url_presets(
        &mut self,
        req: &mut Request,
        method: &Method,
        rest: &[&str],
    ) -> Result<Response> {
        match (method, rest) {
            (Method::Get, []) => Response::from_json(&self.project.url_presets),
            (Method::Post, []) => {
                let preset = body(req).await;
                self.project.url_presets.push(preset);
                self.commit("url-presets", None).await?;
                Response::from_json(&self.project.url_presets)
            }
            (Method::Put, [alias]) => {
                let preset = body(req).await;
                let position = self
                    .project
                    .url_presets
                    .iter()
                    .position(|p| p.get("alias").and_then(Value::as_str) == Some(*alias));
                match position {
                    Some(i) => self.project.url_presets[i] = preset,
                    None => self.project.url_presets.push(preset),
                }
                self.commit("url-presets", None).await?;
                Response::from_json(&self.project.url_presets)
            }
            (Method::Delete, [alias]) => {
                self.project
                    .url_presets
                    .retain(|p| p.get("alias").and_then(Value::as_str) != Some(*alias));
                self.commit("url-presets", None).await?;
                Response::from_json(&self.project.url_presets)
            }
            _ => Response::error("Not implemented", 404),
        }
    }

    // #endregion project settings ================================
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

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn shallow_merge(target: &mut Value, patch: &Value) {
    let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) else {
        return;
    };
    for (key, value) in patch {
        target.insert(key.clone(), value.clone());
    }
}

// #endregion helpers =============================================
