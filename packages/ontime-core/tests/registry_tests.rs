use ontime_core::registry::{
    handle, is_valid_room, should_report, Registry, RoomReport, REGISTRY_NAME, REPORT_INTERVAL_MS,
};
use serde_json::json;

fn report(room: &str, title: &str) -> RoomReport {
    RoomReport {
        room: room.to_string(),
        title: title.to_string(),
        protected: false,
        rundowns: 1,
        members_only: false,
    }
}

#[test]
fn the_registry_name_can_never_be_a_room() {
    assert!(!is_valid_room(REGISTRY_NAME));
    assert!(is_valid_room("is-exam-mbv-bio"));
    assert!(is_valid_room("raknong_4ever"));
    assert!(!is_valid_room(""));
    assert!(!is_valid_room("a/b"));
    assert!(!is_valid_room(&"x".repeat(65)));
}

#[test]
fn a_room_reports_once_per_interval() {
    let same = report("exam", "Exam");
    let sent = (1_000, same.clone());
    assert!(should_report(None, &same, 0));
    assert!(!should_report(
        Some(&sent),
        &same,
        1_000 + REPORT_INTERVAL_MS - 1
    ));
    assert!(should_report(
        Some(&sent),
        &same,
        1_000 + REPORT_INTERVAL_MS
    ));
}

#[test]
fn a_change_reports_without_waiting_for_the_interval() {
    // a room is created by several writes in a row; the title arrives after the
    // first one, and must not sit empty in the list for the whole interval
    let sent = (1_000, report("exam", ""));
    assert!(should_report(Some(&sent), &report("exam", "Exam"), 1_001));

    let locked = RoomReport {
        protected: true,
        ..report("exam", "")
    };
    assert!(should_report(Some(&sent), &locked, 1_001));
}

#[test]
fn a_report_registers_a_room_and_keeps_its_first_sighting() {
    let mut registry = Registry::default();
    assert!(registry.apply_report(&report("exam", "Exam"), 100));
    assert!(registry.apply_report(&report("exam", "Exam renamed"), 900));

    let record = &registry.rooms["exam"];
    assert_eq!(record.first_seen, 100);
    assert_eq!(record.last_seen, 900);
    assert_eq!(record.title, "Exam renamed");
}

#[test]
fn a_report_never_touches_owner_or_note() {
    let mut registry = Registry::default();
    registry.set_labels(
        "exam",
        &json!({ "owner": "อ.สมชาย", "note": "IS exam" }),
        50,
    );
    registry.apply_report(&report("exam", "Exam"), 100);

    let record = &registry.rooms["exam"];
    assert_eq!(record.owner, "อ.สมชาย");
    assert_eq!(record.note, "IS exam");
    assert_eq!(record.first_seen, 50);
}

#[test]
fn an_invalid_report_is_refused() {
    let mut registry = Registry::default();
    assert!(!registry.apply_report(&report("@registry", "x"), 1));
    assert!(registry.rooms.is_empty());
}

#[test]
fn labels_register_an_unknown_room_and_update_only_what_is_given() {
    let mut registry = Registry::default();
    registry.set_labels("old-room", &json!({ "owner": "  Ann  " }), 10);
    registry.set_labels("old-room", &json!({ "note": "2569 event" }), 20);

    let record = &registry.rooms["old-room"];
    assert_eq!(record.owner, "Ann", "trimmed");
    assert_eq!(record.note, "2569 event");
    assert_eq!(record.first_seen, 10);
    assert_eq!(record.last_seen, 0, "never seen in use");
}

#[test]
fn labels_are_capped() {
    let mut registry = Registry::default();
    let long = "ก".repeat(500);
    registry.set_labels("room", &json!({ "owner": long }), 1);
    assert_eq!(registry.rooms["room"].owner.chars().count(), 200);
}

#[test]
fn the_list_puts_the_most_recently_used_first() {
    let mut registry = Registry::default();
    registry.apply_report(&report("older", ""), 100);
    registry.apply_report(&report("newer", ""), 200);
    registry.set_labels("never-used", &json!({ "owner": "x" }), 300);

    let names: Vec<&str> = registry.list().iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["newer", "older", "never-used"]);
}

#[test]
fn routing_covers_list_label_forget_and_report() {
    let mut registry = Registry::default();

    let reported = handle(
        &mut registry,
        "POST",
        &["report"],
        &json!({ "room": "exam", "title": "Exam", "protected": true, "rundowns": 2 }),
        100,
    );
    assert_eq!((reported.status, reported.changed), (200, true));

    let labelled = handle(
        &mut registry,
        "PUT",
        &["rooms", "exam"],
        &json!({ "owner": "Ann" }),
        200,
    );
    assert_eq!(labelled.status, 200);
    assert_eq!(labelled.body["owner"], "Ann");
    assert_eq!(labelled.body["protected"], true);

    let listed = handle(&mut registry, "GET", &["rooms"], &json!(null), 300);
    assert_eq!((listed.status, listed.changed), (200, false));
    assert_eq!(listed.body["rooms"][0]["name"], "exam");
    assert_eq!(listed.body["rooms"][0]["firstSeen"], 100);

    let forgotten = handle(
        &mut registry,
        "DELETE",
        &["rooms", "exam"],
        &json!(null),
        400,
    );
    assert_eq!((forgotten.status, forgotten.changed), (200, true));
    assert_eq!(
        handle(
            &mut registry,
            "DELETE",
            &["rooms", "exam"],
            &json!(null),
            500
        )
        .status,
        404
    );
}

#[test]
fn routing_refuses_bad_input() {
    let mut registry = Registry::default();
    assert_eq!(
        handle(&mut registry, "PUT", &["rooms", "bad name"], &json!({}), 1).status,
        400
    );
    assert_eq!(
        handle(
            &mut registry,
            "POST",
            &["report"],
            &json!({ "room": "" }),
            1
        )
        .status,
        400
    );
    assert_eq!(
        handle(&mut registry, "GET", &["nope"], &json!(null), 1).status,
        404
    );
    assert!(registry.rooms.is_empty());
}

#[test]
fn the_document_round_trips() {
    let mut registry = Registry::default();
    registry.apply_report(&report("exam", "Exam"), 100);
    registry.set_labels("exam", &json!({ "owner": "Ann" }), 200);

    let raw = serde_json::to_string(&registry).unwrap();
    let back: Registry = serde_json::from_str(&raw).unwrap();
    assert_eq!(back, registry);
}
