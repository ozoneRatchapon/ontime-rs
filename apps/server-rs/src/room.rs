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

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        // Load persistent state from Durable Object storage on first access if needed
        if let Ok(saved) = self.state.storage().get::<RundownState>(STORAGE_KEY).await {
            self.rundown = saved;
        }

        let path = req.path();

        match path.as_str() {
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
            "/data/project" => Response::from_json(&self.rundown.project),
            "/data/settings" => Response::from_json(&self.rundown.settings),
            "/data/view-settings" => Response::from_json(&self.rundown.view_settings),
            "/data/rundowns" => Response::from_json(&vec![&self.rundown]),
            "/data/custom-fields" => Response::from_json(&serde_json::json!({})),
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
            WsAction::UpdateProjectTitle { title } => {
                self.rundown.project.title = title;
            }
            _ => {}
        }

        self.rundown.updated_at = chrono::Utc::now().timestamp_millis() as u64;

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
