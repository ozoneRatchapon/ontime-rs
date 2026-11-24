//! Plan 007: the audit log. Who counts as the actor, what each change records,
//! and how the chunked log keeps, merges, drops and pages its lines.

use ontime_core::api::{apply_socket_command, handle, is_owner_or_admin, ApiRequest, HttpMethod};
use ontime_core::audit::{
    chunk_key, Action, Actor, AuditIndex, Chunk, Note, Page, Retention, Subject, MERGE_WINDOW_MS,
};
use ontime_core::audit_describe::{actor, http_note, socket_note};
use ontime_core::project::{hash_password, Project};
use ontime_core::registry::{Member, Role};
use ontime_core::rundown::{Insert, Rundown, MS_PER_DAY};
use serde_json::{json, Value};
use uuid::Uuid;

const NOW: i64 = 1_800_000_000_000;

fn person() -> Member {
    Member { person: Uuid::nil(), name: "Somchai".into(), role: Role::Operator }
}

fn request(method: HttpMethod, path: &str, body: Value, token: Option<&str>, member: Option<Role>) -> ApiRequest {
    ApiRequest {
        method,
        path: path.into(),
        body,
        token: token.map(hash_password),
        master_token: Some(hash_password("master")),
        room_is_new: false,
        member,
    }
}

/// Rundown `show`, loaded: `a` (cue 1, "Welcome") and `b` (cue 2.1, "Keynote").
fn room() -> Project {
    let mut project = Project { password_hash: Some(hash_password("room")), ..Project::default() };
    let mut rundown = Rundown::new("show", "Show");
    rundown.add_entry(&json!({ "id": "a", "type": "event", "cue": "1", "title": "Welcome" }), Insert::End);
    rundown.add_entry(&json!({ "id": "b", "type": "event", "cue": "2.1", "title": "Keynote" }), Insert::End);
    project.rundowns.insert("show".into(), rundown);
    project.loaded = "show".into();
    project.ensure_consistent();
    project
}

fn keynote() -> Option<Subject> {
    Some(Subject { cue: "2.1".into(), title: "Keynote".into() })
}

// --- who ----------------------------------------------------------------------

#[test]
fn the_actor_is_whatever_let_the_request_in() {
    let project = room();
    let path = "/data/settings";
    let with = |token, member| request(HttpMethod::Post, path, json!({}), token, member);

    let somchai = person();
    assert_eq!(
        actor(&project, &with(None, Some(Role::Operator)), Some(&somchai)),
        Actor::Person { id: Uuid::nil(), name: "Somchai".into() }
    );
    assert_eq!(actor(&project, &with(Some("master"), None), None), Actor::Admin);
    assert_eq!(actor(&project, &with(Some("room"), None), None), Actor::RoomPassword);
    assert_eq!(actor(&project, &with(None, None), None), Actor::Anyone);
    assert_eq!(actor(&project, &with(Some("wrong"), None), None), Actor::Anyone);
}

#[test]
fn a_session_wins_over_a_password_sent_alongside_it() {
    let req = request(HttpMethod::Post, "/data/settings", json!({}), Some("master"), Some(Role::Owner));
    assert!(matches!(actor(&room(), &req, Some(&person())), Actor::Person { .. }));
}

#[test]
fn owners_and_the_admin_may_read_the_log_and_nobody_else() {
    let get = |token, member| request(HttpMethod::Get, "/data/activity", json!({}), token, member);
    assert!(is_owner_or_admin(&get(None, Some(Role::Owner))));
    assert!(is_owner_or_admin(&get(Some("master"), None)));
    assert!(!is_owner_or_admin(&get(None, Some(Role::Operator))));
    assert!(!is_owner_or_admin(&get(Some("room"), None)));
    assert!(!is_owner_or_admin(&get(None, None)));
}

// --- what ---------------------------------------------------------------------

fn note_for(method: HttpMethod, path: &str, body: Value) -> Option<Note> {
    http_note(&room(), &request(method, path, body, Some("room"), None))
}

