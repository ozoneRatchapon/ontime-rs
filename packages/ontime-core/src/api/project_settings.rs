//! Project data, settings, view settings and URL presets.

use serde_json::{json, Value};

use crate::project::Project;

use super::*;

// #region project settings ===================================

pub(crate) fn project_data(
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
            project.project = match merged(&project.project, &patch) {
                Ok(data) => data,
                Err(refusal) => return refusal,
            };
            effects.push(Effect::refetch("project-data", None));
            ApiResponse::json(&project.project)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

pub(crate) fn settings(
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
            if patch
                .get("auxTimerNames")
                .is_some_and(|names| !names.is_array())
            {
                return ApiResponse::error("auxTimerNames must be an array", 400);
            }
            // checked before anything is applied: a bad patch must not reset
            // the settings it does not mention, nor half-apply the room fields
            let mut settings: crate::project::Settings = match merged(&project.settings, &patch) {
                Ok(settings) => settings,
                Err(refusal) => return refusal,
            };
            // the app version is the server's to say, not the client's
            settings.version.clone_from(&project.settings.version);
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
            project.settings = settings;
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

pub(crate) fn view_settings(
    project: &mut Project,
    req: &ApiRequest,
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match req.method {
        HttpMethod::Get => ApiResponse::json(&project.view_settings),
        HttpMethod::Post => {
            let patch = req.body.clone();
            project.view_settings = match merged(&project.view_settings, &patch) {
                Ok(settings) => settings,
                Err(refusal) => return refusal,
            };
            effects.push(Effect::refetch("view-settings", None));
            ApiResponse::json(&project.view_settings)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

pub(crate) fn url_presets(
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
