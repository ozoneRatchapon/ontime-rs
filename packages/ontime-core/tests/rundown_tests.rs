use ontime_core::rundown::{Insert, Rundown};
use serde_json::{json, Value};

fn empty() -> Rundown {
    Rundown::new("default", "Test")
}

fn add(rundown: &mut Rundown, title: &str, duration: i64) -> String {
    let entry = rundown.add_entry(
        &json!({ "title": title, "duration": duration }),
        Insert::End,
    );
    entry["id"].as_str().unwrap().to_string()
}

fn field<'a>(rundown: &'a Rundown, id: &str, key: &str) -> &'a Value {
    &rundown.get(id).unwrap()[key]
}

#[test]
fn new_entry_is_populated_with_client_defaults() {
    let mut rundown = empty();
    let entry = rundown.add_entry(&json!({ "title": "Intro" }), Insert::End);

    assert_eq!(entry["type"], "event");
    assert_eq!(entry["title"], "Intro");
    // the mock used to emit invalid enum values, which broke the editor
    assert_eq!(entry["timerType"], "count-down");
    assert_eq!(entry["timeStrategy"], "lock-duration");
    assert_eq!(entry["note"], "");
    assert!(entry["custom"].is_object());
    assert!(entry["triggers"].is_array());
}

#[test]
fn entries_are_appended_in_order() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);

    assert_eq!(rundown.order, vec![a.clone(), b.clone()]);
    assert_eq!(rundown.flat_order, vec![a, b]);
}

#[test]
fn insert_options_place_the_entry() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let c = add(&mut rundown, "C", 1000);

    let b = rundown.add_entry(
        &json!({ "title": "B", "after": a.clone() }),
        Insert::After(Some(a.clone())),
    );
    let b = b["id"].as_str().unwrap().to_string();

    assert_eq!(rundown.order, vec![a, b, c]);
}

#[test]
fn editing_applies_a_partial_patch_and_bumps_revision() {
    let mut rundown = empty();
    let id = add(&mut rundown, "A", 1000);
    let before = field(&rundown, &id, "revision").as_u64().unwrap();

    rundown
        .edit_entry(&id, &json!({ "title": "Renamed" }))
        .unwrap();

    assert_eq!(field(&rundown, &id, "title"), "Renamed");
    // untouched fields survive the patch
    assert_eq!(field(&rundown, &id, "duration"), 1000);
    assert!(field(&rundown, &id, "revision").as_u64().unwrap() > before);
}

#[test]
fn a_patch_cannot_overwrite_calculated_fields() {
    let mut rundown = empty();
    let id = add(&mut rundown, "A", 1000);

    rundown
        .edit_entry(
            &id,
            &json!({ "gap": 99_999, "delay": 99_999, "dayOffset": 7 }),
        )
        .unwrap();

    assert_eq!(field(&rundown, &id, "gap"), 0);
    assert_eq!(field(&rundown, &id, "delay"), 0);
    assert_eq!(field(&rundown, &id, "dayOffset"), 0);
}

#[test]
fn deleting_removes_the_entry_and_its_order_slot() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);

    rundown.delete_entries(std::slice::from_ref(&a));

    assert!(rundown.get(&a).is_none());
    assert_eq!(rundown.order, vec![b]);
}

#[test]
fn linked_events_start_when_the_previous_one_ends() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 60_000);
    let b = add(&mut rundown, "B", 30_000);

    rundown
        .edit_entry(&a, &json!({ "timeStart": 3_600_000 }))
        .unwrap();
    rundown
        .edit_entry(&b, &json!({ "linkStart": true }))
        .unwrap();

    assert_eq!(field(&rundown, &a, "timeEnd"), 3_660_000);
    assert_eq!(field(&rundown, &b, "timeStart"), 3_660_000);
    assert_eq!(field(&rundown, &b, "timeEnd"), 3_690_000);
    assert_eq!(field(&rundown, &b, "gap"), 0);
}

#[test]
fn an_unlinked_event_reports_the_gap_before_it() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 60_000);
    let b = add(&mut rundown, "B", 30_000);

    rundown.edit_entry(&a, &json!({ "timeStart": 0 })).unwrap();
    rundown
        .edit_entry(&b, &json!({ "timeStart": 90_000 }))
        .unwrap();

    assert_eq!(field(&rundown, &b, "gap"), 30_000);
}

#[test]
fn lock_end_derives_duration_from_the_end_time() {
    let mut rundown = empty();
    let id = add(&mut rundown, "A", 1000);

    rundown
        .edit_entry(
            &id,
            &json!({ "timeStrategy": "lock-end", "timeStart": 10_000, "timeEnd": 70_000 }),
        )
        .unwrap();

    assert_eq!(field(&rundown, &id, "duration"), 60_000);
}

#[test]
fn a_delay_entry_is_reported_on_the_events_after_it() {
    let mut rundown = empty();
    let _a = add(&mut rundown, "A", 60_000);
    rundown.add_entry(&json!({ "type": "delay", "duration": 5_000 }), Insert::End);
    let b = add(&mut rundown, "B", 60_000);

    assert_eq!(field(&rundown, &b, "delay"), 5_000);
}

