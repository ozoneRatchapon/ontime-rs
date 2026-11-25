//! Custom views on the Worker (plan 021): a room's own single-file HTML pages,
//! for an OBS browser source or a screen dressed for the event.
//!
//! What upstream's `customViews.service.ts` checks is ported here: the name
//! (`isValidCustomViewSlug`) and the file (`validateHtmlContent`). The host
//! stores the pages apart from the project and serves them with [`serve`]:
//! sandboxed, and with a small script first that points what the page opens at
//! this room, so a page written for the Ontime app works here unchanged.

/// Upstream allows 4 MB on disk. A Durable Object keeps a value up to 2 MB,
/// and a page here is one value.
pub const MAX_BYTES: usize = 1_000_000;
/// Pages a room may keep. Each is read whole when shown.
pub const MAX_VIEWS: usize = 20;
/// upstream `demoViewSlug`.
pub const DEMO_SLUG: &str = "demo";
/// upstream's demo page (`bundle/bundledDemoHtml.ts`), as the app restores it.
pub const DEMO_HTML: &str = include_str!("custom_view_demo.html");

/// Why a page or a name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewError {
    BadName,
    Empty,
    TooLarge,
    NotHtml,
    ExternalScript,
    ExternalStylesheet,
    Iframe,
    TooMany,
}

impl ViewError {
    /// upstream's messages where it has one.
    pub fn message(self) -> String {
        match self {
            Self::BadName => "Use lowercase letters, numbers, and dashes only.".into(),
            Self::Empty => "Uploaded file is empty".into(),
            Self::TooLarge => format!("File size limit ({}MB) exceeded", MAX_BYTES / 1_000_000),
            Self::NotHtml => "File does not appear to be valid HTML. Expected <!DOCTYPE html> or <html> at the start.".into(),
            Self::ExternalScript => "External scripts are not allowed. Use inline <script> instead.".into(),
            Self::ExternalStylesheet => "External stylesheets are not allowed. Use inline <style> instead.".into(),
            Self::Iframe => "Iframes are not allowed.".into(),
            Self::TooMany => format!("A room keeps at most {MAX_VIEWS} custom views"),
        }
    }

    pub fn status(self) -> u16 {
        match self {
            Self::TooLarge => 413,
            _ => 400,
        }
    }
}

/// upstream `isValidCustomViewSlug`: 1 to 63 of `a-z`, `0-9` and `-`, not
/// starting or ending with a dash.
pub fn is_valid_slug(slug: &str) -> bool {
    (1..=63).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
}

/// upstream `validateCustomViewUpload` and `validateHtmlContent`.
pub fn validate(html: &str) -> Result<(), ViewError> {
    if html.is_empty() {
        return Err(ViewError::Empty);
    }
    if html.len() > MAX_BYTES {
        return Err(ViewError::TooLarge);
    }
    let start = html.trim_start().to_ascii_lowercase();
    let opens_html = start
        .strip_prefix("<html")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_whitespace() || c == '>');
    if !doctype_is_html(&start) && !opens_html {
        return Err(ViewError::NotHtml);
    }
    let lower = html.to_ascii_lowercase();
    if tags(&lower, "<script").any(|tag| has_attribute(tag, "src")) {
        return Err(ViewError::ExternalScript);
    }
    if tags(&lower, "<link").any(is_stylesheet_link) {
        return Err(ViewError::ExternalStylesheet);
    }
    if tags(&lower, "<iframe").next().is_some() {
        return Err(ViewError::Iframe);
    }
    Ok(())
}

/// `/^\s*<!doctype\s+html[\s>]/i`, given the trimmed, lowercased start.
fn doctype_is_html(start: &str) -> bool {
    let Some(rest) = start.strip_prefix("<!doctype") else {
        return false;
    };
    let after_space = rest.trim_start();
    after_space.len() < rest.len()
        && after_space
            .strip_prefix("html")
            .and_then(|r| r.chars().next())
            .is_some_and(|c| c.is_whitespace() || c == '>')
}

/// The text of each `<name …>` tag up to its `>`, for a tag name followed by
/// whitespace or `>` (upstream's `<iframe[\s>]`), or anything for script and
/// link, whose patterns need at least one more character (`[^>]+`).
fn tags<'a>(lower: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    lower.match_indices(name).filter_map(move |(at, _)| {
        let rest = &lower[at + name.len()..];
        let end = rest.find('>').unwrap_or(rest.len());
        let inside = &rest[..end];
        match name {
            "<iframe" => rest
                .chars()
                .next()
                .filter(|c| c.is_whitespace() || *c == '>')
                .map(|_| inside),
            _ => Some(inside),
        }
    })
}

