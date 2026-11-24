//! Plan 007, registry side: people events for the admin. Who did it, what it
//! was, and that nothing is recorded for reads or refused requests.

use ontime_core::audit::{Action, Actor, Note, Subject};
use ontime_core::directory::{self, Directory};
use ontime_core::registry_audit::{note, redeemed};
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000_000;

struct World {
    directory: Directory,
    n: u32,
}

impl World {
    fn new() -> Self {
        Self {
            directory: Directory::default(),
            n: 0,
        }
    }

    /// Runs a request the way the host does: describe first, then handle, and
    /// keep the line only for a change that succeeded.
    fn call(&mut self, method: &str, path: &str, body: Value) -> (Value, Option<(Actor, Note)>) {
        let segments: Vec<&str> = path.split('/').collect();
        let described = note(&self.directory, method, &segments, &body, NOW);
        let n = &mut self.n;
        let mut secrets = || {
            *n += 1;
            format!("secret-{n}")
        };
        let response = directory::handle(
            &mut self.directory,
            method,
            &segments,
            &body,
            NOW,
            &mut secrets,
        );
        let line = match response.status < 400 && response.changed {
            true => described.or_else(|| redeemed(method, &segments, &response.body)),
            false => None,
        };
        (response.body, line)
    }

    fn person(&mut self, name: &str) -> String {
        let (person, _) = self.call("POST", "people", json!({ "name": name }));
        person["id"].as_str().unwrap().to_string()
    }

    fn sign_in(&mut self, id: &str) -> String {
        let (invite, _) = self.call("POST", &format!("people/{id}/invite"), Value::Null);
        let (signed, _) = self.call("POST", "auth/redeem", json!({ "code": invite["code"] }));
        signed["token"].as_str().unwrap().to_string()
    }
}

fn titled(name: &str) -> Option<Subject> {
    Some(Subject::titled(name))
}

#[test]
fn the_admin_making_and_removing_people_is_recorded() {
    let mut world = World::new();
    let (person, line) = world.call("POST", "people", json!({ "name": "Somchai" }));
    let (actor, made) = line.unwrap();
    assert_eq!(
        (actor, made.action, made.subject),
        (Actor::Admin, Action::PersonCreate, titled("Somchai"))
    );

    let id = person["id"].as_str().unwrap();
    let (_, line) = world.call("POST", &format!("people/{id}/invite"), Value::Null);
    assert_eq!(
        line.map(|(_, n)| (n.action, n.subject)),
        Some((Action::InviteIssue, titled("Somchai")))
    );

    let (_, line) = world.call("DELETE", &format!("people/{id}"), Value::Null);
    // named from before the removal
    assert_eq!(
        line.map(|(_, n)| (n.action, n.subject)),
        Some((Action::PersonRemove, titled("Somchai")))
    );
}

#[test]
fn spending_an_invite_is_recorded_as_that_person() {
    let mut world = World::new();
    let id = world.person("Nok");
    let (invite, _) = world.call("POST", &format!("people/{id}/invite"), Value::Null);
    let (_, line) = world.call("POST", "auth/redeem", json!({ "code": invite["code"] }));
    let (actor, redeemed) = line.unwrap();
    assert_eq!(redeemed.action, Action::InviteRedeem);
    assert!(matches!(actor, Actor::Person { ref name, .. } if name == "Nok"));
}

#[test]
fn an_owner_managing_operators_is_recorded_with_the_room() {
    let mut world = World::new();
    let owner = world.person("Owner");
    world.call("POST", "report", json!({ "room": "gala" }));
    let (_, line) = world.call("PUT", "rooms/gala", json!({ "owners": [owner] }));
    let (actor, set) = line.unwrap();
    assert_eq!(
        (actor, set.action, set.fields.clone(), set.room.clone()),
        (
            Actor::Admin,
            Action::MembersChange,
            vec!["owners".to_string()],
            Some("gala".to_string())
        )
    );

    let session = world.sign_in(&owner);
    let (made, line) = world.call(
        "POST",
        "auth/rooms/gala/operators",
        json!({ "token": session, "name": "Op" }),
    );
    let (actor, added) = line.unwrap();
    assert!(matches!(actor, Actor::Person { ref name, .. } if name == "Owner"));
    assert_eq!(
        (added.action, added.subject, added.room),
        (Action::OperatorAdd, titled("Op"), Some("gala".into()))
    );

    let op = made["person"]["id"].as_str().unwrap();
    let (_, line) = world.call(
        "DELETE",
        &format!("auth/rooms/gala/operators/{op}"),
        json!({ "token": session }),
    );
    assert_eq!(
        line.map(|(_, n)| (n.action, n.subject)),
        Some((Action::OperatorRemove, titled("Op")))
    );
}

#[test]
fn signing_out_is_recorded_as_the_person() {
    let mut world = World::new();
    let id = world.person("Nok");
    let session = world.sign_in(&id);
    let (_, line) = world.call("POST", "auth/logout", json!({ "token": session }));
    let (actor, out) = line.unwrap();
    assert_eq!(out.action, Action::SignOut);
    assert!(matches!(actor, Actor::Person { ref name, .. } if name == "Nok"));
}

#[test]
fn reads_reports_labels_and_refusals_are_not_recorded() {
    let mut world = World::new();
    let id = world.person("Nok");
    let session = world.sign_in(&id);
    for (method, path, body) in [
        ("GET", "people", Value::Null),
        ("POST", "report", json!({ "room": "gala" })),
        ("PUT", "rooms/gala", json!({ "note": "main hall" })),
        ("POST", "auth/me", json!({ "token": session })),
        (
            "POST",
            "auth/check",
            json!({ "token": session, "room": "gala" }),
        ),
        // not an owner
        (
            "POST",
            "auth/rooms/gala/operators",
            json!({ "token": session, "name": "x" }),
        ),
        // no such person
        (
            "DELETE",
            "people/00000000-0000-0000-0000-000000000000",
            Value::Null,
        ),
    ] {
        let (_, line) = world.call(method, path, body);
        assert_eq!(line, None, "{method} {path}");
    }
}
