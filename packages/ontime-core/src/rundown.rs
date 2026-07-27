//! JSON-backed rundown document.
//!
//! Entries are kept as raw `serde_json::Value` objects rather than a typed struct.
//! The client owns the full `OntimeEntry` shape (and evolves it), so storing the
//! payload verbatim keeps unknown fields intact across a round trip; we only
//! normalise the fields the server is responsible for.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

pub type Entries = Map<String, Value>;

pub const MS_PER_DAY: i64 = 86_400_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rundown {
    pub id: String,
    pub title: String,
    pub order: Vec<String>,
    #[serde(default)]
    pub flat_order: Vec<String>,
    pub entries: Entries,
    pub revision: u64,
}

/// Where a newly created entry should land relative to an existing one.
#[derive(Debug, Clone, Default)]
pub enum Insert {
    /// Append to the end of the top level.
    #[default]
    End,
    /// Insert before the given entry, or at the very start when `None`.
    Before(Option<String>),
    /// Insert after the given entry, or at the very start when `None`.
    After(Option<String>),
}

impl Insert {
    /// Reads the `before` / `after` insert options off a client payload.
    /// Both accept either an entry id or `true` (meaning "at the boundary").
    pub fn from_payload(payload: &Value) -> Self {
        if let Some(v) = payload.get("before") {
            if let Some(id) = v.as_str() {
                return Insert::Before(Some(id.to_string()));
            }
            if v.as_bool() == Some(true) {
                return Insert::Before(None);
            }
        }
        if let Some(v) = payload.get("after") {
            if let Some(id) = v.as_str() {
                return Insert::After(Some(id.to_string()));
            }
            if v.as_bool() == Some(true) {
                return Insert::After(None);
            }
        }
        Insert::End
    }
}

fn new_id() -> String {
    Uuid::now_v7().to_string()
}

/// Fields the server calculates; a client patch must never overwrite them.
const RUNTIME_FIELDS: [&str; 4] = ["delay", "dayOffset", "gap", "revision"];

