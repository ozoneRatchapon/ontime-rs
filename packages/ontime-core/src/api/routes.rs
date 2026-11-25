//! The routing of every `/data/*` request, and the helpers it shares.

use serde_json::{json, Value};

use crate::project::{fresh_room_token, Project};
use crate::teleprompter::{room as prompter, routes as prompter_routes};
use crate::{custom_fields, project_file, rundown_routes};

use super::*;

pub(crate) fn string_list(value: Option<&Value>) -> Vec<String> {
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

/// `current` with `patch`'s top-level keys laid over it. A patch whose values do
/// not fit is refused with a 400 rather than resetting the whole document to
/// its defaults, which is what reading it back leniently used to do.
pub(crate) fn merged<T>(current: &T, patch: &Value) -> Result<T, ApiResponse>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let mut value = serde_json::to_value(current)
        .map_err(|e| ApiResponse::error(format!("Unreadable stored data: {e}"), 500))?;
    shallow_merge(&mut value, patch);
    serde_json::from_value(value)
        .map_err(|e| ApiResponse::error(format!("Invalid input: {e}"), 400))
}

/// The path's segments from the API root, without a query string, a trailing
/// slash or any host prefix before `/data/` or `/user/`.
pub(crate) fn route_segments(raw: &str) -> Vec<&str> {
    let path = raw.split('?').next().unwrap_or(raw).trim_end_matches('/');
    // a caller may hand us the whole original URL path, so trim anything before
    // the API root
    let path = ["/data/", "/user/"]
        .iter()
        .find_map(|prefix| path.find(prefix).map(|i| &path[i..]))
        .unwrap_or(path);
    path.trim_start_matches('/').split('/').collect()
}

/// Puts a stored project back in place of the room's, from a backup file or the
/// room's own undo history. The room keeps its password, its activity log and
/// its shared teleprompter playback; playback starts stopped rather than
/// resuming from whenever the copy was taken.
pub fn restore_into(project: &mut Project, mut restored: Project, now: i64) {
    restored.password_hash = project.password_hash.clone();
    // the log belongs to this room, not to whatever the backup came from
    restored.audit = project.audit.clone();
    // shared playback never carries into another project
    restored.prompter = project.prompter.clone();
    restored.runtime = Default::default();
    *project = restored;
    project.ensure_consistent();
    prompter::pause_for_new_project(project, now);
}

pub fn handle(project: &mut Project, req: &ApiRequest, now: i64) -> ApiResponse {
    let mut effects: Vec<Effect> = Vec::new();
    let segments = route_segments(&req.path);

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

    // the teleprompter keeps its reader in place through edits, which needs the
    // script from before the change; only built once anything prompts
    let script_before = match req.method {
        HttpMethod::Get => None,
        _ => prompter::before_change(project),
    };

    let mut response = match segments.as_slice() {
        ["data", "teleprompter", rest @ ..] => {
            prompter_routes::handle(project, req, rest, &mut effects, now)
        }
        ["data", "rundowns", rest @ ..] => {
            let response = rundown_routes::handle(project, req, rest, &mut effects);
            if req.method != HttpMethod::Get && project.reconcile_runtime() {
                effects.push(Effect::Reschedule);
            }
            response
        }
        ["data", "project", rest @ ..] => project_data(project, req, rest, &mut effects),
        ["data", "settings", rest @ ..] => settings(project, req, rest, &mut effects),
        ["data", "view-settings"] => view_settings(project, req, &mut effects),
        ["data", "custom-fields", rest @ ..] => {
            custom_fields::handle(project, req, rest, &mut effects)
        }
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
                return ApiResponse::error(
                    "Only an owner of this room can change who may edit it",
                    403,
                );
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
        // locks the room with a new random token, or reopens it. `{ lock: true }`
        // from our client; `{ password }` from upstream's still locks, but the
        // password is not kept or used: nothing ever asks for it again, and a
        // token made from it could be guessed (plan 019)
        ["data", "session", "password"] => {
            let body = req.body.clone();
            let lock = body.get("lock").and_then(Value::as_bool) == Some(true)
                || body
                    .get("password")
                    .and_then(Value::as_str)
                    .is_some_and(|password| !password.is_empty());
            match lock {
                true => {
                    let token = fresh_room_token();
                    project.password_hash = Some(token.clone());
                    effects.push(Effect::Persist);
                    // every link made with the old token stops working
                    ApiResponse::json(&json!({ "protected": true, "token": token }))
                }
                // an explicit null clears it, reopening the room
                false if body.get("password").map(Value::is_null).unwrap_or(false) => {
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
                Ok(url) => ApiResponse::json(&json!({ "url": url })),
                Err(message) => ApiResponse::error(message, 422),
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
                object.remove("audit");
            }
            ApiResponse::json(&snapshot)
        }

        // restore a room from a downloaded backup. The room keeps its current
        // password (the backup never carried one), and playback starts stopped
        // rather than resuming from whenever the snapshot was taken.
        ["data", "db", "restore"] if req.method == HttpMethod::Post => {
            match serde_json::from_value::<Project>(req.body.clone()) {
                Ok(restored) => {
                    restore_into(project, restored, now);
                    effects.push(Effect::Persist);
                    effects.push(Effect::refetch("all", None));
                    effects.push(Effect::Reschedule);
                    ApiResponse::json(&json!({ "restored": true }))
                }
                Err(error) => ApiResponse::error(format!("Invalid backup file: {error}"), 400),
            }
        }

        // the room as a project file the Ontime app opens (plan 020)
        ["data", "db", "export"] if req.method == HttpMethod::Post => {
            ApiResponse::json(&project_file::export(project))
        }

        // a project file from the Ontime app, sent as its text, in place of the
        // room's show; the undo history keeps a copy of what it replaced
        ["data", "db", "import"] if req.method == HttpMethod::Post => {
            let file = req.body.get("file").and_then(Value::as_str);
            match file.map(project_file::import) {
                None => ApiResponse::error("Send the project file as text in \"file\"", 400),
                Some(Ok(imported)) => {
                    project_file::import_into(project, imported.project, now);
                    effects.push(Effect::Persist);
                    effects.push(Effect::refetch("all", None));
                    effects.push(Effect::Reschedule);
                    ApiResponse::json(&json!({
                        "imported": true,
                        "warnings": imported.warnings,
                    }))
                }
                Some(Err(error)) => ApiResponse::error(error.message(), 400),
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

        _ => {
            let method = req.method;
            let path = segments.join("/");
            ApiResponse::error(format!("Not implemented: {method:?} /{path}"), 404)
        }
    };

    if let Some(previous) =
        script_before.filter(|_| response.status < 400 && prompter_routes::touches_script(&effects))
    {
        if prompter::after_change(project, &previous, now) {
            effects.push(Effect::Persist);
        }
    }

    response.effects.append(&mut effects);
    response
}
