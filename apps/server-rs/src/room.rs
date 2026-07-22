use worker::*;

use crate::types::{EventItem, PlaybackState, RundownState, WsAction};

const STORAGE_KEY: &str = "ontime_rundown_state";

#[durable_object]
pub struct OntimeRoom {
    state: State,
    _env: Env,
    rundown: RundownState,
}

#[durable_object]
#[async_trait::async_trait(?Send)]
impl DurableObject for OntimeRoom {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            _env: env,
            rundown: RundownState::default(),
        }
    }

    async fn fetch(&mut self, mut req: Request) -> Result<Response> {
        // Load persistent state from Durable Object storage on first access if needed
        if let Ok(saved) = self.state.storage().get::<RundownState>(STORAGE_KEY).await {
            self.rundown = saved;
        }

        let raw_path = req.path();
        let clean_path = raw_path.split('?').next().unwrap_or(&raw_path);
        let path = if let Some(idx) = clean_path.find("/data/") {
            &clean_path[idx..]
        } else if let Some(idx) = clean_path.find("/api/") {
            &clean_path[idx..]
        } else if let Some(idx) = clean_path.find("/ws") {
            &clean_path[idx..]
        } else {
            clean_path
        };

        match path {
            "/ws" => {
                let pair = WebSocketPair::new()?;
                let server_ws = pair.server;
                let client_ws = pair.client;

                self.state.accept_web_socket(&server_ws);

                // Initial state push
                let init_msg = WsAction::SyncState(self.rundown.clone());
                if let Ok(json) = serde_json::to_string(&init_msg) {
                    let _ = server_ws.send_with_str(&json);
                }

                Response::from_websocket(client_ws)
            }
            "/api/state" | "/data/rundowns/current" => Response::from_json(&self.rundown),
            "/api/v1/control/start" => {
                self.rundown.playback_state = PlaybackState::Running;
                let now = chrono::Utc::now().timestamp_millis() as u64;
                if self.rundown.started_at_utc.is_none() {
                    self.rundown.started_at_utc = Some(now);
                }
                let _ = self.state.storage().set_alarm(1000).await;
                Response::from_json(&serde_json::json!({ "status": "ok", "state": "Running" }))
            }
            "/api/v1/control/pause" => {
                self.rundown.playback_state = PlaybackState::Paused;
                Response::from_json(&serde_json::json!({ "status": "ok", "state": "Paused" }))
            }
            "/api/v1/control/reset" => {
                self.rundown.playback_state = PlaybackState::Stopped;
                self.rundown.elapsed_millis = 0;
                self.rundown.started_at_utc = None;
                Response::from_json(&serde_json::json!({ "status": "ok", "state": "Stopped" }))
            }
            "/api/v1/control/next" => {
                if self.rundown.current_event_index + 1 < self.rundown.events.len() {
                    self.rundown.current_event_index += 1;
                    self.rundown.elapsed_millis = 0;
                    self.rundown.started_at_utc = None;
                }
                Response::from_json(&serde_json::json!({ "status": "ok", "currentIndex": self.rundown.current_event_index }))
            }
            "/api/v1/control/previous" => {
                if self.rundown.current_event_index > 0 {
                    self.rundown.current_event_index -= 1;
                    self.rundown.elapsed_millis = 0;
                }
                Response::from_json(&serde_json::json!({ "status": "ok", "currentIndex": self.rundown.current_event_index }))
            }
            "/data/project" => Response::from_json(&self.rundown.project),
            "/data/settings" => Response::from_json(&self.rundown.settings),
            "/data/view-settings" => Response::from_json(&self.rundown.view_settings),
            "/data/rundowns" => {
                if req.method() == Method::Post {
                    if let Ok(json_body) = req.json::<serde_json::Value>().await {
                        if let Some(t) = json_body.get("title").and_then(|v| v.as_str()) {
                            self.rundown.project.title = t.to_string();
                        }
                    }
                    let _ = self.state.storage().put(STORAGE_KEY, &self.rundown).await;
                }
                let rundowns_list = serde_json::json!({
                    "loaded": self.rundown.id,
                    "rundowns": [
                        {
                            "id": self.rundown.id,
                            "title": self.rundown.project.title,
                            "numEntries": self.rundown.events.len(),
                            "revision": 1
                        }
                    ]
                });
                Response::from_json(&rundowns_list)
            }
            "/data/url-presets" => Response::from_json(&serde_json::json!([])),
            "/data/custom-views" => Response::from_json(&serde_json::json!([])),
            "/data/session" => Response::from_json(&serde_json::json!({ "activeConnections": 1 })),
            "/data/session/info" => Response::from_json(&serde_json::json!({ "version": "v3.13.1", "isCloud": true })),
            "/data/report" => Response::from_json(&serde_json::json!([])),
            "/data/custom-fields" => Response::from_json(&serde_json::json!({})),
            "/data/assets/css" => Response::ok(""),
            p if p.contains("/entry") => {
                if req.method() == Method::Post {
                    if let Ok(json_body) = req.json::<serde_json::Value>().await {
                        let cue = json_body.get("cue").and_then(|v| v.as_str()).unwrap_or("New Cue");
                        let title = json_body.get("title").and_then(|v| v.as_str()).unwrap_or("New Event");
                        let duration = json_body.get("duration").and_then(|v| v.as_u64()).unwrap_or(300);
                        let new_event = EventItem::new(cue, title, duration);
                        self.rundown.events.push(new_event.clone());
                        let _ = self.state.storage().put(STORAGE_KEY, &self.rundown).await;
                        Response::from_json(&new_event)
                    } else {
                        let new_event = EventItem::new("Cue", "New Event", 300);
                        self.rundown.events.push(new_event.clone());
                        let _ = self.state.storage().put(STORAGE_KEY, &self.rundown).await;
                        Response::from_json(&new_event)
                    }
                } else {
                    Response::from_json(&serde_json::json!({}))
                }
            }
            _ => Response::from_json(&serde_json::json!({})),
        }
    }

    async fn alarm(&mut self) -> Result<Response> {
        if self.rundown.playback_state == PlaybackState::Running {
            self.rundown.elapsed_millis += 1000;
            self.rundown.updated_at = chrono::Utc::now().timestamp_millis() as u64;

            // Broadcast tick
            let broadcast_msg = WsAction::SyncState(self.rundown.clone());
            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                for socket in self.state.get_websockets() {
                    let _ = socket.send_with_str(&json);
                }
            }

            // Persist tick state to DO storage
            let _ = self.state.storage().put(STORAGE_KEY, &self.rundown).await;

            // Schedule next 1-second alarm tick
            let _ = self.state.storage().set_alarm(1000).await;
        }

        Response::ok("alarm tick")
    }

    async fn websocket_message(&mut self, _ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        let text = match message {
            WebSocketIncomingMessage::String(s) => s,
            _ => return Ok(()),
        };

        let action: WsAction = match serde_json::from_str(&text) {
            Ok(act) => act,
            Err(_) => return Ok(()),
        };

        match action {
            WsAction::StartTimer => {
                if self.rundown.playback_state != PlaybackState::Running {
                    self.rundown.playback_state = PlaybackState::Running;
                    let now = chrono::Utc::now().timestamp_millis() as u64;
                    if self.rundown.started_at_utc.is_none() {
                        self.rundown.started_at_utc = Some(now);
                    }
                    let _ = self.state.storage().set_alarm(1000).await;
                }
            }
            WsAction::PauseTimer => {
                self.rundown.playback_state = PlaybackState::Paused;
            }
            WsAction::ResetTimer => {
                self.rundown.playback_state = PlaybackState::Stopped;
                self.rundown.elapsed_millis = 0;
                self.rundown.started_at_utc = None;
            }
            WsAction::NextEvent => {
                if self.rundown.current_event_index + 1 < self.rundown.events.len() {
                    self.rundown.current_event_index += 1;
                    self.rundown.elapsed_millis = 0;
                    self.rundown.started_at_utc = None;
                }
            }
            WsAction::PreviousEvent => {
                if self.rundown.current_event_index > 0 {
                    self.rundown.current_event_index -= 1;
                    self.rundown.elapsed_millis = 0;
                }
            }
            WsAction::AddEvent { cue, title, duration } => {
                self.rundown.events.push(EventItem::new(cue, title, duration));
            }
            WsAction::EditEvent { id, cue, title, duration, subtitle, presenter, note } => {
                if let Some(evt) = self.rundown.events.iter_mut().find(|e| e.id == id) {
                    if let Some(c) = cue { evt.cue = c; }
                    if let Some(t) = title { evt.title = t; }
                    if let Some(d) = duration { evt.duration = d; }
                    if subtitle.is_some() { evt.subtitle = subtitle; }
                    if presenter.is_some() { evt.presenter = presenter; }
                    if note.is_some() { evt.note = note; }
                }
            }
            WsAction::DeleteEvent { id } => {
                self.rundown.events.retain(|e| e.id != id);
                if self.rundown.current_event_index >= self.rundown.events.len() && !self.rundown.events.is_empty() {
                    self.rundown.current_event_index = self.rundown.events.len() - 1;
                }
            }
            WsAction::ReorderEvents { from_index, to_index } => {
                let len = self.rundown.events.len();
                if from_index < len && to_index < len {
                    let item = self.rundown.events.remove(from_index);
                    self.rundown.events.insert(to_index, item);
                }
            }
            WsAction::UpdateProjectTitle { title } => {
                self.rundown.project.title = title;
            }
            _ => {}
        }

        self.rundown.updated_at = chrono::Utc::now().timestamp_millis() as u64;
        self.rundown.checksum = self.rundown.compute_checksum();

        // Save state persistently in Cloudflare DO SQLite storage
        let _ = self.state.storage().put(STORAGE_KEY, &self.rundown).await;

        // Broadcast updated state to all connected web sockets
        let broadcast_msg = WsAction::SyncState(self.rundown.clone());
        if let Ok(json) = serde_json::to_string(&broadcast_msg) {
            for socket in self.state.get_websockets() {
                let _ = socket.send_with_str(&json);
            }
        }

        Ok(())
    }

    async fn websocket_close(&mut self, _ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        Ok(())
    }
}