#[test]
fn each_kind_of_change_is_described() {
    use HttpMethod::*;
    let cases: Vec<(HttpMethod, &str, Value, Action)> = vec![
        (Post, "/data/rundowns", json!({ "title": "Day 2" }), Action::RundownCreate),
        (Patch, "/data/rundowns/show", json!({ "title": "Day 1" }), Action::RundownRename),
        (Put, "/data/rundowns/show", json!({ "entries": [] }), Action::RundownReplace),
        (Delete, "/data/rundowns/show", json!({}), Action::RundownDelete),
        (Post, "/data/rundowns/show/load", json!({}), Action::RundownLoad),
        (Post, "/data/rundowns/show/duplicate", json!({}), Action::RundownDuplicate),
        (Post, "/data/rundowns/show/entry", json!({ "title": "New" }), Action::EntryAdd),
        (Put, "/data/rundowns/show/entry", json!({ "id": "b", "note": "x" }), Action::EntryEdit),
        (Put, "/data/rundowns/show/batch", json!({ "ids": ["a"], "data": { "colour": "red" } }), Action::EntryEdit),
        (Delete, "/data/rundowns/show/entries", json!({ "ids": ["b"] }), Action::EntryDelete),
        (Delete, "/data/rundowns/show/all", json!({}), Action::EntryDeleteAll),
        (Patch, "/data/rundowns/show/reorder", json!({ "entryId": "b" }), Action::EntryMove),
        (Patch, "/data/rundowns/show/swap", json!({ "from": "a", "to": "b" }), Action::EntrySwap),
        (Patch, "/data/rundowns/show/renumber", json!({ "ids": ["a", "b"] }), Action::EntryRenumber),
        (Post, "/data/rundowns/show/clone/a", json!({}), Action::EntryClone),
        (Post, "/data/rundowns/show/group", json!({ "ids": ["a", "b"] }), Action::EntryGroup),
        (Post, "/data/project", json!({ "title": "Gala" }), Action::ProjectData),
        (Post, "/data/settings", json!({ "language": "th" }), Action::Settings),
        (Post, "/data/view-settings", json!({ "dangerColor": "#f00" }), Action::ViewSettings),
        (Post, "/data/custom-fields", json!({ "label": "Camera" }), Action::CustomFields),
        (Post, "/data/url-presets", json!({}), Action::UrlPresets),
        (Post, "/data/report/archive", json!({}), Action::ReportArchive),
        (Delete, "/data/report/all", json!({}), Action::ReportClear),
        (Post, "/data/members-only", json!({ "on": true }), Action::MembersOnlyOn),
        (Post, "/data/members-only", json!({ "on": false }), Action::MembersOnlyOff),
        (Post, "/data/session/password", json!({ "password": "new" }), Action::PasswordSet),
        (Post, "/data/session/password", json!({ "password": null }), Action::PasswordCleared),
        (Post, "/data/assets/css", json!({ "css": "body{}" }), Action::Css),
        (Post, "/data/db/restore", json!({}), Action::Restore),
    ];
    for (method, path, body, action) in cases {
        let note = note_for(method, path, body).unwrap_or_else(|| panic!("{method:?} {path} is not described"));
        assert_eq!(note.action, action, "{method:?} {path}");
    }
}

#[test]
fn reads_and_side_requests_are_not_recorded() {
    use HttpMethod::*;
    for (method, path) in [
        (Get, "/data/rundowns/current"),
        (Get, "/data/activity"),
        (Post, "/data/session/url"),
        (Post, "/data/db/download"),
        (Post, "/data/members-only"),
    ] {
        assert_eq!(note_for(method, path, json!({})), None, "{method:?} {path}");
    }
}

#[test]
fn a_host_prefix_does_not_change_the_description() {
    let note = note_for(HttpMethod::Delete, "/r/gala/data/rundowns/show", json!({}));
    assert_eq!(note.map(|n| n.action), Some(Action::RundownDelete));
}