impl Rundown {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        let mut rundown = Self {
            id: id.into(),
            title: title.into(),
            order: Vec::new(),
            flat_order: Vec::new(),
            entries: Map::new(),
            revision: 1,
        };
        rundown.recompute();
        rundown
    }

    /// A rundown with a few events, used for a first run so the editor is not empty.
    pub fn demo(id: impl Into<String>, title: impl Into<String>) -> Self {
        let mut rundown = Self::new(id, title);
        for (cue, event_title, duration) in [
            ("1", "Welcome & Intro", 300_000),
            ("2", "Keynote Presentation", 1_800_000),
            ("3", "Q&A Session", 600_000),
        ] {
            let payload = json!({ "cue": cue, "title": event_title, "duration": duration });
            let _ = rundown.add_entry(&payload, Insert::End);
        }
        rundown
    }

    pub fn summary(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "numEntries": self.entries.len(),
            "revision": self.revision,
        })
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.entries.get(id)
    }

    // #region mutations ==========================================

    /// Creates an entry from a (partial) client payload and links it into the order.
    /// Returns the stored entry.
    pub fn add_entry(&mut self, payload: &Value, at: Insert) -> Value {
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !self.entries.contains_key(*id))
            .map(str::to_string)
            .unwrap_or_else(new_id);

        let mut entry = default_entry(payload.get("type").and_then(Value::as_str), &id);
        merge_patch(&mut entry, payload);
        // the payload carries insert options and an id we may have discarded
        for key in ["before", "after"] {
            entry.as_object_mut().map(|o| o.remove(key));
        }
        set_id(&mut entry, &id);

        self.entries.insert(id.clone(), entry);
        self.link(&id, at);
        self.recompute();
        self.entries.get(&id).cloned().unwrap_or(Value::Null)
    }

    /// Applies a partial patch to an existing entry. Returns the stored entry.
    pub fn edit_entry(&mut self, id: &str, patch: &Value) -> Option<Value> {
        let entry = self.entries.get_mut(id)?;
        let mut sanitised = patch.clone();
        if let Some(obj) = sanitised.as_object_mut() {
            for field in RUNTIME_FIELDS {
                obj.remove(field);
            }
            obj.remove("id");
            obj.remove("type");
        }
        merge_patch(entry, &sanitised);
        bump(entry);
        self.recompute();
        self.entries.get(id).cloned()
    }

    /// Applies the same patch to many entries.
    pub fn batch_edit(&mut self, ids: &[String], patch: &Value) {
        for id in ids {
            self.edit_entry(id, patch);
        }
    }

    pub fn delete_entries(&mut self, ids: &[String]) {
        for id in ids {
            // a group takes its children with it
            let children = self
                .entries
                .get(id)
                .filter(|e| entry_type(e) == "group")
                .and_then(|e| e.get("entries"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            for child in &children {
                self.entries.remove(child);
            }
            self.entries.remove(id);
            self.unlink(id);
        }
        self.recompute();
    }

    pub fn delete_all(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.recompute();
    }

    /// Moves `entry_id` relative to `destination_id`.
    /// `order` is one of `before`, `after` or `insert` (into a group).
    pub fn reorder(&mut self, entry_id: &str, destination_id: &str, order: &str) -> bool {
        if entry_id == destination_id || !self.entries.contains_key(entry_id) {
            return false;
        }
        if order != "insert" && !self.entries.contains_key(destination_id) {
            return false;
        }

        self.unlink(entry_id);
        match order {
            "before" => self.link(entry_id, Insert::Before(Some(destination_id.to_string()))),
            "insert" => {
                // `insert` targets a group: append as its last child
                if self.entries.get(destination_id).map(entry_type) == Some("group") {
                    self.push_into_group(destination_id, entry_id);
                } else {
                    self.link(entry_id, Insert::After(Some(destination_id.to_string())));
                }
            }
            _ => self.link(entry_id, Insert::After(Some(destination_id.to_string()))),
        }
        self.recompute();
        true
    }

    pub fn swap(&mut self, from: &str, to: &str) -> bool {
        if !self.entries.contains_key(from) || !self.entries.contains_key(to) {
            return false;
        }
        // swap the cue/content but keep each entry in place, matching the original
        // behaviour where only the payloads move
        let (Some(a), Some(b)) = (
            self.entries.get(from).cloned(),
            self.entries.get(to).cloned(),
        ) else {
            return false;
        };
        let mut new_a = b;
        let mut new_b = a;
        set_id(&mut new_a, from);
        set_id(&mut new_b, to);
        bump(&mut new_a);
        bump(&mut new_b);
        self.entries.insert(from.to_string(), new_a);
        self.entries.insert(to.to_string(), new_b);
        self.recompute();
        true
    }

    /// Duplicates an entry, placing the copy directly after the original by default.
    pub fn clone_entry(&mut self, entry_id: &str, at: Insert) -> Option<Value> {
        let source = self.entries.get(entry_id)?.clone();
        let at = match at {
            Insert::End => Insert::After(Some(entry_id.to_string())),
            other => other,
        };
        Some(self.add_entry(&source, at))
    }

    /// A deep copy under a fresh rundown id, with a fresh id for every entry too.
    ///
    /// Entry ids are the keys of the room-wide report, so a copy that kept the
    /// originals' ids would share their report slots: running the copy would
    /// overwrite the original's recorded times, and the copy would appear to have
    /// already run. Regenerating every id — and rewriting the group `parent` and
    /// child references that point at them — keeps the copy fully independent,
    /// which is what makes "duplicate last year's day and adjust" safe.
    pub fn duplicate(&self, new_title: String) -> Rundown {
        use std::collections::HashMap;

        let id_map: HashMap<String, String> = self
            .entries
            .keys()
            .map(|old| (old.clone(), new_id()))
            .collect();
        let remap = |id: &str| id_map.get(id).cloned();

        let mut entries = Entries::new();
        for (old_id, entry) in &self.entries {
            let new_entry_id = id_map[old_id].clone();
            let mut copy = entry.clone();
            if let Some(obj) = copy.as_object_mut() {
                obj.insert("id".into(), json!(new_entry_id));
                // a group child points back at its group
                if let Some(mapped) = obj.get("parent").and_then(Value::as_str).and_then(remap) {
                    obj.insert("parent".into(), json!(mapped));
                }
                // a group lists its children
                if let Some(children) = obj.get_mut("entries").and_then(Value::as_array_mut) {
                    for child in children.iter_mut() {
                        if let Some(mapped) = child.as_str().and_then(remap) {
                            *child = json!(mapped);
                        }
                    }
                }
            }
            entries.insert(new_entry_id, copy);
        }

        let mut copy = Rundown {
            id: new_id(),
            title: new_title,
            order: self.order.iter().filter_map(|id| remap(id)).collect(),
            flat_order: Vec::new(),
            entries,
            revision: 0,
        };
        copy.recompute(); // rebuilds flat_order and times from the remapped ids
        copy
    }

    /// Renumbers cues sequentially from `start_at`, stepping by `increment`.
    pub fn renumber(&mut self, start_at: f64, increment: f64) {
        let mut value = start_at;
        let ids: Vec<String> = self.flat_order.clone();
        for id in ids {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            if !matches!(entry_type(entry), "event" | "milestone") {
                continue;
            }
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("cue".into(), json!(format_cue(value)));
            }
            bump(entry);
            value += increment;
        }
        self.recompute();
    }

    /// Folds the given entries into a new group, positioned where the first one was.
    pub fn group_entries(&mut self, ids: &[String]) -> Option<Value> {
        let members: Vec<String> = ids
            .iter()
            .filter(|id| self.entries.get(*id).map(entry_type) != Some("group"))
            .filter(|id| self.entries.contains_key(*id))
            .cloned()
            .collect();
        if members.is_empty() {
            return None;
        }

        let anchor = members[0].clone();
        let position = self.order.iter().position(|id| *id == anchor);

        for id in &members {
            self.unlink(id);
        }

        let group_id = new_id();
        let mut group = default_entry(Some("group"), &group_id);
        if let Some(obj) = group.as_object_mut() {
            obj.insert("entries".into(), json!(members));
        }
        self.entries.insert(group_id.clone(), group);

        match position {
            Some(idx) if idx <= self.order.len() => self.order.insert(idx, group_id.clone()),
            _ => self.order.push(group_id.clone()),
        }

        for id in &members {
            if let Some(entry) = self.entries.get_mut(id) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("parent".into(), json!(group_id));
                }
            }
        }

        self.recompute();
        self.entries.get(&group_id).cloned()
    }

    /// Dissolves a group, promoting its children to the top level in place.
    pub fn ungroup(&mut self, group_id: &str) -> bool {
        let Some(group) = self.entries.get(group_id) else {
            return false;
        };
        if entry_type(group) != "group" {
            return false;
        }
        let children: Vec<String> = group
            .get("entries")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let position = self.order.iter().position(|id| id == group_id);
        self.entries.remove(group_id);
        self.order.retain(|id| id != group_id);

        let at = position.unwrap_or(self.order.len()).min(self.order.len());
        for (offset, child) in children.iter().enumerate() {
            if let Some(entry) = self.entries.get_mut(child) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("parent".into(), Value::Null);
                }
            }
            self.order.insert(at + offset, child.clone());
        }

        self.recompute();
        true
    }

    /// Bakes a delay entry into the events that follow it, then removes the delay.
    pub fn apply_delay(&mut self, delay_id: &str) -> bool {
        let Some(delay) = self.entries.get(delay_id) else {
            return false;
        };
        if entry_type(delay) != "delay" {
            return false;
        }
        let amount = delay.get("duration").and_then(Value::as_i64).unwrap_or(0);

        let Some(from) = self.flat_order.iter().position(|id| id == delay_id) else {
            return false;
        };
        let targets: Vec<String> = self.flat_order[from + 1..].to_vec();

        for id in targets {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            if entry_type(entry) != "event" {
                continue;
            }
            let Some(obj) = entry.as_object_mut() else {
                continue;
            };
            let start = obj.get("timeStart").and_then(Value::as_i64).unwrap_or(0);
            let end = obj.get("timeEnd").and_then(Value::as_i64).unwrap_or(0);
            obj.insert("timeStart".into(), json!(wrap_day(start + amount)));
            obj.insert("timeEnd".into(), json!(wrap_day(end + amount)));
            bump(entry);
        }

        self.entries.remove(delay_id);
        self.unlink(delay_id);
        self.recompute();
        true
    }

    // #endregion mutations =======================================
    // #region order bookkeeping ==================================

    fn push_into_group(&mut self, group_id: &str, entry_id: &str) {
        if let Some(group) = self.entries.get_mut(group_id) {
            if let Some(list) = group.get_mut("entries").and_then(Value::as_array_mut) {
                list.push(json!(entry_id));
            }
        }
        if let Some(entry) = self.entries.get_mut(entry_id) {
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("parent".into(), json!(group_id));
            }
        }
    }

    /// Places `id` into the order (top level or inside a group), honouring `at`.
    fn link(&mut self, id: &str, at: Insert) {
        let anchor = match &at {
            Insert::Before(Some(a)) | Insert::After(Some(a)) => Some(a.clone()),
            _ => None,
        };

        // when the anchor lives inside a group, the new entry joins that group
        if let Some(anchor) = &anchor {
            if let Some(parent) = self.parent_of(anchor) {
                let offset = usize::from(matches!(at, Insert::After(_)));
                let inserted = self.entries.get_mut(&parent).and_then(|group| {
                    let list = group.get_mut("entries")?.as_array_mut()?;
                    let idx = list
                        .iter()
                        .position(|v| v.as_str() == Some(anchor.as_str()))?;
                    list.insert((idx + offset).min(list.len()), json!(id));
                    Some(())
                });
                if inserted.is_some() {
                    if let Some(entry) = self.entries.get_mut(id) {
                        if let Some(obj) = entry.as_object_mut() {
                            obj.insert("parent".into(), json!(parent));
                        }
                    }
                    return;
                }
            }
        }

        let position = match &at {
            Insert::End => self.order.len(),
            Insert::Before(None) => 0,
            Insert::After(None) => 0,
            Insert::Before(Some(a)) => self.order.iter().position(|x| x == a).unwrap_or(0),
            Insert::After(Some(a)) => self
                .order
                .iter()
                .position(|x| x == a)
                .map(|i| i + 1)
                .unwrap_or(self.order.len()),
        };
        self.order
            .insert(position.min(self.order.len()), id.to_string());
    }

    /// Removes `id` from the top level order and from any group that holds it.
    fn unlink(&mut self, id: &str) {
        self.order.retain(|x| x != id);
        for (_, entry) in self.entries.iter_mut() {
            let Some(list) = entry.get_mut("entries").and_then(Value::as_array_mut) else {
                continue;
            };
            list.retain(|v| v.as_str() != Some(id));
        }
        if let Some(entry) = self.entries.get_mut(id) {
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("parent".into(), Value::Null);
            }
        }
    }

    fn parent_of(&self, id: &str) -> Option<String> {
        self.entries
            .get(id)
            .and_then(|e| e.get("parent"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    // #endregion order bookkeeping ===============================

    /// Rebuilds `flatOrder` and every server-calculated field.
    /// Must be called after any mutation.
    pub fn recompute(&mut self) {
        // drop dangling ids left behind by a partial write
        let known: Vec<String> = self.entries.keys().cloned().collect();
        self.order.retain(|id| known.contains(id));

        let mut flat: Vec<String> = Vec::with_capacity(self.entries.len());
        for id in &self.order {
            flat.push(id.clone());
            let children = self
                .entries
                .get(id)
                .filter(|e| entry_type(e) == "group")
                .and_then(|e| e.get("entries"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for child in children {
                if self.entries.contains_key(&child) {
                    flat.push(child);
                }
            }
        }
        self.flat_order = flat;

        self.recompute_times();
        self.recompute_groups();
        self.revision = self.revision.saturating_add(1);
    }

    /// Walks the flat order accumulating clock time, delays and day rollovers.
    fn recompute_times(&mut self) {
        let mut previous_end: Option<i64> = None;
        let mut day_offset: i64 = 0;
        let mut accumulated_delay: i64 = 0;

        let ids = self.flat_order.clone();
        for id in ids {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };

            match entry_type(entry) {
                "delay" => {
                    accumulated_delay += entry.get("duration").and_then(Value::as_i64).unwrap_or(0);
                    continue;
                }
                "event" => {}
                // groups and milestones do not consume clock time
                _ => continue,
            }

            let Some(obj) = entry.as_object_mut() else {
                continue;
            };

            let link_start = obj
                .get("linkStart")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let strategy = obj
                .get("timeStrategy")
                .and_then(Value::as_str)
                .unwrap_or("lock-duration")
                .to_string();

            let mut time_start = obj.get("timeStart").and_then(Value::as_i64).unwrap_or(0);
            let mut duration = obj
                .get("duration")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0);
            let mut time_end = obj
                .get("timeEnd")
                .and_then(Value::as_i64)
                .unwrap_or(time_start + duration);

            if link_start {
                if let Some(prev) = previous_end {
                    time_start = prev;
                }
            }

            if strategy == "lock-end" {
                // the end is authoritative: derive duration, wrapping past midnight
                duration = if time_end >= time_start {
                    time_end - time_start
                } else {
                    time_end + MS_PER_DAY - time_start
                };
            } else {
                time_end = wrap_day(time_start + duration);
            }

            // a start that lands before the previous end means we crossed midnight
            let gap = match previous_end {
                _ if link_start => 0,
                Some(prev) if time_start >= prev => time_start - prev,
                Some(_) => {
                    day_offset += 1;
                    0
                }
                None => 0,
            };

            obj.insert("timeStart".into(), json!(wrap_day(time_start)));
            obj.insert("timeEnd".into(), json!(wrap_day(time_end)));
            obj.insert("duration".into(), json!(duration));
            obj.insert("gap".into(), json!(gap));
            obj.insert("delay".into(), json!(accumulated_delay));
            obj.insert("dayOffset".into(), json!(day_offset));

            previous_end = Some(wrap_day(time_start + duration));
        }
    }

    /// Groups report the span and duration of the events they contain.
    fn recompute_groups(&mut self) {
        let group_ids: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, e)| entry_type(e) == "group")
            .map(|(id, _)| id.clone())
            .collect();

        for group_id in group_ids {
            let children: Vec<String> = self
                .entries
                .get(&group_id)
                .and_then(|e| e.get("entries"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            let mut start: Option<i64> = None;
            let mut end: Option<i64> = None;
            let mut duration: i64 = 0;
            let mut is_first_linked = false;

            for (idx, child) in children.iter().enumerate() {
                let Some(entry) = self.entries.get(child) else {
                    continue;
                };
                if entry_type(entry) != "event" {
                    continue;
                }
                if idx == 0 {
                    is_first_linked = entry
                        .get("linkStart")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                }
                let child_start = entry.get("timeStart").and_then(Value::as_i64).unwrap_or(0);
                let child_end = entry.get("timeEnd").and_then(Value::as_i64).unwrap_or(0);
                duration += entry.get("duration").and_then(Value::as_i64).unwrap_or(0);
                start = Some(start.map_or(child_start, |s: i64| s.min(child_start)));
                end = Some(child_end);
            }

            if let Some(group) = self.entries.get_mut(&group_id) {
                if let Some(obj) = group.as_object_mut() {
                    obj.insert(
                        "timeStart".into(),
                        start.map(|v| json!(v)).unwrap_or(Value::Null),
                    );
                    obj.insert(
                        "timeEnd".into(),
                        end.map(|v| json!(v)).unwrap_or(Value::Null),
                    );
                    obj.insert("duration".into(), json!(duration));
                    obj.insert("isFirstLinked".into(), json!(is_first_linked));
                }
            }
        }
    }
}

// #region helpers ================================================

fn wrap_day(value: i64) -> i64 {
    value.rem_euclid(MS_PER_DAY)
}

fn entry_type(entry: &Value) -> &str {
    entry.get("type").and_then(Value::as_str).unwrap_or("event")
}

fn set_id(entry: &mut Value, id: &str) {
    if let Some(obj) = entry.as_object_mut() {
        obj.insert("id".into(), json!(id));
    }
}

fn bump(entry: &mut Value) {
    if let Some(obj) = entry.as_object_mut() {
        let revision = obj.get("revision").and_then(Value::as_u64).unwrap_or(0);
        obj.insert("revision".into(), json!(revision + 1));
    }
}

fn format_cue(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Shallow merge of `patch` into `target`, ignoring nulls for keys the patch omits.
fn merge_patch(target: &mut Value, patch: &Value) {
    let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) else {
        return;
    };
    for (key, value) in patch {
        if key == "before" || key == "after" {
            continue;
        }
        target.insert(key.clone(), value.clone());
    }
}

/// A fully populated entry of the given type, matching the client's `OntimeEntry`.
pub fn default_entry(entry_type: Option<&str>, id: &str) -> Value {
    match entry_type.unwrap_or("event") {
        "delay" => json!({
            "type": "delay",
            "id": id,
            "duration": 0,
            "parent": Value::Null,
        }),
        "milestone" => json!({
            "type": "milestone",
            "id": id,
            "cue": "",
            "title": "",
            "note": "",
            "colour": "",
            "custom": {},
            "parent": Value::Null,
            "revision": 1,
        }),
        "group" => json!({
            "type": "group",
            "id": id,
            "title": "",
            "note": "",
            "entries": [],
            "targetDuration": Value::Null,
            "colour": "",
            "custom": {},
            "revision": 1,
            "timeStart": Value::Null,
            "timeEnd": Value::Null,
            "duration": 0,
            "isFirstLinked": false,
        }),
        _ => json!({
            "type": "event",
            "id": id,
            "flag": false,
            "cue": "",
            "title": "",
            "note": "",
            "endAction": "none",
            "timerType": "count-down",
            "countToEnd": false,
            "linkStart": false,
            "timeStrategy": "lock-duration",
            "timeStart": 0,
            "timeEnd": 0,
            "duration": 0,
            "skip": false,
            "colour": "",
            "timeWarning": 120_000,
            "timeDanger": 60_000,
            "custom": {},
            "triggers": [],
            "parent": Value::Null,
            "revision": 1,
            "delay": 0,
            "dayOffset": 0,
            "gap": 0,
        }),
    }
}

// #endregion helpers =============================================