/// upstream `<script[^>]+src\s*=`: `src` then optional spaces then `=`,
/// anywhere before the tag closes and after at least one character.
fn has_attribute(inside: &str, name: &str) -> bool {
    inside
        .match_indices(name)
        .any(|(at, _)| at >= 1 && inside[at + name.len()..].trim_start().starts_with('='))
}

/// upstream `<link[^>]+rel\s*=\s*["']?stylesheet["']?`.
fn is_stylesheet_link(inside: &str) -> bool {
    inside.match_indices("rel").any(|(at, _)| {
        if at < 1 {
            return false;
        }
        let Some(value) = inside[at + 3..].trim_start().strip_prefix('=') else {
            return false;
        };
        let value = value.trim_start();
        let value = value.strip_prefix(['"', '\'']).unwrap_or(value);
        value.starts_with("stylesheet")
    })
}

/// The `Content-Security-Policy` a page is served with. `sandbox` gives it an
/// origin of its own, so it reads nothing of the Worker's and nothing of the
/// editor's; it may run its inline script and style, show images and fonts
/// from anywhere, and talk only to the Worker it came from.
pub fn content_security_policy(origin: &str) -> String {
    let socket = origin
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!(
        "sandbox allow-scripts allow-popups allow-forms; default-src 'none'; \
         script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
         img-src * data: blob:; font-src * data:; media-src * data: blob:; \
         connect-src {origin} {socket}"
    )
}

/// The page as it is served: the script that points it at this room first,
/// then the page as uploaded.
pub fn serve(html: &str) -> String {
    let shim = format!("<script>{ROOM_SHIM}</script>");
    // before anything of the page's own can open a socket
    let lower = html.to_ascii_lowercase();
    let head = lower.match_indices("<head").map(|(at, _)| at).find(|at| {
        lower[at + 5..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>')
    });
    match head {
        Some(at) => match html[at..].find('>') {
            Some(end) => {
                let split = at + end + 1;
                format!("{}{shim}{}", &html[..split], &html[split..])
            }
            None => format!("{shim}{html}"),
        },
        None => format!("{shim}{html}"),
    }
}

/// A page written for the Ontime app connects to `ws://<host>/ws` and asks
/// `/data/…` and `/api/…` of its host. Here a page lives at
/// `/r/<room>/external/<name>/`, so these are pointed at its room, carrying
/// the token the page was opened with.
const ROOM_SHIM: &str = r#"(function () {
  var match = location.pathname.match(/^\/r\/([^/]+)\//);
  if (!match) return;
  var room = decodeURIComponent(match[1]);
  var token = new URLSearchParams(location.search).get('token');
  function point(input, socket) {
    var url;
    try { url = new URL(String(input), location.href); } catch (e) { return null; }
    if (url.host !== location.host) return null;
    if (socket) {
      if (!/\/ws$/.test(url.pathname)) return null;
      url.pathname = '/ws';
      url.searchParams.set('room', room);
    } else {
      if (!/^\/(data|api)(\/|$)/.test(url.pathname)) return null;
      url.pathname = '/r/' + encodeURIComponent(room) + url.pathname;
    }
    if (token && !url.searchParams.has('token')) url.searchParams.set('token', token);
    return url.toString();
  }
  var NativeSocket = window.WebSocket;
  function RoomSocket(url, protocols) {
    var pointed = point(url, true) || url;
    return protocols === undefined ? new NativeSocket(pointed) : new NativeSocket(pointed, protocols);
  }
  RoomSocket.prototype = NativeSocket.prototype;
  ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED'].forEach(function (key) { RoomSocket[key] = NativeSocket[key]; });
  window.WebSocket = RoomSocket;
  var nativeFetch = window.fetch;
  window.fetch = function (input, init) {
    if (typeof input === 'string' || input instanceof URL) {
      return nativeFetch.call(window, point(input, false) || input, init);
    }
    var pointed = input && point(input.url, false);
    return nativeFetch.call(window, pointed ? new Request(pointed, input) : input, init);
  };
  var open = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url) {
    var args = Array.prototype.slice.call(arguments);
    args[1] = point(url, false) || url;
    return open.apply(this, args);
  };
})();"#;
