//! The HTTP surface, independent of any transport.
//!
//! The Cloudflare Worker — and any other host — translates its own request type
//! into [`ApiRequest`], calls [`handle`], and carries out the [`Effect`]s it gets
//! back. Keeping the routing here rather than in the Worker is what stops a second
//! host from becoming a second implementation that quietly drifts from the first.

use serde_json::{json, Value};

use crate::project::{hash_password, token_matches, Project};
use crate::registry::Role;
use crate::rundown::Insert;
use crate::Rundown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Other,
}

impl HttpMethod {
    pub fn parse(method: &str) -> Self {
        match method.to_ascii_uppercase().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            _ => HttpMethod::Other,
        }
    }
}

pub struct ApiRequest {
    pub method: HttpMethod,
    /// Path from the API root. A host prefix is tolerated and trimmed.
    pub path: String,
    /// Parsed body; an empty object when there was none.
    pub body: Value,
    /// Bearer token, however the host chose to carry it.
    pub token: Option<String>,
    /// A master token that overrides a room's own, if the host has one configured.
    pub master_token: Option<String>,
    /// True when this room has never been used before.
    ///
    /// Any name creates a room on first use, which is fine for one operator and
    /// not fine for a public deployment: names can be squatted and abandoned
    /// rooms accumulate. When a master token is configured, creating one is
    /// gated behind it; with no master configured nothing changes.
    pub room_is_new: bool,
    /// What the signed-in person presenting a session is to this room, as the
    /// host looked it up. `None` for anyone not signed in or not a member.
    pub member: Option<Role>,
}

/// Something the host must do once a request has been handled.
///
/// Returned rather than performed, because persisting and broadcasting look
/// entirely different on a Durable Object and on a native server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Write the project down.
    Persist,
    /// Tell connected clients that a slice of state changed.
    Refetch {
        target: String,
        rundown_id: Option<String>,
    },
    /// Reconsider when to wake next.
    Reschedule,
}

impl Effect {
    fn refetch(target: &str, rundown_id: Option<String>) -> Self {
        Effect::Refetch {
            target: target.to_string(),
            rundown_id,
        }
    }
}

pub struct ApiResponse {
    pub status: u16,
    /// `None` for a body-less response such as 204, or when `text` is set.
    pub body: Option<Value>,
    /// Set when the body is not JSON, as the CSS override is not.
    pub text: Option<String>,
    pub effects: Vec<Effect>,
}

impl ApiResponse {
    pub fn json<T: serde::Serialize>(value: &T) -> Self {
        Self {
            status: 200,
            body: Some(serde_json::to_value(value).unwrap_or(Value::Null)),
            text: None,
            effects: Vec::new(),
        }
    }

    pub fn status<T: serde::Serialize>(status: u16, value: &T) -> Self {
        Self {
            status,
            ..Self::json(value)
        }
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: None,
            text: Some(body.into()),
            effects: Vec::new(),
        }
    }

    pub fn no_content() -> Self {
        Self {
            status: 204,
            body: None,
            text: None,
            effects: Vec::new(),
        }
    }

    pub fn error(message: impl Into<String>, status: u16) -> Self {
        Self {
            status,
            body: Some(json!({ "message": message.into() })),
            text: None,
            effects: Vec::new(),
        }
    }
}

/// Whether the request may act on this room.
///
/// A room with no password is open, matching the original server with
/// `SESSION_PASSWORD` unset, so adding this could not lock anyone out.
///
/// Public because a websocket upgrade never reaches [`handle`] — the host owns
/// the socket — but must be gated by exactly the same rule.
pub fn is_authorised(project: &Project, req: &ApiRequest) -> bool {
    // a member is let in without knowing the room password
    if req.member.is_some() {
        return true;
    }
    let Some(room_hash) = project.password_hash.as_deref() else {
        return true;
    };
    let Some(presented) = req.token.as_deref() else {
        return false;
    };
    if token_matches(room_hash, presented) {
        return true;
    }
    match req.master_token.as_deref() {
        Some(master) => token_matches(master, presented),
        None => false,
    }
}

/// Whether the caller holds more than a link: a membership, the room
/// password, or the master password.
pub fn is_privileged(project: &Project, req: &ApiRequest) -> bool {
    if req.member.is_some() {
        return true;
    }
    let Some(presented) = req.token.as_deref() else {
        return false;
    };
    let room = project.password_hash.as_deref().is_some_and(|hash| token_matches(hash, presented));
    let master = req.master_token.as_deref().is_some_and(|master| token_matches(master, presented));
    room || master
}

