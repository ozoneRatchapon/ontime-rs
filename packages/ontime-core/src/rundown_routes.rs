//! `/data/rundowns`: the project's rundowns and the entries in them.

use serde_json::{json, Value};

use crate::api::{string_list, ApiRequest, ApiResponse, Effect, HttpMethod};
use crate::cue::CueNumber;
use crate::custom_fields;
use crate::project::{Project, ReplaceError};
use crate::rundown::{entry_type, validate_title, AddError, Insert, Rundown};

pub(crate) fn handle(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    match (&req.method, rest) {
        // --- project rundowns ---
        (HttpMethod::Get, []) => ApiResponse::json(&project.rundown_list()),

        (HttpMethod::Post, []) => {
            let Some(title) = req.body.get("title").and_then(Value::as_str) else {
                return ApiResponse::error("No title provided", 422);
            };
            if let Some(refusal) = validate_title(title) {
                return ApiResponse::error(refusal, 422);
            }
            let title = title.to_string();
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
                return ApiResponse::error("No title provided", 422);
            };
            if let Some(refusal) = validate_title(title) {
                return ApiResponse::error(refusal, 422);
            }
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            rundown.title = title.to_string();
            effects.push(Effect::refetch("project-rundowns", None));
            ApiResponse::json(&project.rundown_list())
        }

        (HttpMethod::Put, [id]) => {
            let Some(entries) = req.body.get("entries").and_then(Value::as_array) else {
                return ApiResponse::error("entries must be a list of entries", 400);
            };
            let title = req.body.get("title").and_then(Value::as_str);
            if let Some(refusal) = title.and_then(validate_title) {
                return ApiResponse::error(refusal, 422);
            }
            match project.replace_rundown(id, title, entries) {
                Err(ReplaceError::NotFound) => ApiResponse::error("Rundown not found", 404),
                Err(ReplaceError::NotAnEntry) => {
                    ApiResponse::error("entries must be a list of entries", 400)
                }
                Ok(replaced) => {
                    if replaced.list_changed {
                        effects.push(Effect::refetch("project-rundowns", None));
                    }
                    if replaced.stopped {
                        effects.push(Effect::Reschedule);
                    }
                    respond_with_rundown(project, id, effects)
                }
            }
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
            match rundown.check_add(&payload) {
                Ok(()) => {}
                Err(AddError::Exists(entry_id)) => {
                    return ApiResponse::error(
                        format!("Entry with ID {entry_id} already exists"),
                        409,
                    )
                }
                Err(AddError::InvalidParent(parent)) => {
                    return ApiResponse::error(
                        format!("Invalid parent event with ID {parent}"),
                        422,
                    )
                }
                Err(AddError::NestedGroup) => {
                    return ApiResponse::error("Cannot add a group to another group", 422)
                }
            }
            let entry = rundown.add_entry(&payload, at);
            let entry_id = entry["id"].as_str().unwrap_or_default().to_string();
            let entry = drop_unknown_custom_values(project, id, &[entry_id]).unwrap_or(entry);
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
            // an entry cannot change its type
            let current_type = rundown.get(&entry_id).map(entry_type);
            if let (Some(current), Some(asked)) =
                (current_type, payload.get("type").and_then(Value::as_str))
            {
                if current != asked {
                    return ApiResponse::error("Invalid event type", 422);
                }
            }
            let Some(mut entry) = rundown.edit_entry(&entry_id, &payload) else {
                return ApiResponse::error("Entry not found", 404);
            };
            if payload.get("custom").is_some() {
                entry = drop_unknown_custom_values(project, id, &[entry_id]).unwrap_or(entry);
            }
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
            if data.get("custom").is_some() {
                drop_unknown_custom_values(project, id, &ids);
            }
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
            if order != "insert" && !rundown.entries.contains_key(&destination) {
                return ApiResponse::error("Event not found", 404);
            }
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
            // RenumberCues: { ids, prefix, start, increment }, numbers as decimal strings
            let body = &req.body;
            let ids = string_list(body.get("ids"));
            let prefix = body.get("prefix").and_then(Value::as_str);
            let number = |key: &str| body.get(key).and_then(Value::as_str).map(CueNumber::parse);
            let (Some(prefix), Some(start), Some(increment)) =
                (prefix, number("start"), number("increment"))
            else {
                return ApiResponse::error("Renumber needs ids, prefix, start and increment", 422);
            };
            if ids.is_empty() {
                return ApiResponse::error("Renumber needs ids, prefix, start and increment", 422);
            }
            let (Some(start), Some(increment)) = (start, increment) else {
                return ApiResponse::error("input can not be converted to a number", 422);
            };
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            if rundown.renumber(&ids, prefix, start, increment).is_err() {
                return ApiResponse::error("A given id was not an event", 422);
            }
            respond_with_rundown(project, id, effects)
        }

        (HttpMethod::Patch, [id, "applydelay", delay_id]) => {
            let Some(rundown) = project.get_mut(id) else {
                return ApiResponse::error("Rundown not found", 404);
            };
            match rundown.get(delay_id).map(entry_type) {
                None => return ApiResponse::error("Delay not found", 404),
                Some("delay") => {}
                Some(_) => return ApiResponse::error("Given delay ID not found", 422),
            }
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
            match rundown.get(group_id).map(entry_type) {
                None => {
                    return ApiResponse::error(
                        format!("Group with ID {group_id} not found or is not a group"),
                        404,
                    )
                }
                Some("group") => {}
                Some(_) => {
                    return ApiResponse::error(
                        format!("Group with ID {group_id} not found or is not a group"),
                        422,
                    )
                }
            }
            if !rundown.ungroup(group_id) {
                return ApiResponse::error("Group not found", 404);
            }
            respond_with_rundown(project, id, effects)
        }

        _ => ApiResponse::error("Not implemented", 404),
    }
}

/// Applies the custom field rule to the entries a write touched, and returns the
/// last of them as stored now.
fn drop_unknown_custom_values(
    project: &mut Project,
    rundown_id: &str,
    ids: &[String],
) -> Option<Value> {
    let rundown = project.rundowns.get_mut(rundown_id)?;
    custom_fields::drop_unknown_values(&project.custom_fields, rundown, ids);
    rundown.get(ids.last()?).cloned()
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
