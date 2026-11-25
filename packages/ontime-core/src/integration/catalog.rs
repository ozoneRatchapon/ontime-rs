//! Every call the integration API takes, as data (plan 013). The OpenAPI
//! document (`.docs/integration_api.openapi.json`) is generated from this, and
//! `tests/integration_catalog_tests.rs` keeps the two honest: every example
//! here must parse, every action upstream has must be here, and the committed
//! document must be what this generates.

use serde_json::{json, Map, Value};

use super::limit::is_read;

/// One call, as `GET /r/{room}/api<path>`.
pub struct Call {
    /// The OpenAPI path, with `{parameters}`.
    pub path: &'static str,
    /// A concrete request, which must parse.
    pub example: &'static str,
    pub summary: &'static str,
}

pub const CALLS: &[Call] = &[
    Call { path: "/version", example: "/version", summary: "The Ontime version this server speaks" },
    Call { path: "/poll", example: "/poll", summary: "Everything the screens are sent: playback, timers, the overlay and the teleprompter" },
    Call { path: "/start", example: "/start", summary: "Start or resume the loaded event" },
    Call { path: "/start/next", example: "/start/next", summary: "Load the next event and start it" },
    Call { path: "/start/previous", example: "/start/previous", summary: "Load the previous event and start it" },
    Call { path: "/start/index/{index}", example: "/start/index/3", summary: "Start the event at this position, counting from 1, skipped events included" },
    Call { path: "/start/id/{id}", example: "/start/id/abc123", summary: "Start the event with this id" },
    Call { path: "/start/cue/{cue}", example: "/start/cue/4A", summary: "Start the next event with this cue" },
    Call { path: "/load/next", example: "/load/next", summary: "Arm the next event, wrapping to the first after the last" },
    Call { path: "/load/previous", example: "/load/previous", summary: "Arm the previous event; at the first, the first again" },
    Call { path: "/load/index/{index}", example: "/load/index/2", summary: "Arm the event at this position, counting from 1" },
    Call { path: "/load/id/{id}", example: "/load/id/abc123", summary: "Arm the event with this id" },
    Call { path: "/load/cue/{cue}", example: "/load/cue/4A", summary: "Arm the next event with this cue" },
    Call { path: "/pause", example: "/pause", summary: "Pause" },
    Call { path: "/stop", example: "/stop", summary: "Stop and unload" },
    Call { path: "/reload", example: "/reload", summary: "Load the loaded event again, armed" },
    Call { path: "/roll", example: "/roll", summary: "Follow the wall clock: play whatever is due now" },
    Call { path: "/addtime/{ms}", example: "/addtime/60000", summary: "Add milliseconds to the running event (negative removes), at most an hour" },
    Call { path: "/addtime/add/{ms}", example: "/addtime/add/60000", summary: "Add milliseconds, at most an hour" },
    Call { path: "/addtime/remove/{ms}", example: "/addtime/remove/30000", summary: "Remove milliseconds, at most an hour" },
    Call { path: "/offsetmode/{mode}", example: "/offsetmode/relative", summary: "Measure the schedule as planned (absolute) or from the actual start (relative)" },
    Call { path: "/message/timer/text/{text}", example: "/message/timer/text/Wrap%20up", summary: "The timer overlay's text, percent-decoded" },
    Call { path: "/message/timer/visible/{flag}", example: "/message/timer/visible/true", summary: "Show or hide the timer overlay" },
    Call { path: "/message/timer/blink/{flag}", example: "/message/timer/blink/1", summary: "Blink the timer overlay" },
    Call { path: "/message/timer/blackout/{flag}", example: "/message/timer/blackout/false", summary: "Black out the timer screens" },
    Call { path: "/message/timer/secondarySource/{source}", example: "/message/timer/secondarySource/aux1", summary: "What the timer's second line shows: aux1, aux2, aux3 or secondary" },
    Call { path: "/message/secondary/{text}", example: "/message/secondary/Doors%20open", summary: "The secondary message, percent-decoded" },
    Call { path: "/auxtimer/{timer}/{command}", example: "/auxtimer/1/start", summary: "Start, pause or stop aux timer 1, 2 or 3" },
    Call { path: "/auxtimer/{timer}/duration/{ms}", example: "/auxtimer/2/duration/90000", summary: "Set an aux timer's duration" },
    Call { path: "/auxtimer/{timer}/addtime/{ms}", example: "/auxtimer/2/addtime/-5000", summary: "Add to an aux timer" },
    Call { path: "/auxtimer/{timer}/direction/{direction}", example: "/auxtimer/3/direction/count-up", summary: "Count an aux timer up or down" },
    Call { path: "/change/{id}/{field}/{value}", example: "/change/abc123/title/Opening", summary: "Edit one field of an entry. Path values stay percent-encoded; send text with spaces in the query (?title=My+Title)" },
    Call { path: "/client", example: "/client?target=abc&rename=Stage", summary: "Rename (rename=) or redirect (redirect=) a connected screen, by its client id (target=)" },
    Call { path: "/teleprompter/{command}", example: "/teleprompter/play", summary: "play, pause, toggle, next, previous or top" },
    Call { path: "/teleprompter/speed/{lpm}", example: "/teleprompter/speed/120", summary: "Reading speed in lines per minute; negative reads back up" },
    Call { path: "/teleprompter/speedBy/{lpm}", example: "/teleprompter/speedBy/-20", summary: "Change the reading speed by this much" },
    Call { path: "/teleprompter/scroll/{lines}", example: "/teleprompter/scroll/3", summary: "Nudge the reader by lines" },
    Call { path: "/teleprompter/goto/{by}/{value}", example: "/teleprompter/goto/cue/4A", summary: "Jump to an event by cue, id or index" },
    Call { path: "/teleprompter/goto/loaded", example: "/teleprompter/goto/loaded", summary: "Jump to the event Ontime has loaded" },
    Call { path: "/teleprompter/mode/{mode}", example: "/teleprompter/mode/script", summary: "Stop at each event (event) or read through (script), or toggle" },
    Call { path: "/teleprompter/blackout/{state}", example: "/teleprompter/blackout/toggle", summary: "Hide the following screens: on, off or toggle" },
];

