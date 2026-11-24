//! Plan 008, T0: the script and settings contract. The fixture was produced by
//! running upstream's buildScriptEvents, parseTeleprompterSettings,
//! isReadableScriptSource and renameScriptSource. The route validation is
//! not runnable on its own upstream, so its rows are written out here from
//! upstream's teleprompter.integration.test.ts.

use ontime_core::rundown::Rundown;
use ontime_core::teleprompter::script::{build_script_events, revision_of, ScriptEvent};
use ontime_core::teleprompter::settings::{
    is_readable_script_source, parse_settings, rename_script_source, validate_patch, SettingsError,
    TeleprompterSettings,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Build {
    name: String,
    rundown: Rundown,
    custom_fields: Value,
    settings: TeleprompterSettings,
    events: Vec<ScriptEvent>,
}

#[derive(Deserialize)]
struct Parse {
    name: String,
    data: Value,
    fallback: TeleprompterSettings,
    result: TeleprompterSettings,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Readable {
    script: String,
    custom_fields: Value,
    readable: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rename {
    source: String,
    old_key: String,
    new_key: Option<String>,
    result: String,
}

#[derive(Deserialize)]
struct Cases {
    builds: Vec<Build>,
    parses: Vec<Parse>,
    readables: Vec<Readable>,
    renames: Vec<Rename>,
}

#[derive(Deserialize)]
struct Fixture {
    cases: Cases,
}

fn fixture() -> Cases {
    serde_json::from_str::<Fixture>(include_str!("fixtures/teleprompter_script.json")).unwrap().cases
}

#[test]
fn scripts_match_upstream() {
    let failures: Vec<String> = fixture()
        .builds
        .iter()
        .filter_map(|case| {
            let actual = build_script_events(&case.rundown, &case.custom_fields, &case.settings);
            (actual != case.events).then(|| format!("{}:\n  want {:?}\n  got  {actual:?}", case.name, case.events))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn settings_parse_like_upstream() {
    for case in fixture().parses {
        assert_eq!(parse_settings(&case.data, &case.fallback), case.result, "{}", case.name);
    }
}

#[test]
fn readable_sources_match_upstream() {
    for case in fixture().readables {
        assert_eq!(is_readable_script_source(&case.script, &case.custom_fields), case.readable, "{:?}", case.script);
    }
}

#[test]
fn renamed_fields_carry_the_source_like_upstream() {
    for case in fixture().renames {
        assert_eq!(rename_script_source(&case.source, &case.old_key, case.new_key.as_deref()), case.result);
    }
}

#[test]
fn the_route_refuses_what_upstream_refuses() {
    let fields = json!({ "script": { "type": "text" }, "poster": { "type": "image" } });
    use SettingsError::*;
    let refused = [
        (json!([]), NotAnObject),
        (json!(null), NotAnObject),
        (json!("note"), NotAnObject),
        (json!({ "script": 123 }), UnreadableScript),
        (json!({ "script": "" }), UnreadableScript),
        (json!({ "script": "custom-missing" }), UnreadableScript),
        (json!({ "script": "custom-poster" }), UnreadableScript),
        (json!({ "script": null }), UnreadableScript),
        (json!({ "heading": "banner" }), InvalidHeading),
        (json!({ "heading": ["title"] }), InvalidHeading),
        (json!({ "charsPerLine": 9 }), InvalidCharsPerLine),
        (json!({ "charsPerLine": 81 }), InvalidCharsPerLine),
        (json!({ "charsPerLine": 32.5 }), InvalidCharsPerLine),
        (json!({ "charsPerLine": "32" }), InvalidCharsPerLine),
        (json!({ "charsPerLine": null }), InvalidCharsPerLine),
    ];
    for (body, error) in refused {
        assert_eq!(validate_patch(&body, &fields), Err(error), "{body}");
    }
    for body in [
        json!({}),
        json!({ "script": "title" }),
        json!({ "script": "custom-script", "charsPerLine": 10, "heading": "none" }),
        json!({ "charsPerLine": 80 }),
        json!({ "charsPerLine": 32.0 }),
    ] {
        assert_eq!(validate_patch(&body, &fields), Ok(()), "{body}");
    }
}

#[test]
fn the_revision_follows_the_lines() {
    let case = fixture().builds.into_iter().find(|case| case.name == "long notes wrap").unwrap();
    let events = build_script_events(&case.rundown, &case.custom_fields, &case.settings);
    let revision = revision_of(&events, case.settings.chars_per_line);
    assert_eq!(revision, revision_of(&events.clone(), case.settings.chars_per_line), "stable");
    assert!(revision < 1 << 48, "fits a JavaScript number exactly");
    assert_ne!(revision, revision_of(&events, case.settings.chars_per_line + 1), "the line length counts");
    assert_ne!(revision, revision_of(&events[..0], case.settings.chars_per_line), "the lines count");
}
