//! Custom fields: the project's list of them, and the values entries hold.
//!
//! The rule kept here, as upstream keeps it: an entry only holds values for
//! fields the project has. Renaming a field moves its values to the new key and
//! deleting one removes them, in every rundown, not only the loaded one.

use serde_json::Value;

use crate::api::{ApiRequest, ApiResponse, Effect, HttpMethod};
use crate::project::Project;
use crate::rundown::Rundown;

/// Spaces become underscores, as the client's `customFieldLabelToKey` does.
pub fn key_for(label: &str) -> String {
    label.trim().replace(' ', "_")
}

/// The original accepts only letters, numbers and spaces in a label.
pub fn is_valid_label(label: &str) -> bool {
    let trimmed = label.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ')
}

/// Drops the values of `ids` for fields not in `fields`.
pub fn drop_unknown_values(fields: &Value, rundown: &mut Rundown, ids: &[String]) {
    for id in ids {
        let Some(values) = rundown
            .entries
            .get_mut(id)
            .and_then(|entry| entry.get_mut("custom"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        values.retain(|key, _| fields.get(key).is_some());
    }
}

/// Applies `change` to every entry's custom values in every rundown. Returns
/// the ids of the rundowns where it changed something.
fn for_each_value_map(
    project: &mut Project,
    change: impl Fn(&mut serde_json::Map<String, Value>) -> bool,
) -> Vec<String> {
    let mut changed = Vec::new();
    for rundown in project.rundowns.values_mut() {
        let mut touched = false;
        for entry in rundown.entries.values_mut() {
            if let Some(values) = entry.get_mut("custom").and_then(Value::as_object_mut) {
                touched |= change(values);
            }
        }
        if touched {
            rundown.revision += 1;
            changed.push(rundown.id.clone());
        }
    }
    changed
}

fn rename_values(project: &mut Project, old: &str, new: &str) -> Vec<String> {
    for_each_value_map(project, |values| match values.remove(old) {
        Some(value) => {
            values.insert(new.to_string(), value);
            true
        }
        None => false,
    })
}

fn remove_values(project: &mut Project, key: &str) -> Vec<String> {
    for_each_value_map(project, |values| values.remove(key).is_some())
}

fn fields_response(
    project: &Project,
    changed: Vec<String>,
    effects: &mut Vec<Effect>,
) -> ApiResponse {
    effects.push(Effect::refetch("custom-fields", None));
    for rundown_id in changed {
        effects.push(Effect::refetch("rundown", Some(rundown_id)));
    }
    ApiResponse::json(&project.custom_fields)
}

/// `/data/custom-fields`.
pub(crate) fn handle(
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
            if !is_valid_label(label) {
                // the original validates this, so accepting more here would let
                // us create fields the real client cannot handle
                return ApiResponse::error(
                    "A custom field label may only contain letters, numbers and spaces",
                    400,
                );
            }
            let key = key_for(label);
            if let Some(fields) = project.custom_fields.as_object_mut() {
                fields.insert(key, field);
            }
            fields_response(project, Vec::new(), effects)
        }
        (HttpMethod::Put, [key]) => edit(project, key, &req.body, effects),
        (HttpMethod::Delete, [key]) => {
            let existed = project
                .custom_fields
                .as_object_mut()
                .and_then(|fields| fields.remove(*key))
                .is_some();
            let changed = if existed {
                remove_values(project, key)
            } else {
                Vec::new()
            };
            fields_response(project, changed, effects)
        }
        _ => ApiResponse::error("Not implemented", 404),
    }
}

fn edit(project: &mut Project, key: &str, patch: &Value, effects: &mut Vec<Effect>) -> ApiResponse {
    let Some(existing) = project.custom_fields.get(key).cloned() else {
        return ApiResponse::error("Custom field not found", 404);
    };
    let Some(patch) = patch.as_object() else {
        return ApiResponse::error("A custom field edit must be an object", 400);
    };
    if let Some(kind) = patch.get("type") {
        if Some(kind) != existing.get("type") {
            return ApiResponse::error("A custom field cannot change type", 400);
        }
    }
    let new_key = match patch.get("label") {
        None => key.to_string(),
        Some(Value::String(label)) if is_valid_label(label) => key_for(label),
        Some(_) => {
            return ApiResponse::error(
                "A custom field label may only contain letters, numbers and spaces",
                400,
            )
        }
    };
    if new_key != key && project.custom_fields.get(&new_key).is_some() {
        return ApiResponse::error("A custom field with that label already exists", 400);
    }

    let mut field = existing;
    if let Some(field) = field.as_object_mut() {
        field.extend(patch.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let Some(fields) = project.custom_fields.as_object_mut() else {
        return ApiResponse::error("Custom fields are unreadable", 500);
    };
    // an unchanged key keeps its place in the list; a renamed one goes last,
    // as upstream's assign-then-delete does
    if new_key != key {
        fields.remove(key);
    }
    fields.insert(new_key.clone(), field);

    let changed = if new_key == key {
        Vec::new()
    } else {
        rename_values(project, key, &new_key)
    };
    fields_response(project, changed, effects)
}