#[test]
fn an_edit_records_which_fields_changed_never_their_values() {
    let body = json!({ "id": "b", "note": "Somchai's phone: 081", "custom": { "camera": "Nok" } });
    let note = note_for(HttpMethod::Put, "/data/rundowns/show/entry", body).unwrap();
    assert_eq!(note.subject, keynote());
    let mut fields = note.fields.clone();
    fields.sort();
    assert_eq!(fields, ["custom.camera", "note"]);

    let stored = serde_json::to_string(&note).unwrap();
    assert!(!stored.contains("081"), "{stored}");
    assert!(!stored.contains("Nok"), "{stored}");
}

#[test]
fn a_deletion_names_the_entry_by_its_old_cue_and_counts_the_rest() {
    let note = note_for(HttpMethod::Delete, "/data/rundowns/show/entries", json!({ "ids": ["b", "a"] })).unwrap();
    assert_eq!(note.subject, keynote());
    assert_eq!(note.more, 1);
}

#[test]
fn playback_names_the_event_it_affected() {
    let mut project = room();
    let before = project.runtime.selected_id.clone();
    assert!(apply_socket_command(&mut project, "start", Some(&json!({ "id": "b" })), NOW));
    let start = socket_note(&project, "start", Some(&json!({ "id": "b" })), before.as_deref()).unwrap();
    assert_eq!((start.action, start.subject.clone()), (Action::Start, keynote()));

    let before = project.runtime.selected_id.clone();
    assert!(apply_socket_command(&mut project, "addtime", Some(&json!(60_000)), NOW));
    let add = socket_note(&project, "addtime", Some(&json!(60_000)), before.as_deref()).unwrap();
    assert_eq!((add.action, add.subject, add.amount), (Action::AddTime, keynote(), Some(60_000)));

    let before = project.runtime.selected_id.clone();
    assert!(apply_socket_command(&mut project, "stop", None, NOW));
    let stop = socket_note(&project, "stop", None, before.as_deref()).unwrap();
    assert_eq!((stop.action, stop.subject), (Action::Stop, keynote()));
}

#[test]
fn teleprompter_commands_are_recorded_with_the_event_read() {
    let mut project = room();
    // the test events have titles but no notes
    project.teleprompter.script = "title".into();
    for (payload, action) in [
        (json!({ "goto": { "cue": "2.1" } }), Action::PrompterMove),
        (json!("toggle"), Action::PrompterPlay),
        (json!({ "speedBy": 5 }), Action::PrompterSpeed),
        (json!("pause"), Action::PrompterPause),
        (json!({ "mode": "script" }), Action::PrompterMode),
        (json!({ "blackout": "on" }), Action::PrompterBlackout),
    ] {
        assert!(apply_socket_command(&mut project, "teleprompter", Some(&payload), NOW), "{payload}");
        let note = socket_note(&project, "teleprompter", Some(&payload), None).unwrap();
        assert_eq!((note.action, note.subject), (action, keynote()), "{payload}");
    }
    // a refused command is never applied, so the host never asks
    assert_eq!(socket_note(&project, "teleprompter", Some(&json!("dance")), None), None);
}

#[test]
fn quick_repeated_speed_taps_merge_into_one_line() {
    let mut store = Store::new();
    let faster = || Note::new(Action::PrompterSpeed).about(keynote());
    store.record(Actor::Anyone, faster(), NOW, Retention::ROOM);
    store.record(Actor::Anyone, faster(), NOW + 300, Retention::ROOM);
    assert_eq!(store.chunks[&0].lines.len(), 1);
    assert_eq!(store.chunks[&0].lines[0].times, 2);
}

#[test]
fn the_overlay_and_aux_timers_are_not_recorded() {
    let project = room();
    assert_eq!(socket_note(&project, "message", Some(&json!({ "timer": { "text": "hi" } })), None), None);
    assert_eq!(socket_note(&project, "auxtimer", Some(&json!({ "1": "start" })), None), None);
}

// --- storing ------------------------------------------------------------------

const SMALL: Retention = Retention { max_lines: 6, max_age_ms: 10 * MS_PER_DAY, chunk_lines: 2 };

/// Records `note` the way a host does: loads the head, stores what it is told.
struct Store {
    index: AuditIndex,
    chunks: std::collections::BTreeMap<u64, Chunk>,
}

impl Store {
    fn new() -> Self {
        Self { index: AuditIndex::default(), chunks: Default::default() }
    }