/// Whether the caller may change the room — edit it, or drive its timers.
/// Only a members-only room asks for more than being let in.
pub fn may_control(project: &Project, req: &ApiRequest) -> bool {
    !project.members_only || is_privileged(project, req)
}

/// Turning members-only on or off is for an owner or the admin — not an
/// operator, and not the room password, which members-only is there to replace.
fn may_switch_members_only(req: &ApiRequest) -> bool {
    let master = match (req.master_token.as_deref(), req.token.as_deref()) {
        (Some(master), Some(presented)) => token_matches(master, presented),
        _ => false,
    };
    master || req.member == Some(Role::Owner)
}

pub const NEEDS_SIGN_IN: &str = "This room needs sign-in to edit. Ask its owner for an invite link.";

/// Whether a request is allowed to bring a new room into existence.
///
/// Open when no master token is configured, which is how a single operator or a
/// LAN install is expected to run. Once one is configured, a new room needs it —
/// existing rooms are unaffected, so turning this on cannot lock anyone out of
/// what they already have.
pub fn may_create_room(req: &ApiRequest) -> bool {
    if !req.room_is_new {
        return true;
    }
    let Some(master) = req.master_token.as_deref() else {
        return true;
    };
    req.token
        .as_deref()
        .is_some_and(|presented| token_matches(master, presented))
}

/// The scheme+host+port of a URL, dropping any path — the equivalent of reading
/// `.origin` off a `URL`. The client sends its own origin as `baseUrl`.
fn origin_of(base_url: &str) -> &str {
    match base_url.find("://") {
        Some(scheme) => {
            let after = scheme + 3;
            match base_url[after..].find('/') {
                Some(slash) => &base_url[..after + slash],
                None => base_url.trim_end_matches('/'),
            }
        }
        None => base_url.trim_end_matches('/'),
    }
}

/// Builds a pre-authenticated share URL, ported from the original server's
/// `generateShareUrl`. The client supplies its own origin as `baseUrl` and the
/// target view as `path`; a locked preset masks the path, `lockNav` adds `n=1`,
/// and `authenticate` injects the room token (which is the password hash).
///
/// Returns `None` when `baseUrl` is missing.
pub fn generate_share_url(body: &Value, room_hash: Option<&str>) -> Option<String> {
    let base_url = body.get("baseUrl").and_then(Value::as_str)?;
    let canonical_path = body.get("path").and_then(Value::as_str).unwrap_or("");
    let flag = |key: &str| body.get(key).and_then(Value::as_bool).unwrap_or(false);
    let str_opt = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };

    let preset = str_opt("preset");
    let prefix = str_opt("prefix");
    let hash = str_opt("hash").or(room_hash);

    let mut query: Vec<String> = Vec::new();
    let path = if canonical_path == "<<companion>>" {
        // companion links point to the root
        String::new()
    } else {
        // a locked preset hides the real path behind `preset/<name>`
        let should_mask = preset.is_some() && (canonical_path == "cuesheet" || flag("lockConfig"));
        let target = if should_mask {
            format!("preset/{}", preset.unwrap())
        } else {
            preset.unwrap_or(canonical_path).to_string()
        };
        if flag("lockNav") {
            query.push("n=1".to_string());
        }
        match prefix {
            Some(prefix) => format!("{prefix}/{target}"),
            None => target,
        }
    };

    if flag("authenticate") {
        if let Some(hash) = hash {
            query.push(format!("token={hash}"));
        }
    }

    let suffix = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };
    Some(format!("{}/{}{}", origin_of(base_url), path, suffix))
}

/// The key a custom field is stored under, matching the original's
/// `customFieldLabelToKey`.
fn custom_field_key(label: &str) -> String {
    label.trim().replace(' ', "_")
}

