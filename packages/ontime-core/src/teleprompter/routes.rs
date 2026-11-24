//! `/data/teleprompter/*`, as upstream serves it.
//!
//! - `GET  settings`          the room's teleprompter settings
//! - `POST settings`          a partial patch; 422 and no change when invalid
//! - `GET  script?source=`    the shared script, or one cut from another source
//!
//! Upstream also answers an unchanged script with 304 through Express's ETag.
//! Not here: the script carries its revision, which is what screens compare.

use super::room;
use super::settings::{parse_settings, validate_patch};
use crate::api::{ApiRequest, ApiResponse, Effect, HttpMethod};
use crate::project::Project;

pub(crate) fn handle(
    project: &mut Project,
    req: &ApiRequest,
    rest: &[&str],
    effects: &mut Vec<Effect>,
    now: i64,
) -> ApiResponse {
    match (req.method, rest) {
        (HttpMethod::Get, ["settings"]) => ApiResponse::json(&project.teleprompter),
        (HttpMethod::Post, ["settings"]) => {
            if let Err(error) = validate_patch(&req.body, &project.custom_fields) {
                return ApiResponse::error(error.message(), 422);
            }
            project.teleprompter = parse_settings(&req.body, &project.teleprompter);
            effects.push(Effect::refetch("teleprompter", None));
            ApiResponse::json(&project.teleprompter)
        }
        (HttpMethod::Get, ["script"]) => {
            let was_built = project.prompter.built;
            let script = room::script_for_source(project, query(&req.path, "source").as_deref(), now);
            // the first read starts following the loaded event, which is worth keeping
            if project.prompter.built != was_built {
                effects.push(Effect::Persist);
            }
            ApiResponse::json(&*script)
        }
        _ => ApiResponse::error("Not found", 404),
    }
}

/// One query parameter, percent-decoded, from a path with a query string.
fn query(path: &str, key: &str) -> Option<String> {
    let (_, query) = path.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = match bytes[i] {
            b'%' => bytes
                .get(i + 1..i + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok()),
            _ => None,
        };
        match (decoded, bytes[i]) {
            (Some(byte), _) => {
                out.push(byte);
                i += 3;
            }
            (None, b'+') => {
                out.push(b' ');
                i += 1;
            }
            (None, byte) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a change may have touched what the script is built from: the
/// rundown, custom fields, the teleprompter settings, or everything.
pub(crate) fn touches_script(effects: &[Effect]) -> bool {
    effects.iter().any(|effect| {
        matches!(effect, Effect::Refetch { target, .. }
            if matches!(target.as_str(), "rundown" | "custom-fields" | "teleprompter" | "all"))
    })
}
