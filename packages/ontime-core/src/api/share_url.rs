//! Pre-authenticated share links.

use serde_json::Value;

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
/// Refuses, as upstream's validator does, a `baseUrl` that is not a full URL
/// and a missing `path`.
pub fn generate_share_url(body: &Value, room_hash: Option<&str>) -> Result<String, &'static str> {
    let base_url = body
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|base| is_full_url(base))
        .ok_or("baseUrl must be a full URL, eg: http://192.168.10.1:4001")?;
    let canonical_path = body
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or("path must be given")?;
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
    Ok(format!("{}/{}{}", origin_of(base_url), path, suffix))
}

/// `scheme://host…`, the scheme starting with a letter — what a browser's
/// `location.origin` always is.
fn is_full_url(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    let scheme_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    scheme_ok && !host.is_empty()
}