/// The original accepts only letters, numbers and spaces in a label.
fn is_valid_custom_field_label(label: &str) -> bool {
    let trimmed = label.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ')
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

pub fn handle(project: &mut Project, req: &ApiRequest, now: i64) -> ApiResponse {
    let mut effects: Vec<Effect> = Vec::new();
    let path = req
        .path
        .split('?')
        .next()
        .unwrap_or(&req.path)
        .trim_end_matches('/');

    // a caller may hand us the whole original URL path, so trim anything before
    // the API root
    let path = ["/data/", "/user/"]
        .iter()
        .find_map(|prefix| path.find(prefix).map(|i| &path[i..]))
        .unwrap_or(path);

    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    // Setting the password is allowed while the room is still open, so a new
    // room can be claimed; afterwards it needs the current token like anything else.
    let is_claim = matches!(segments.as_slice(), ["data", "session", "password"])
        && project.password_hash.is_none();

    if !is_claim && !is_authorised(project, req) {
        return ApiResponse::error("Unauthorized", 401);
    }

    if !may_create_room(req) {
        return ApiResponse::error(
            "This room does not exist. Creating one requires the master password.",
            403,
        );
    }

    // reading is decided above; anything else changes the room
    if req.method != HttpMethod::Get && !may_control(project, req) {
        return ApiResponse::error(NEEDS_SIGN_IN, 403);
    }

    let mut response = match segments.as_slice() {
        ["data", "rundowns", rest @ ..] => rundowns(project, req, rest, &mut effects),
        ["data", "project", rest @ ..] => project_data(project, req, rest, &mut effects),
        ["data", "settings", rest @ ..] => settings(project, req, rest, &mut effects),
        ["data", "view-settings"] => view_settings(project, req, &mut effects),
        ["data", "custom-fields", rest @ ..] => custom_fields(project, req, rest, &mut effects),
        ["data", "url-presets", rest @ ..] => url_presets(project, req, rest, &mut effects),
        // automations drive OSC/HTTP integrations, which a Worker cannot host;
        // the settings are stored and served, but never acted on
        ["data", "automations", ..] if req.method == HttpMethod::Get => {
            ApiResponse::json(&project.automation)
        }
        ["data", "automations", ..] => {
            ApiResponse::error("Automations are not supported on the cloud backend", 501)
        }
        ["data", "custom-views", ..] => ApiResponse::json(&json!({ "views": [] })),
        // permanent history — never overwritten, readable for analysis/export
        ["data", "report", "archive"] if req.method == HttpMethod::Get => {
            ApiResponse::json(&project.report_archive.runs())
        }
        ["data", "report", "archive"] if req.method == HttpMethod::Post => {
            // snapshot the currently-loaded rundown's run on demand — used before
            // re-running the same rundown, which would otherwise overwrite it
            let loaded = project.loaded.clone();
            let archived = project.archive_run(&loaded, now);
            if archived {
                effects.push(Effect::refetch("report", None));
            }
            ApiResponse::json(&json!({
                "archived": archived,
                "runs": project.report_archive.len(),
            }))
        }
        ["data", "report"] if req.method == HttpMethod::Get => ApiResponse::json(&project.report),
        ["data", "report", "all"] if req.method == HttpMethod::Delete => {
            project.report.clear_all();
            effects.push(Effect::refetch("report", None));
            ApiResponse::json(&project.report)
        }
        ["data", "report", id] if req.method == HttpMethod::Delete => {
            project.report.clear_one(id);
            effects.push(Effect::refetch("report", None));
            ApiResponse::json(&project.report)
        }
        ["data", "report", ..] => ApiResponse::json(&project.report),
        ["data", "members-only"] if req.method == HttpMethod::Get => {
            ApiResponse::json(&json!({ "membersOnly": project.members_only }))
        }
        ["data", "members-only"] => {
            if !may_switch_members_only(req) {
                return ApiResponse::error("Only an owner of this room can change who may edit it", 403);
            }
            match req.body.get("on").and_then(Value::as_bool) {
                Some(on) => {
                    project.members_only = on;
                    effects.push(Effect::Persist);
                    ApiResponse::json(&json!({ "membersOnly": on }))
                }
                None => ApiResponse::error("Missing on", 400),
            }
        }
        ["data", "session", "password"] => {
            let body = req.body.clone();
            match body.get("password").and_then(Value::as_str) {
                Some(password) if !password.is_empty() => {
                    let hash = hash_password(password);
                    project.password_hash = Some(hash.clone());
                    effects.push(Effect::Persist);
                    // the hash doubles as the bearer token; it is returned once,
                    // here, because there is no way to recover it from the hash
                    ApiResponse::json(&json!({ "protected": true, "token": hash }))
                }
                // an explicit null clears it, reopening the room
                _ if body.get("password").map(Value::is_null).unwrap_or(false) => {
                    project.password_hash = None;
                    effects.push(Effect::Persist);
                    ApiResponse::json(&json!({ "protected": false }))
                }
                _ => ApiResponse::error("Missing password", 400),
            }
        }
        // GetInfo: the client reads every field, so none may be omitted
        ["data", "session", "info"] => ApiResponse::json(&json!({
            "networkInterfaces": [],
            "version": project.settings.version,
            "serverPort": 443,
            "publicDir": "",
            // lets the client show whether this room is locked without a failed request
            "protected": project.password_hash.is_some(),
            "membersOnly": project.members_only,
            // whether this caller may change the room, so the editor can say so
            // up front instead of failing on the first click
            "canEdit": may_control(project, req),
        })),
        // the client passes its own origin as `baseUrl`, so the URL can be built
        // here without the Worker ever needing to serve HTML
        ["data", "session", "url"] => {
            match generate_share_url(&req.body, project.password_hash.as_deref()) {
                Some(url) => ApiResponse::json(&json!({ "url": url })),
                None => ApiResponse::error("Missing baseUrl", 400),
            }
        }
        ["data", "session", ..] => ApiResponse::json(&json!({ "clients": {} })),
        ["data", "assets", "css"] if req.method == HttpMethod::Get => {
            ApiResponse::text(project.css_override.clone())
        }
        ["data", "assets", "css"] => {
            let css = req
                .body
                .get("css")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            project.css_override = css;
            effects.push(Effect::refetch("css-override", None));
            ApiResponse::no_content()
        }
        ["data", "assets", "css", "restore"] => {
            project.css_override.clear();
            effects.push(Effect::refetch("css-override", None));
            ApiResponse::text("")
        }
        ["data", "assets", ..] => {
            ApiResponse::error("Asset uploads are not supported on the cloud backend", 501)
        }
        // ProjectFileListResponse — the project *file browser*, not a dump of
        // the project. A Worker has no filesystem, so the list is empty.
        ["data", "db", "all"] => ApiResponse::json(&json!({
            "files": [],
            "lastLoadedProject": "",
        })),

        // export the whole room as a downloadable backup — schedule, settings and
        // the permanent report archive. The password hash, which doubles as the
        // room's access token, is deliberately left out of the file.
        ["data", "db", "download"] if req.method == HttpMethod::Post => {
            let mut snapshot = serde_json::to_value(&*project).unwrap_or(Value::Null);
            if let Some(object) = snapshot.as_object_mut() {
                object.remove("passwordHash");
            }
            ApiResponse::json(&snapshot)
        }

        // restore a room from a downloaded backup. The room keeps its current
        // password (the backup never carried one), and playback starts stopped
        // rather than resuming from whenever the snapshot was taken.
        ["data", "db", "restore"] if req.method == HttpMethod::Post => {
            match serde_json::from_value::<Project>(req.body.clone()) {
                Ok(mut restored) => {
                    restored.password_hash = project.password_hash.clone();
                    restored.runtime = Default::default();
                    *project = restored;
                    project.ensure_consistent();
                    effects.push(Effect::Persist);
                    effects.push(Effect::refetch("all", None));
                    effects.push(Effect::Reschedule);
                    ApiResponse::json(&json!({ "restored": true }))
                }
                Err(error) => ApiResponse::error(format!("Invalid backup file: {error}"), 400),
            }
        }

        ["user", "translations", "translations.json"] => ApiResponse::json(&json!({})),

        // modules that depend on the filesystem, long-lived sockets or OAuth,
        // none of which a Worker provides — answered explicitly so the client
        // surfaces a real message instead of a bare 404
        ["data", "db", ..] => {
            ApiResponse::error("Project files are not supported on the cloud backend", 501)
        }
        ["data", "excel", ..] => ApiResponse::error(
            "Excel import/export is not supported on the cloud backend",
            501,
        ),
        ["data", "sheets", ..] => ApiResponse::error(
            "Google Sheets sync is not supported on the cloud backend",
            501,
        ),

        _ => ApiResponse::error(format!("Not implemented: {:?} {path}", req.method), 404),
    };

    response.effects.append(&mut effects);
    response
}

// #region /data/rundowns =====================================

fn rundowns(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match (&req.method, rest) {
        // --- project rundowns ---
        (HttpMethod::Get, []) => ApiResponse::json(&project.rundown_list()),

        (HttpMethod::Post, []) => {
            let body = req.body.clone();
            let title = body
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled")
                .to_string();
            let id = uuid::Uuid::now_v7().to_string();
            project.rundowns.insert(id.clone(), Rundown::new(id, title));
            effects.push(Effect::refetch("project-rundowns", None));
            ApiResponse::json(&project.rundown_list())
        }

        (HttpMethod::Get, ["current"]) => match project.current() {
            Some(rundown) => ApiResponse::json(rundown),
            None => ApiResponse::error("No rundown loaded", 404),
        },

        (HttpMethod::Post, ["import"]) => {
            ApiResponse::error("Import is not supported on the cloud backend", 501)
        }

        // --- a single rundown ---
        (HttpMethod::Get, [id]) => match project.rundowns.get(*id) {
            Some(rundown) => ApiResponse::json(rundown),
            None => ApiResponse::error("Rundown not found", 404),
        },

        (HttpMethod::Patch, [id]) => {
            let body = req.body.clone();
            let Some(title) = body.get("title").and_then(Value::as_str) else {
                return ApiResponse::error("Missing title", 400);
            };
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            rundown.title = title.to_string();
            effects.push(Effect::refetch("project-rundowns", None));
            ApiResponse::json(&project.rundown_list())
        }

        (HttpMethod::Delete, [id]) => {
            if project.rundowns.len() <= 1 {
                return ApiResponse::error("Cannot delete the only rundown", 400);
            }
            if let Some(removed) = project.rundowns.remove(*id) {
                // the report is room-wide; drop the gone rundown's records with it
                for entry_id in removed.entries.keys() {
                    project.report.clear_one(entry_id);
                }
            }
            project.ensure_consistent();
            effects.push(Effect::refetch("project-rundowns", None));
            ApiResponse::json(&project.rundown_list())
        }

        (HttpMethod::Post, [id, "load"]) => {
            if !project.rundowns.contains_key(*id) {
                return ApiResponse::error("Rundown not found", 404);
            }
            if project.loaded != *id {
                project.loaded = id.to_string();
                // Switching stops playback — the loaded event does not exist in
                // the new rundown, and you cannot run two at once (upstream does
                // the same). But it deliberately does NOT touch the report:
                // switching can be an accidental tap or just a peek at another
                // day while a show is still running, and clearing the report
                // there would split the run in two and lose the operator's place.
                // Preserving a finished run to the permanent archive is an
                // explicit action — POST /data/report/archive — not a side
                // effect of switching. Coming back and pressing play continues
                // the same live report, uninterrupted.
                project.runtime = Default::default();
                effects.push(Effect::Reschedule);
            }
            effects.push(Effect::refetch("all", Some(id.to_string())));
            ApiResponse::json(&project.rundown_list())
        }

        (HttpMethod::Post, [id, "duplicate"]) => {
            let Some(source) = project.rundowns.get(*id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            let copy = source.duplicate(format!("{} (copy)", source.title));
            project.rundowns.insert(copy.id.clone(), copy);
            effects.push(Effect::refetch("project-rundowns", None));
            ApiResponse::json(&project.rundown_list())
        }

        // --- entries ---
        (HttpMethod::Post, [id, "entry"]) => {
            let payload = req.body.clone();
            let at = Insert::from_payload(&payload);
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            let entry = rundown.add_entry(&payload, at);
            effects.push(Effect::refetch("rundown", Some(id.to_string())));
            ApiResponse::status(201, &entry)
        }

        (HttpMethod::Put, [id, "entry"]) => {
            let payload = req.body.clone();
            let Some(entry_id) = payload
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                return ApiResponse::error("Missing entry id", 400);
            };
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            let Some(entry) = rundown.edit_entry(&entry_id, &payload) else {
                return ApiResponse::error("Entry not found", 404);
            };
            effects.push(Effect::refetch("rundown", Some(id.to_string())));
            ApiResponse::json(&entry)
        }

        (HttpMethod::Put, [id, "batch"]) => {
            let payload = req.body.clone();
            let ids = string_list(payload.get("ids"));
            let data = payload.get("data").cloned().unwrap_or(json!({}));
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            rundown.batch_edit(&ids, &data);
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Delete, [id, "entries"]) => {
            let payload = req.body.clone();
            let ids = string_list(payload.get("ids"));
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            let was: Vec<String> = rundown.entries.keys().cloned().collect();
            rundown.delete_entries(&ids);
            project.prune_report(&was, id);
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Delete, [id, "all"]) => {
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            let was: Vec<String> = rundown.entries.keys().cloned().collect();
            rundown.delete_all();
            project.prune_report(&was, id);
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Patch, [id, "reorder"]) => {
            let payload = req.body.clone();
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
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if !rundown.reorder(&entry_id, &destination, &order) {
                return ApiResponse::error("Could not reorder entry", 400);
            }
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Patch, [id, "swap"]) => {
            let payload = req.body.clone();
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
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if !rundown.swap(&from, &to) {
                return ApiResponse::error("Could not swap entries", 400);
            }
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Patch, [id, "renumber"]) => {
            let payload = req.body.clone();
            let start_at = payload
                .get("startAt")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let increment = payload
                .get("increment")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            rundown.renumber(start_at, increment);
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Patch, [id, "applydelay", delay_id]) => {
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if !rundown.apply_delay(delay_id) {
                return ApiResponse::error("Delay not found", 404);
            }
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Post, [id, "clone", entry_id]) => {
            let payload = req.body.clone();
            let at = Insert::from_payload(&payload);
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if rundown.clone_entry(entry_id, at).is_none() {
                return ApiResponse::error("Entry not found", 404);
            }
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Post, [id, "group"]) => {
            let payload = req.body.clone();
            let ids = string_list(payload.get("ids"));
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if rundown.group_entries(&ids).is_none() {
                return ApiResponse::error("Could not group entries", 400);
            }
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Post, [id, "ungroup", group_id]) => {
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if !rundown.ungroup(group_id) {
                return ApiResponse::error("Group not found", 404);
            }
            respond_with_rundown(project, id, effects)
        }

        _ => ApiResponse::error("Not implemented", 404),
    }
}

/// Persists, notifies and returns the whole rundown — the shape most
/// entry-mutating endpoints reply with.
fn respond_with_rundown(project: &mut Project, id: &str, effects: &mut Vec<Effect>) -> ApiResponse {
    effects.push(Effect::refetch("rundown", Some(id.to_string())));
    match project.rundowns.get(id) {
        Some(rundown) => ApiResponse::json(rundown),
        None => ApiResponse::error("Rundown not found", 404),
    }
}

// #endregion /data/rundowns ==================================
// #region project settings ===================================

fn project_data(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    if !rest.is_empty() {
        return ApiResponse::error("Not implemented", 404);
    }
    match req.method {
        HttpMethod::Get => ApiResponse::json(&project.project),
        HttpMethod::Post => {
            let patch = req.body.clone();
            let mut merged = serde_json::to_value(&project.project).unwrap_or_default();
            shallow_merge(&mut merged, &patch);
            project.project = serde_json::from_value(merged).unwrap_or_default();
            effects.push(Effect::refetch("project-data", None));
            ApiResponse::json(&project.project)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

fn settings(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match (&req.method, rest) {
        (HttpMethod::Get, []) => {
            let mut settings = serde_json::to_value(&project.settings).unwrap_or_default();
            // not part of the client's Settings type — a Worker has no local
            // timezone, so the operator's UTC offset has to be configurable
            if let Some(obj) = settings.as_object_mut() {
                obj.insert("timezoneOffset".into(), json!(project.timezone_offset));
                obj.insert("heartbeatMs".into(), json!(project.heartbeat_ms));
            }
            ApiResponse::json(&settings)
        }
        (HttpMethod::Post, []) => {
            let patch = req.body.clone();
            if let Some(offset) = patch.get("timezoneOffset").and_then(Value::as_i64) {
                project.timezone_offset = offset;
            }
            if let Some(heartbeat) = patch.get("heartbeatMs").and_then(Value::as_i64) {
                // clamped: below 250ms the wake-ups cost more than they help,
                // above a minute a stale client looks broken
                project.heartbeat_ms = if heartbeat <= 0 {
                    0
                } else {
                    heartbeat.clamp(250, 60_000)
                };
                effects.push(Effect::Reschedule);
            }
            let mut merged = serde_json::to_value(&project.settings).unwrap_or_default();
            shallow_merge(&mut merged, &patch);
            project.settings = serde_json::from_value(merged).unwrap_or_default();
            effects.push(Effect::refetch("settings", None));
            ApiResponse::json(&project.settings)
        }
        // PortInfo. The cloud backend has no configurable port; report the edge one.
        (HttpMethod::Get, ["serverport"]) => {
            ApiResponse::json(&json!({ "port": 443, "pendingRestart": false }))
        }
        (_, ["welcomedialog"]) => ApiResponse::json(&json!({ "status": "ok" })),
        _ => ApiResponse::error("Not implemented", 404),
    }
}

fn view_settings(
    project: &mut Project,
    req: &ApiRequest,
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match req.method {
        HttpMethod::Get => ApiResponse::json(&project.view_settings),
        HttpMethod::Post => {
            let patch = req.body.clone();
            let mut merged = serde_json::to_value(&project.view_settings).unwrap_or_default();
            shallow_merge(&mut merged, &patch);
            project.view_settings = serde_json::from_value(merged).unwrap_or_default();
            effects.push(Effect::refetch("view-settings", None));
            ApiResponse::json(&project.view_settings)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

fn custom_fields(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match (&req.method, rest) {
        (HttpMethod::Get, []) => ApiResponse::json(&project.custom_fields),
        (HttpMethod::Post, []) => {
            let field = req.body.clone();
            let Some(label) = field.get("label").and_then(Value::as_str) else {
                return ApiResponse::error("Missing label", 400);
            };
            if !is_valid_custom_field_label(label) {
                // the original validates this, so accepting more here would let
                // us create fields the real client cannot handle
                return ApiResponse::error(
                    "A custom field label may only contain letters, numbers and spaces",
                    400,
                );
            }
            let key = custom_field_key(label);
            if let Some(fields) = project.custom_fields.as_object_mut() {
                fields.insert(key, field);
            }
            effects.push(Effect::refetch("custom-fields", None));
            ApiResponse::json(&project.custom_fields)
        }
        (HttpMethod::Put, [key]) => {
            let field = req.body.clone();
            if let Some(fields) = project.custom_fields.as_object_mut() {
                fields.insert((*key).to_string(), field);
            }
            effects.push(Effect::refetch("custom-fields", None));
            ApiResponse::json(&project.custom_fields)
        }
        (HttpMethod::Delete, [key]) => {
            if let Some(fields) = project.custom_fields.as_object_mut() {
                fields.remove(*key);
            }
            effects.push(Effect::refetch("custom-fields", None));
            ApiResponse::json(&project.custom_fields)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

fn url_presets(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match (&req.method, rest) {
        (HttpMethod::Get, []) => ApiResponse::json(&project.url_presets),
        (HttpMethod::Post, []) => {
            let preset = req.body.clone();
            project.url_presets.push(preset);
            effects.push(Effect::refetch("url-presets", None));
            ApiResponse::json(&project.url_presets)
        }
        (HttpMethod::Put, [alias]) => {
            let preset = req.body.clone();
            let position = project
                .url_presets
                .iter()
                .position(|p| p.get("alias").and_then(Value::as_str) == Some(*alias));
            match position {
                Some(i) => project.url_presets[i] = preset,
                None => project.url_presets.push(preset),
            }
            effects.push(Effect::refetch("url-presets", None));
            ApiResponse::json(&project.url_presets)
        }
        (HttpMethod::Delete, [alias]) => {
            project
                .url_presets
                .retain(|p| p.get("alias").and_then(Value::as_str) != Some(*alias));
            effects.push(Effect::refetch("url-presets", None));
            ApiResponse::json(&project.url_presets)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

// #endregion project settings ================================

/// How long until the host next has to do something, in milliseconds.
///
/// `None` means nothing is moving and the host may sleep indefinitely — on the
/// Worker that is the difference between an idle room costing nothing and costing
/// a request a second, and on a native server it is the difference between an
/// idle process being quiet and being busy.
///
/// `has_clients` says whether anyone is connected to hear a broadcast. With
/// nobody there the heartbeat is pointless, and a room whose timer was left
/// running would otherwise wake for ever at somebody's expense. The end-of-event
/// wake is kept regardless, because that one advances the rundown and there is
/// one of it per event rather than one a second.
///
/// This lives here rather than in either host because it is policy, not
/// transport: the two answering it differently is exactly how they would drift.
pub fn next_wake_in(project: &Project, now: i64, has_clients: bool) -> Option<i64> {
    let runtime = &project.runtime;
    let heartbeat = if has_clients {
        project.heartbeat_ms.max(0)
    } else {
        0
    };

    if !runtime.is_running() {
        // A paused show is still losing time against the schedule and the
        // operator is watching that figure grow, so the tick continues. An aux
        // timer runs independently of the rundown too: without this a stopwatch
        // started while playback is stopped would never update.
        let needs_tick =
            runtime.playback == crate::runtime::Playback::Pause || project.aux_timers.any_running();

        return (needs_tick && heartbeat > 0).then_some(heartbeat);
    }

    let rundown = project.current()?;
    let event = runtime.selected_id.as_ref().and_then(|id| rundown.get(id));
    let inputs = runtime.inputs_for(event, project.timezone_offset);

    // In roll mode the host is the only thing that will start the event, so it
    // wakes at the exact moment rather than waiting for a heartbeat.
    if let Some(target) = runtime.secondary_target {
        let clock = (now + project.timezone_offset).rem_euclid(crate::rundown::MS_PER_DAY);
        let until_due = (target - clock).max(0);
        let delay = if heartbeat > 0 {
            until_due.min(heartbeat)
        } else {
            until_due
        };
        return Some(delay.max(50));
    }

    // roll hands over to the next event on its own, so it needs the wake even
    // without an end action
    let has_end_action = runtime.playback == crate::runtime::Playback::Roll
        || event
            .and_then(|e| e.get("endAction"))
            .and_then(Value::as_str)
            .is_some_and(|action| action != "none");

    let until_end = match (has_end_action, inputs.ends_at(now)) {
        (true, Some(ends_at)) => Some((ends_at - now).max(0)),
        _ => None,
    };

    // a heartbeat of zero disables the safety net entirely, leaving only the
    // end-of-event wake — the cheapest a host can run
    let delay = match (until_end, heartbeat > 0) {
        (Some(until_end), true) => until_end.min(heartbeat),
        (Some(until_end), false) => until_end,
        (None, true) => heartbeat,
        (None, false) => return None,
    };

    // a floor stops a rundown of already-expired events spinning the wake
    Some(delay.max(50))
}

/// Applies a websocket command to the project.
///
/// Playback, the overlay, the aux timers and the run report all move through
/// here, so a host cannot implement three of the four and quietly lose the
/// fourth — which is how the report went missing from the native server.
///
/// Returns true when clients should be told and the project written down.
pub fn apply_socket_command(
    project: &mut Project,
    tag: &str,
    payload: Option<&Value>,
    now: i64,
) -> bool {
    match tag {
        "message" => return payload.map(|p| project.message.apply(p)).unwrap_or(false),
        "auxtimer" => {
            return payload
                .map(|p| project.aux_timers.apply(p, now))
                .unwrap_or(false)
        }
        _ => {}
    }

    let Some(command) = crate::runtime::Command::parse(tag, payload) else {
        return false;
    };
    let Some(rundown) = project.current().cloned() else {
        return false;
    };

    let was_playing = project.runtime.playback == crate::runtime::Playback::Play;
    let previous_entry = project.runtime.selected_id.clone();
    let tz = project.timezone_offset;

    if !project.runtime.apply(command, &rundown, now, tz) {
        return false;
    }

    record_report(project, was_playing, previous_entry, now);
    true
}

/// Notes when an event actually started or stopped.
///
/// The report is what lets an operator compare the run against the plan
/// afterwards, so it records wall-clock time of day rather than epoch.
fn record_report(
    project: &mut Project,
    was_playing: bool,
    previous_entry: Option<String>,
    now: i64,
) {
    let clock = (now + project.timezone_offset).rem_euclid(crate::rundown::MS_PER_DAY);
    let is_playing = project.runtime.playback == crate::runtime::Playback::Play;
    let current_entry = project.runtime.selected_id.clone();

    // whatever was running has stopped if playback ended or moved on
    if was_playing {
        if let Some(previous) = previous_entry.as_deref() {
            if !is_playing || current_entry.as_deref() != Some(previous) {
                project.report.on_stop(previous, clock);
            }
        }
    }

    // and a new one has begun if playback just started, or moved to another entry
    if is_playing {
        if let Some(current) = current_entry.as_deref() {
            if !was_playing || previous_entry.as_deref() != Some(current) {
                project.report.on_start(current, clock);
            }
        }
    }
}