/// The action a catalogue path belongs to: its first segment.
pub fn tag_of(path: &str) -> &str {
    path.trim_start_matches('/')
        .split(['/', '?'])
        .next()
        .unwrap_or_default()
}

fn parameters(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .map(|name| json!({ "name": name, "in": "path", "required": true, "schema": { "type": "string" } }))
        .collect()
}

/// The OpenAPI 3.1 document for the catalogue.
pub fn openapi() -> Value {
    let message = json!({
        "type": "object",
        "properties": { "message": { "type": "string" } },
        "required": ["message"],
    });
    let mut paths = Map::new();
    for call in CALLS {
        let mut parameters = vec![json!({ "$ref": "#/components/parameters/room" })];
        parameters.extend(self::parameters(call.path));
        let mut responses = Map::new();
        responses.insert(
            "202".into(),
            json!({ "description": "Done. `payload` is upstream's reply: \"success\", or the state asked for.",
                    "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Reply" } } } }),
        );
        for (code, why) in [
            ("400", "Refused, with upstream's message"),
            ("401", "The room needs its token"),
            ("404", "No such room; a call never creates one"),
            ("429", "Too many calls; wait for Retry-After seconds"),
        ] {
            responses.insert(code.into(), json!({ "description": why, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Message" } } } }));
        }
        if !is_read(tag_of(call.path)) {
            responses.insert("403".into(), json!({ "description": "A members-only room, driven only by a member or its password", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Message" } } } }));
        }
        let operation = json!({
            "summary": call.summary,
            "tags": [tag_of(call.path)],
            "parameters": parameters,
            "x-example": format!("/r/{{room}}/api{}", call.example),
            "responses": responses,
        });
        paths.insert(
            format!("/r/{{room}}/api{}", call.path),
            json!({ "get": operation }),
        );
    }
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Ontime integration API",
            "version": crate::project::ONTIME_VERSION,
            "description": "Drive an Ontime room from Companion, a Stream Deck or a script. Generated from ontime-core's integration catalogue; see .docs/003_integration_api.md.",
        },
        "security": [{ "bearer": [] }, { "query": [] }],
        "paths": paths,
        "components": {
            "parameters": {
                "room": { "name": "room", "in": "path", "required": true, "schema": { "type": "string" } },
            },
            "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer", "description": "The room's token. A cookie never counts." },
                "query": { "type": "apiKey", "in": "query", "name": "token", "description": "The same token, for senders that cannot set headers" },
            },
            "schemas": {
                "Reply": { "type": "object", "properties": { "payload": {} }, "required": ["payload"] },
                "Message": message,
            },
        },
    })
}
