//! What a room shows its audience without a token (plan 023):
//!
//! - `GET /r/<room>/public` the agenda and where the show is, as JSON;
//! - `GET /public/ws?room=<room>` the same, live, for as long as it is open;
//! - `GET /r/<room>/public/agenda/` a page for attendees' phones.
//!
//! All answer 404 until the room turns its feed on. The audience's sockets are
//! tagged `public`: they only ever receive what `ontime_core::public_feed`
//! builds, anything they send is ignored, and they are never counted as
//! clients, so a crowd does not keep the room's heartbeat beating.

use std::cell::RefCell;

use serde_json::{json, Value};
use worker::*;

use ontime_core::custom_views::content_security_policy;
use ontime_core::public_feed::{self, MAX_LISTENERS};

use super::{now_ms, OntimeProject};

pub(super) const PUBLIC_TAG: &str = "public";

/// What was sent to the audience last, so a tick is not sent as a change. Lost
/// on eviction, which only means the next broadcast is sent whole.
#[derive(Default)]
pub(super) struct Sent {
    agenda: Option<String>,
    live: Option<String>,
}

pub(super) type LastSent = RefCell<Sent>;

pub(super) fn is_public_path(path: &str) -> bool {
    path == "/public" || path.starts_with("/public/")
}

fn not_open() -> Result<Response> {
    Ok(
        Response::from_json(&json!({ "message": "This room has no public page" }))?
            .with_status(404),
    )
}

impl OntimeProject {
    /// The room's own sockets: editors, screens, Companion. Not the audience.
    pub(super) fn operator_sockets(&self) -> Vec<WebSocket> {
        self.state
            .get_websockets()
            .into_iter()
            .filter(|socket| !self.is_public_socket(socket))
            .collect()
    }

    pub(super) fn is_public_socket(&self, socket: &WebSocket) -> bool {
        self.state
            .get_tags(socket)
            .iter()
            .any(|tag| tag == PUBLIC_TAG)
    }

    pub(super) async fn public_http(&mut self, req: &Request) -> Result<Response> {
        if self.is_new || !self.project.public_feed.enabled {
            return not_open();
        }
        let path = req.path();
        let is_upgrade = req
            .headers()
            .get("Upgrade")?
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
        match path.trim_end_matches('/') {
            "/public" if req.method() == Method::Get => {
                let now = now_ms();
                Response::from_json(&json!({
                    "agenda": public_feed::agenda(&self.project),
                    "live": public_feed::live(&self.project, now),
                }))
            }
            "/public/ws" if is_upgrade => self.open_public_socket(),
            "/public/agenda" if req.method() == Method::Get => {
                let origin = req.url()?.origin().ascii_serialization();
                let mut response = Response::ok(public_feed::AGENDA_PAGE)?;
                let headers = response.headers_mut();
                headers.set("Content-Type", "text/html; charset=utf-8")?;
                headers.set("Content-Security-Policy", &content_security_policy(&origin))?;
                headers.set("X-Content-Type-Options", "nosniff")?;
                headers.set("Referrer-Policy", "no-referrer")?;
                headers.set("Cache-Control", "no-store")?;
                Ok(response)
            }
            _ => not_open(),
        }
    }

    fn open_public_socket(&mut self) -> Result<Response> {
        if self.state.get_websockets_with_tag(PUBLIC_TAG).len() >= MAX_LISTENERS {
            return Ok(Response::from_json(
                &json!({ "message": "Too many people are watching this room" }),
            )?
            .with_status(503));
        }
        let pair = WebSocketPair::new()?;
        self.state
            .accept_websocket_with_tags(&pair.server, &[PUBLIC_TAG]);
        let now = now_ms();
        let packet = json!({
            "tag": "public-data",
            "payload": {
                "agenda": public_feed::agenda(&self.project),
                "live": public_feed::live(&self.project, now),
            },
        });
        let _ = pair.server.send_with_str(packet.to_string());
        Response::from_websocket(pair.client)
    }

    /// Sends the audience what changed since it was last sent: the agenda when
    /// the show was edited, where the show is when the operator acted or an
    /// event ended. A tick alone sends nothing; the page counts by itself.
    pub(super) fn publish_public(&self, now: i64) {
        let sockets = self.state.get_websockets_with_tag(PUBLIC_TAG);
        if sockets.is_empty() {
            return;
        }
        if !self.project.public_feed.enabled {
            // turned off: nobody is let on, and nobody stays
            for socket in sockets {
                let _ = socket.close(Some(1000), Some("This room closed its public page"));
            }
            *self.public_sent.borrow_mut() = Sent::default();
            return;
        }
        let agenda = public_feed::agenda(&self.project);
        let live = public_feed::live(&self.project, now);
        let agenda_text = agenda.to_string();
        let live_key = public_feed::live_key(&live);
        let mut sent = self.public_sent.borrow_mut();
        let mut payload = serde_json::Map::new();
        if sent.agenda.as_deref() != Some(agenda_text.as_str()) {
            payload.insert("agenda".into(), agenda);
            sent.agenda = Some(agenda_text);
        }
        if payload.contains_key("agenda") || sent.live.as_deref() != Some(live_key.as_str()) {
            payload.insert("live".into(), live);
            sent.live = Some(live_key);
        }
        if payload.is_empty() {
            return;
        }
        let packet = json!({ "tag": "public-data", "payload": Value::Object(payload) }).to_string();
        for socket in sockets {
            let _ = socket.send_with_str(&packet);
        }
    }
}
