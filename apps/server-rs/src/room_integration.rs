//! The integration API's host side: `GET /api/...` and websocket actions go
//! through `ontime_core::integration`, and what they changed is carried out
//! here, where the storage and the sockets are.
//!
//! A child of `room`, so it reaches the Durable Object's own fields.

use serde_json::{json, Value};
use worker::*;

use ontime_core::api::{self, ApiRequest, HttpMethod};
use ontime_core::audit::Actor;
use ontime_core::audit_describe;
use ontime_core::integration::{self, Change, ClientCommand, HttpCall, Reply, HELLO_MESSAGE};
use ontime_core::project::hash_password;

use super::{now_ms, ClientInfo, OntimeProject};

/// Whether a path is the integration API's, as the Durable Object receives it.
pub(super) fn is_integration_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

impl OntimeProject {
    /// `GET /api/<action>/...` from Companion, a Stream Deck or a script.
    ///
    /// State changes on a GET, so only a token counts here, in the
    /// `Authorization` header or `?token=`: never the browser's cookie or
    /// session, or any page an operator visits could press Start for them
    /// with an `<img src>`.
    pub(super) async fn integration_http(
        &mut self,
        req: &Request,
        room_name: &str,
    ) -> Result<Response> {
        if req.method() != Method::Get {
            return Response::error("The integration API answers GET only", 405);
        }
        if self.is_new {
            return Response::error("This room does not exist", 404);
        }
        let url = req.url()?;
        let token = bearer(req).or_else(|| {
            url.query_pairs()
                .find(|(key, _)| key == "token")
                .map(|(_, token)| token.to_string())
        });
        let access = ApiRequest {
            method: HttpMethod::Get,
            path: url.path().to_string(),
            body: json!({}),
            token,
            master_token: self
                ._env
                .secret("SESSION_PASSWORD")
                .ok()
                .map(|secret| hash_password(&secret.to_string())),
            room_is_new: false,
            member: None,
        };
        if !api::is_authorised(&self.project, &access) {
            return Response::error("Unauthorized", 401);
        }

        let path = url.path().strip_prefix("/api").unwrap_or_default();
        let (tag, payload) = match integration::http_call(path, url.query()) {
            Ok(HttpCall::Hello) => {
                return Response::from_json(&json!({ "message": HELLO_MESSAGE }))
            }
            Ok(HttpCall::Action { tag, payload }) => (tag, payload),
            Err(error) => return refusal(&error.message),
        };
        // a members-only room is driven by its members, its password or the
        // master password; reading the version or the state is not driving
        if !integration::is_read(&tag) && !api::may_control(&self.project, &access) {
            return Response::error(api::NEEDS_SIGN_IN, 403);
        }
        // counted only once let in, so a stranger flooding the room cannot
        // use up the operator's calls
        if let Err(seconds) = self.rate_limit.take(&tag, now_ms()) {
            let mut response =
                Response::from_json(&json!({ "message": "Too many requests" }))?.with_status(429);
            response
                .headers_mut()
                .set("Retry-After", &seconds.to_string())?;
            return Ok(response);
        }

        let actor = audit_describe::actor(&self.project, &access, None);
        match self.run_action(&tag, payload.as_ref(), actor).await? {
            Ok(payload) => {
                self.report_if_due(room_name).await;
                Ok(Response::from_json(&json!({ "payload": payload }))?.with_status(202))
            }
            Err(message) => refusal(&message),
        }
    }

    /// A websocket packet that is an action. Answers the sender with the
    /// action's reply, as upstream does; a refusal is not answered.
    pub(super) async fn integration_socket(
        &mut self,
        ws: &WebSocket,
        info: Option<ClientInfo>,
        tag: &str,
        payload: Option<&Value>,
    ) -> Result<()> {
        let actor = info.map_or(Actor::Anyone, |info| info.actor);
        if let Ok(reply) = self.run_action(tag, payload, actor).await? {
            let _ = ws.send_with_str(json!({ "tag": tag, "payload": reply }).to_string());
        }
        Ok(())
    }

    /// Dispatches an action and carries out what it changed. The inner result
    /// is the reply's payload, or why it was refused.
    async fn run_action(
        &mut self,
        tag: &str,
        payload: Option<&Value>,
        actor: Actor,
    ) -> Result<std::result::Result<Value, String>> {
        let now = now_ms();
        let before = self.project.runtime.selected_id.clone();
        let Reply {
            payload: reply,
            change,
        } = match integration::handle(&mut self.project, tag, payload, now) {
            Ok(reply) => reply,
            Err(error) => return Ok(Err(error.message)),
        };

        match change {
            Change::None => {}
            Change::Runtime => {
                let note =
                    audit_describe::socket_note(&self.project, tag, payload, before.as_deref());
                let writes = match note {
                    Some(note) => Some(self.audit(actor, note, now).await),
                    None => None,
                };
                self.persist(writes).await?;
                self.broadcast_runtime(now);
                self.sync_alarm().await?;
            }
            Change::Rundown { rundown_id } => {
                // the edit, in the room's activity log as if made in the editor
                let note =
                    audit_describe::socket_note(&self.project, tag, payload, before.as_deref());
                let writes = match note {
                    Some(note) => Some(self.audit(actor, note, now).await),
                    None => None,
                };
                self.persist(writes).await?;
                self.broadcast_refetch("rundown", Some(&rundown_id));
                // the edited entry may be the one on air
                self.broadcast_runtime(now);
                self.sync_alarm().await?;
            }
            Change::Client(command) => {
                if let Err(message) = self.client_command(&command) {
                    return Ok(Err(message));
                }
            }
        }
        Ok(Ok(reply))
    }

    /// Renames, redirects or flags one connected screen (upstream
    /// renameClient, redirectClient, identifyClient).
    fn client_command(&self, command: &ClientCommand) -> std::result::Result<(), String> {
        let target = match command {
            ClientCommand::Rename { target, .. }
            | ClientCommand::Redirect { target, .. }
            | ClientCommand::Identify { target, .. } => target,
        };
        let found = self.state.get_websockets().into_iter().find_map(|socket| {
            let info = socket
                .deserialize_attachment::<ClientInfo>()
                .ok()
                .flatten()?;
            (info.id == *target).then_some((socket, info))
        });
        let Some((socket, mut info)) = found else {
            return Err(format!("Client \"{target}\" not found"));
        };

        match command {
            ClientCommand::Rename { name, .. } => {
                info.name = name.clone();
                let _ = socket.serialize_attachment(&info);
                // only the screen being renamed needs to hear it
                let packet = json!({ "tag": "client-rename", "payload": { "name": name, "target": target } });
                let _ = socket.send_with_str(packet.to_string());
            }
            ClientCommand::Redirect { path, .. } => {
                let packet = json!({ "tag": "client-redirect", "payload": { "target": target, "path": path } });
                let _ = socket.send_with_str(packet.to_string());
                return Ok(());
            }
            ClientCommand::Identify { identify, .. } => {
                info.identify = *identify;
                let _ = socket.serialize_attachment(&info);
            }
        }
        self.broadcast_client_list(None);
        Ok(())
    }
}

/// A token in an `Authorization: Bearer` header.
fn bearer(req: &Request) -> Option<String> {
    let header = req.headers().get("Authorization").ok().flatten()?;
    header
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
}

/// Upstream answers a refused action with 500; it is the request that is
/// wrong, so it is a 400 here, with the same `{ message }` body.
fn refusal(message: &str) -> Result<Response> {
    Ok(Response::from_json(&json!({ "message": message }))?.with_status(400))
}
