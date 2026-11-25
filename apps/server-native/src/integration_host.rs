//! The integration API's host side on the native server: `GET /api/...` and
//! websocket actions go through `ontime_core::integration`, as on the Worker
//! (`apps/server-rs/src/room_integration.rs`); what they changed is carried out
//! here, where the files and the sockets are.

use std::collections::HashMap;

use axum::{
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use ontime_core::api::{self, ApiRequest, HttpMethod};
use ontime_core::audit::Actor;
use ontime_core::audit_describe;
use ontime_core::integration::{self, Change, ClientCommand, HttpCall, Reply, HELLO_MESSAGE};
use serde_json::{json, Value};

use crate::{broadcast_client_list, now_ms, store, with_room, AppState};

/// Whether a path is the integration API's, once the room prefix is off.
pub fn is_integration_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// `GET /api/<action>/...`. Only a token counts, in the `Authorization`
/// header or `?token=`, never a cookie: these GETs change state, and a cookie
/// would let any page an operator opens press Start for them.
pub async fn http(
    state: &AppState,
    room_name: &str,
    method: &Method,
    api_path: &str,
    raw_query: Option<&str>,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
) -> Response {
    if method != Method::GET {
        return refusal(
            StatusCode::METHOD_NOT_ALLOWED,
            "The integration API answers GET only",
        );
    }
    let room_is_new = !store::exists(&state.data_dir, room_name).await
        && !state.rooms.lock().await.contains_key(room_name);
    if room_is_new {
        return refusal(StatusCode::NOT_FOUND, "This room does not exist");
    }
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .map(|token| token.trim().to_string());
    let access = ApiRequest {
        method: HttpMethod::Get,
        path: api_path.to_string(),
        body: json!({}),
        token: bearer.or_else(|| query.get("token").cloned()),
        master_token: state.master_token.clone(),
        room_is_new: false,
        member: None,
    };

    let path = api_path.strip_prefix("/api").unwrap_or_default();
    let (tag, payload) = match integration::http_call(path, raw_query) {
        Ok(HttpCall::Hello) => (String::new(), None),
        Ok(HttpCall::Action { tag, payload }) => (tag, payload),
        Err(error) => return refusal(StatusCode::BAD_REQUEST, &error.message),
    };
    let now = now_ms();
    let ((authorised, may_drive, actor, limited), _) = with_room(state, room_name, |room| {
        let authorised = api::is_authorised(&room.project, &access);
        let may_drive = api::may_control(&room.project, &access);
        // counted only once let in, so a stranger flooding the room cannot use
        // up the operator's calls
        let counted = authorised && !tag.is_empty() && (integration::is_read(&tag) || may_drive);
        let limited = match counted {
            true => room.rate_limit.take(&tag, now).err(),
            false => None,
        };
        (
            authorised,
            may_drive,
            audit_describe::actor(&room.project, &access, None),
            limited,
        )
    })
    .await;
    if !authorised {
        return refusal(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    if tag.is_empty() {
        return axum::Json(json!({ "message": HELLO_MESSAGE })).into_response();
    }
    // reading the version or the state is not driving a members-only room
    if !integration::is_read(&tag) && !may_drive {
        return refusal(StatusCode::FORBIDDEN, api::NEEDS_SIGN_IN);
    }
    if let Some(seconds) = limited {
        let mut response = refusal(StatusCode::TOO_MANY_REQUESTS, "Too many requests");
        if let Ok(value) = seconds.to_string().parse() {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        return response;
    }

    match run_action(state, room_name, &tag, payload.as_ref(), actor).await {
        Ok(payload) => (
            StatusCode::ACCEPTED,
            axum::Json(json!({ "payload": payload })),
        )
            .into_response(),
        Err(message) => refusal(StatusCode::BAD_REQUEST, &message),
    }
}

/// A websocket action. The reply goes back to the sender, as upstream; a
/// refusal is not answered.
pub async fn socket(
    state: &AppState,
    room_name: &str,
    actor: Actor,
    tag: &str,
    payload: Option<&Value>,
) -> Option<String> {
    let reply = run_action(state, room_name, tag, payload, actor)
        .await
        .ok()?;
    Some(json!({ "tag": tag, "payload": reply }).to_string())
}

/// Dispatches an action and carries out what it changed: the reply's payload,
/// or why it was refused.
async fn run_action(
    state: &AppState,
    room_name: &str,
    tag: &str,
    payload: Option<&Value>,
    actor: Actor,
) -> Result<Value, String> {
    let now = now_ms();
    let (outcome, _) = with_room(state, room_name, |room| {
        let before = room.project.runtime.selected_id.clone();
        let Reply {
            payload: reply,
            change,
        } = integration::handle(&mut room.project, tag, payload, now).map_err(|e| e.message)?;
        let mut writes = None;
        let mut save = None;
        match &change {
            Change::None | Change::Client(_) => {}
            Change::Runtime => {
                let note =
                    audit_describe::socket_note(&room.project, tag, payload, before.as_deref());
                writes = note.map(|note| room.audit(actor, note, now));
                room.broadcast_runtime(now);
                save = Some(room.project.clone());
            }
            Change::Rundown { rundown_id } => {
                // the edit, in the room's activity log as if made in the editor
                let note =
                    audit_describe::socket_note(&room.project, tag, payload, before.as_deref());
                writes = note.map(|note| room.audit(actor, note, now));
                room.broadcast_refetch("rundown", Some(rundown_id));
                // the edited entry may be the one on air
                room.broadcast_runtime(now);
                save = Some(room.project.clone());
            }
        }
        Ok::<_, String>((reply, change, writes, save))
    })
    .await;
    let (reply, change, writes, save) = outcome?;

    if let Some(writes) = writes {
        let _ = store::save_audit(&state.data_dir, room_name, &writes).await;
    }
    if let Some(project) = save {
        let _ = store::save(&state.data_dir, room_name, &project).await;
    }
    if let Change::Client(command) = change {
        client_command(state, room_name, &command).await?;
    }
    Ok(reply)
}

/// Renames, redirects or flags one connected screen of this room.
async fn client_command(
    state: &AppState,
    room_name: &str,
    command: &ClientCommand,
) -> Result<(), String> {
    let target = match command {
        ClientCommand::Rename { target, .. }
        | ClientCommand::Redirect { target, .. }
        | ClientCommand::Identify { target, .. } => target,
    };
    let packet = {
        let mut clients = state.clients.lock().await;
        let Some(client) = clients
            .get_mut(target)
            .filter(|client| client.room == room_name)
        else {
            return Err(format!("Client \"{target}\" not found"));
        };
        match command {
            ClientCommand::Rename { name, .. } => {
                client.name = name.clone();
                Some(
                    json!({ "tag": "client-rename", "payload": { "name": name, "target": target } }),
                )
            }
            ClientCommand::Redirect { path, .. } => Some(
                json!({ "tag": "client-redirect", "payload": { "target": target, "path": path } }),
            ),
            ClientCommand::Identify { identify, .. } => {
                client.identify = *identify;
                None
            }
        }
    };
    // every socket of the room hears it; each screen keeps only what is its own
    if let Some(packet) = &packet {
        if let Some(room) = state.rooms.lock().await.get(room_name) {
            room.publish(packet.clone());
        }
    }
    if !matches!(command, ClientCommand::Redirect { .. }) {
        broadcast_client_list(state, room_name, None).await;
    }
    Ok(())
}

/// Upstream answers a refused action with 500; it is the request that is
/// wrong, so it is a 4xx here, with the same `{ message }` body.
fn refusal(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(json!({ "message": message }))).into_response()
}