    fn record(&mut self, actor: Actor, note: Note, now: i64, retention: Retention) {
        let head = self.index.head().and_then(|no| self.chunks.get(&no).cloned());
        let writes = self.index.record(head, actor, note, now, retention);
        for no in writes.delete {
            self.chunks.remove(&no);
        }
        self.chunks.insert(writes.put.no, writes.put);
    }

    fn page(&self, before: Option<i64>, limit: usize, now: i64, retention: Retention) -> Page {
        let mut page = Page::new(before, Some(limit), now, retention);
        for no in self.index.newest_first(before) {
            if page.take(&self.chunks[&no]) {
                break;
            }
        }
        page
    }
}

fn line(n: i64) -> Note {
    Note::new(Action::EntryEdit).about(Some(Subject::titled(format!("event {n}"))))
}

#[test]
fn one_change_is_one_line() {
    let mut store = Store::new();
    store.record(Actor::Anyone, line(1), NOW, Retention::ROOM);
    assert_eq!(store.index.len(), 1);
    assert_eq!(store.chunks[&0].lines[0].at, NOW);
}

#[test]
fn the_oldest_lines_go_once_the_log_is_full() {
    let mut store = Store::new();
    for n in 0..9 {
        store.record(Actor::Anyone, line(n), NOW + n * 1_000, SMALL);
    }
    // 6 lines at most, kept as 3 chunks of 2; the newest chunk holds the 9th
    assert!(store.index.len() <= SMALL.max_lines, "{}", store.index.len());
    assert_eq!(store.chunks.len(), store.index.chunks.len());
    let page = store.page(None, 50, NOW + 9_000, SMALL);
    let titles: Vec<_> = page.lines.iter().map(|l| l.note.subject.clone().unwrap().title).collect();
    assert_eq!(titles, ["event 8", "event 7", "event 6", "event 5", "event 4"]);
}

#[test]
fn lines_older_than_the_limit_go() {
    let mut store = Store::new();
    store.record(Actor::Anyone, line(1), NOW, SMALL);
    store.record(Actor::Anyone, line(2), NOW + 1, SMALL);
    let later = NOW + 11 * MS_PER_DAY;
    store.record(Actor::Anyone, line(3), later, SMALL);
    assert_eq!(store.chunks.keys().copied().collect::<Vec<_>>(), [1]);
    assert_eq!(store.page(None, 50, later, SMALL).lines.len(), 1);
}

#[test]
fn old_lines_are_hidden_even_before_they_are_dropped() {
    let mut store = Store::new();
    store.record(Actor::Anyone, line(1), NOW, SMALL);
    // nothing new was written, so nothing was dropped yet
    assert_eq!(store.page(None, 50, NOW + 11 * MS_PER_DAY, SMALL).lines.len(), 0);
}

#[test]
fn a_head_that_failed_to_load_is_never_overwritten() {
    let mut index = AuditIndex::default();
    index.record(None, Actor::Anyone, line(1), NOW, Retention::ROOM);
    // the host could not load chunk 0, so it passes nothing
    let writes = index.record(None, Actor::Anyone, line(2), NOW + 1, Retention::ROOM);
    assert_eq!(writes.put.no, 1);
    assert_eq!(index.chunks.iter().map(|meta| meta.no).collect::<Vec<_>>(), [0, 1]);
}

#[test]
fn repeated_playback_merges_into_one_line() {
    let mut store = Store::new();
    let pause = || Note::new(Action::Pause).about(keynote());
    store.record(Actor::RoomPassword, pause(), NOW, Retention::ROOM);
    store.record(Actor::RoomPassword, pause(), NOW + 500, Retention::ROOM);
    store.record(Actor::RoomPassword, pause(), NOW + 500 + MERGE_WINDOW_MS, Retention::ROOM);
    let lines = &store.chunks[&0].lines;
    assert_eq!(lines.len(), 1);
    assert_eq!((lines[0].times, lines[0].at), (3, NOW + 500 + MERGE_WINDOW_MS));
}