#[test]
fn applying_a_delay_bakes_it_in_and_removes_the_delay_entry() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 60_000);
    rundown.edit_entry(&a, &json!({ "timeStart": 0 })).unwrap();

    let delay = rundown.add_entry(&json!({ "type": "delay", "duration": 5_000 }), Insert::End);
    let delay_id = delay["id"].as_str().unwrap().to_string();

    let b = add(&mut rundown, "B", 60_000);
    rundown
        .edit_entry(&b, &json!({ "timeStart": 60_000 }))
        .unwrap();

    assert!(rundown.apply_delay(&delay_id));

    assert!(rundown.get(&delay_id).is_none());
    assert_eq!(field(&rundown, &b, "timeStart"), 65_000);
    assert_eq!(field(&rundown, &b, "delay"), 0);
    // the event before the delay is untouched
    assert_eq!(field(&rundown, &a, "timeStart"), 0);
}

#[test]
fn reorder_moves_an_entry_before_another() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);
    let c = add(&mut rundown, "C", 1000);

    assert!(rundown.reorder(&c, &a, "before"));

    assert_eq!(rundown.order, vec![c, a, b]);
}

#[test]
fn grouping_nests_entries_and_flat_order_expands_them() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);
    let c = add(&mut rundown, "C", 1000);

    let group = rundown.group_entries(&[a.clone(), b.clone()]).unwrap();
    let group_id = group["id"].as_str().unwrap().to_string();

    assert_eq!(rundown.order, vec![group_id.clone(), c.clone()]);
    assert_eq!(
        rundown.flat_order,
        vec![group_id.clone(), a.clone(), b.clone(), c]
    );
    assert_eq!(field(&rundown, &a, "parent"), group_id.as_str());

    // the group reports the span of its children
    assert_eq!(field(&rundown, &group_id, "duration"), 2000);
}

#[test]
fn ungrouping_promotes_children_back_in_place() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);
    let c = add(&mut rundown, "C", 1000);

    let group = rundown.group_entries(&[a.clone(), b.clone()]).unwrap();
    let group_id = group["id"].as_str().unwrap().to_string();

    assert!(rundown.ungroup(&group_id));

    assert!(rundown.get(&group_id).is_none());
    assert_eq!(rundown.order, vec![a.clone(), b, c]);
    assert!(field(&rundown, &a, "parent").is_null());
}

#[test]
fn deleting_a_group_takes_its_children_with_it() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);

    let group = rundown.group_entries(&[a.clone(), b.clone()]).unwrap();
    let group_id = group["id"].as_str().unwrap().to_string();

    rundown.delete_entries(std::slice::from_ref(&group_id));

    assert!(rundown.get(&group_id).is_none());
    assert!(rundown.get(&a).is_none());
    assert!(rundown.get(&b).is_none());
    assert!(rundown.order.is_empty());
}

#[test]
fn cloning_places_the_copy_after_the_original() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);

    let copy = rundown.clone_entry(&a, Insert::End).unwrap();
    let copy_id = copy["id"].as_str().unwrap().to_string();

    assert_ne!(copy_id, a);
    assert_eq!(copy["title"], "A");
    assert_eq!(rundown.order, vec![a, copy_id, b]);
}

#[test]
fn renumber_rewrites_cues_across_the_flat_order() {
    let mut rundown = empty();
    let a = add(&mut rundown, "A", 1000);
    let b = add(&mut rundown, "B", 1000);

    rundown.renumber(10.0, 5.0);

    assert_eq!(field(&rundown, &a, "cue"), "10");
    assert_eq!(field(&rundown, &b, "cue"), "15");
}

#[test]
fn delete_all_empties_the_rundown() {
    let mut rundown = empty();
    add(&mut rundown, "A", 1000);
    add(&mut rundown, "B", 1000);

    rundown.delete_all();

    assert!(rundown.entries.is_empty());
    assert!(rundown.order.is_empty());
    assert!(rundown.flat_order.is_empty());
}

#[test]
fn a_rundown_survives_a_serialisation_round_trip() {
    let mut rundown = Rundown::demo("default", "Demo");
    let id = add(&mut rundown, "Extra", 1000);

    let json = serde_json::to_string(&rundown).unwrap();
    let restored: Rundown = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.order, rundown.order);
    assert_eq!(restored.entries.len(), rundown.entries.len());
    assert_eq!(restored.get(&id).unwrap()["title"], "Extra");

    // the client validates these keys before rendering
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(value["id"].is_string());
    assert!(value["order"].is_array());
    assert!(value["flatOrder"].is_array());
    assert!(value["entries"].is_object());
    assert!(value["revision"].is_number());
}

// --- room password hashing -------------------------------------------------

#[test]
fn hashing_a_password_is_deterministic_and_distinct() {
    use ontime_core::project::hash_password;

    assert_eq!(hash_password("hunter2"), hash_password("hunter2"));
    assert_ne!(hash_password("hunter2"), hash_password("hunter3"));
    // the digest is what travels as the bearer token, so it must be url-safe
    assert!(hash_password("hunter2")
        .chars()
        .all(|c| c.is_ascii_hexdigit()));
    assert!(!hash_password("hunter2").contains("hunter2"));
}

#[test]
fn token_comparison_rejects_mismatches_of_any_length() {
    use ontime_core::project::{hash_password, token_matches};

    let token = hash_password("hunter2");
    assert!(token_matches(&token, &token));
    assert!(!token_matches(&token, &hash_password("hunter3")));
    assert!(!token_matches(&token, ""));
    assert!(!token_matches(&token, &token[..token.len() - 1]));
    assert!(!token_matches("", &token));
}