#[test]
fn only_identical_playback_close_together_merges() {
    let mut store = Store::new();
    let pause = || Note::new(Action::Pause).about(keynote());
    store.record(Actor::RoomPassword, pause(), NOW, Retention::ROOM);
    // too late
    store.record(Actor::RoomPassword, pause(), NOW + MERGE_WINDOW_MS + 1, Retention::ROOM);
    // someone else
    store.record(Actor::Admin, pause(), NOW + MERGE_WINDOW_MS + 2, Retention::ROOM);
    // not playback
    store.record(Actor::Admin, line(1), NOW + MERGE_WINDOW_MS + 3, Retention::ROOM);
    store.record(Actor::Admin, line(1), NOW + MERGE_WINDOW_MS + 4, Retention::ROOM);
    assert_eq!(store.index.len(), 5);
}

#[test]
fn pages_run_newest_first_and_continue_from_a_cursor() {
    let mut store = Store::new();
    for n in 0..5 {
        store.record(Actor::Anyone, line(n), NOW + n, Retention::ROOM);
    }
    let first = store.page(None, 2, NOW + 10, Retention::ROOM);
    assert_eq!(first.lines.iter().map(|l| l.at).collect::<Vec<_>>(), [NOW + 4, NOW + 3]);
    let before = first.next_before();
    assert_eq!(before, Some(NOW + 3));
    let second = store.page(before, 10, NOW + 10, Retention::ROOM);
    assert_eq!(second.lines.iter().map(|l| l.at).collect::<Vec<_>>(), [NOW + 2, NOW + 1, NOW]);
    assert_eq!(second.next_before(), None);
}

#[test]
fn a_cursor_skips_chunks_without_loading_them() {
    let mut store = Store::new();
    for n in 0..6 {
        store.record(Actor::Anyone, line(n), NOW + n, SMALL);
    }
    // chunks: 0 = [0, 1], 1 = [2, 3], 2 = [4, 5]. A chunk's lines are only
    // known to be at or after its older neighbour's newest, since two lines
    // may share a millisecond, so it is skipped once that neighbour is too.
    assert_eq!(store.index.newest_first(Some(NOW + 1)).collect::<Vec<_>>(), [0]);
    assert_eq!(store.index.newest_first(Some(NOW + 3)).collect::<Vec<_>>(), [1, 0]);
    assert_eq!(store.index.newest_first(Some(NOW + 4)).collect::<Vec<_>>(), [2, 1, 0]);
    assert_eq!(store.index.newest_first(None).collect::<Vec<_>>(), [2, 1, 0]);
}

#[test]
fn a_line_stores_compactly() {
    let note = Note::new(Action::Start).about(keynote());
    let mut index = AuditIndex::default();
    let writes = index.record(None, Actor::RoomPassword, note, NOW, Retention::ROOM);
    let stored = serde_json::to_string(&writes.put.lines[0]).unwrap();
    assert_eq!(
        stored,
        r#"{"at":1800000000000,"actor":{"kind":"roomPassword"},"action":"start","subject":{"cue":"2.1","title":"Keynote"}}"#
    );
    let back: ontime_core::audit::AuditLine = serde_json::from_str(&stored).unwrap();
    assert_eq!(back.times, 1);
    assert_eq!(chunk_key(7), "audit_v1:0000000007");
}

// --- the room document -------------------------------------------------------

#[test]
fn a_restore_keeps_this_rooms_log_and_a_download_leaves_it_out() {
    let mut project = room();
    let mut index = AuditIndex::default();
    index.record(None, Actor::Admin, line(1), NOW, Retention::ROOM);
    project.audit = index.clone();

    let download = handle(&mut project, &request(HttpMethod::Post, "/data/db/download", json!({}), Some("room"), None), NOW);
    let backup = download.body.unwrap();
    assert!(backup.get("audit").is_none());

    let mut foreign = backup.clone();
    foreign["audit"] = json!({ "chunks": [{ "no": 99, "newest": 0, "len": 1 }] });
    let restored = handle(&mut project, &request(HttpMethod::Post, "/data/db/restore", foreign, Some("room"), None), NOW);
    assert_eq!(restored.status, 200);
    assert_eq!(project.audit, index);
}
