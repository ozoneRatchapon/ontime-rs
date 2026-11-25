//! JSON-backed rundown document.
//!
//! Entries are kept as raw `serde_json::Value` objects rather than a typed struct.
//! The client owns the full `OntimeEntry` shape (and evolves it), so storing the
//! payload verbatim keeps unknown fields intact across a round trip; we only
//! normalise the fields the server is responsible for.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::entry_edits;

use crate::cue::{nth_cue, separated_prefix, CueNumber};

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

/// Why a new entry was refused. Checked before anything changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// The payload carries an id the rundown already has.
    Exists(String),
    /// `parent` names something that is not a group.
    InvalidParent(String),
    NestedGroup,
}

/// Longest rundown title, in characters.
pub const TITLE_MAX_LENGTH: usize = 64;

/// Upstream's `validateRundownTitle`: the reason a title is refused, or `None`.
pub fn validate_title(title: &str) -> Option<&'static str> {
    if title.trim().is_empty() {
        return Some("Title cannot be empty");
    }
    if title.chars().count() > TITLE_MAX_LENGTH {
        return Some("Title must be 64 characters or fewer");
    }
    // upstream tests `[\p{L}\p{N}]`; `is_alphanumeric` is that plus a few
    // alphabetic combining marks (eg a lone Thai vowel sign), a harmless superset
    if !title.chars().any(char::is_alphanumeric) {
        return Some("Title must contain at least one letter or number");
    }
    None
}

/// A renumber was asked for an entry that is not an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAnEvent;

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
            let _ = rundown.place_entry(&payload, Insert::End);
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

    /// The checks upstream's `addEntry` makes before it adds anything.
    pub fn check_add(&self, payload: &Value) -> Result<(), AddError> {
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            if self.entries.contains_key(id) {
                return Err(AddError::Exists(id.to_string()));
            }
        }
        let Some(parent) = payload.get("parent").and_then(Value::as_str) else {
            return Ok(());
        };
        if self.entries.get(parent).map(entry_type) != Some("group") {
            return Err(AddError::InvalidParent(parent.to_string()));
        }
        if payload.get("type").and_then(Value::as_str) == Some("group") {
            return Err(AddError::NestedGroup);
        }
        Ok(())
    }

    /// Creates an entry from a (partial) client payload and links it into the order.
    /// Returns the stored entry.
    ///
    /// Lenient by design (see `check_add` for the strict checks): a taken id is
    /// replaced with a fresh one, and a `parent` that is not a group is ignored.
    /// Adds an entry as the editor does: a new event works out its times from
    /// what was sent and takes the next cue (upstream addEntry).
    pub fn add_entry(&mut self, payload: &Value, at: Insert) -> Value {
        self.insert_entry(payload, at, true)
    }

    /// Adds an entry exactly as given, as an import or a replace does.
    pub fn place_entry(&mut self, payload: &Value, at: Insert) -> Value {
        self.insert_entry(payload, at, false)
    }

    fn insert_entry(&mut self, payload: &Value, at: Insert, as_editor: bool) -> Value {
        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !self.entries.contains_key(*id))
            .map(str::to_string)
            .unwrap_or_else(new_id);

        let mut entry = default_entry(payload.get("type").and_then(Value::as_str), &id);
        let base = entry.clone();
        merge_patch(&mut entry, payload);
        if as_editor && entry_type(&entry) == "event" && entry_edits::touches_times(payload) {
            let times = entry_edits::event_times(&base, payload);
            if let Some(object) = entry.as_object_mut() {
                object.extend(times);
            }
        }
        // the payload carries insert options and an id we may have discarded
        for key in ["before", "after"] {
            entry.as_object_mut().map(|o| o.remove(key));
        }
        set_id(&mut entry, &id);

        let is_group = entry_type(&entry) == "group";
        // the group asked for, when the entry may go there
        let wanted = payload
            .get("parent")
            .and_then(Value::as_str)
            .filter(|parent| {
                !is_group && self.entries.get(*parent).map(entry_type) == Some("group")
            })
            .map(str::to_string);

        self.entries.insert(id.clone(), entry);
        if let Some(object) = self.entries.get_mut(&id).and_then(Value::as_object_mut) {
            // where it ends up decides its parent, not what the payload said
            object.remove("parent");
        }
        let anchor_in_wanted = match (&at, &wanted) {
            (Insert::Before(Some(anchor)) | Insert::After(Some(anchor)), Some(group)) => {
                self.parent_of(anchor).as_deref() == Some(group.as_str())
            }
            _ => false,
        };
        match wanted {
            Some(group) if !anchor_in_wanted => {
                self.link_into_group(&id, &group, matches!(at, Insert::Before(None)));
            }
            _ => self.link(&id, at),
        }
        if !is_group && self.parent_of(&id).is_none() {
            if let Some(object) = self.entries.get_mut(&id).and_then(Value::as_object_mut) {
                object.insert("parent".into(), Value::Null);
            }
        }
        self.recompute();
        // a new event without a cue of its own follows the one before it
        // (upstream createEvent with getCueCandidate); a clone keeps its cue
        let asked_cue = payload
            .get("cue")
            .is_some_and(|cue| !cue.is_null() && !cue.is_object());
        if as_editor && entry_type(&self.entries[&id]) == "event" && !asked_cue {
            let cue = entry_edits::cue_candidate(self, &id);
            if let Some(object) = self.entries.get_mut(&id).and_then(Value::as_object_mut) {
                object.insert("cue".into(), json!(cue));
            }
        }
        self.entries.get(&id).cloned().unwrap_or(Value::Null)
    }

    /// Puts `id` first or last among `group`'s entries.
    fn link_into_group(&mut self, id: &str, group: &str, first: bool) {
        let Some(list) = self
            .entries
            .get_mut(group)
            .and_then(|g| g.get_mut("entries"))
            .and_then(Value::as_array_mut)
        else {
            return self.link(id, Insert::End);
        };
        if first {
            list.insert(0, json!(id));
        } else {
            list.push(json!(id));
        }
        if let Some(object) = self.entries.get_mut(id).and_then(Value::as_object_mut) {
            object.insert("parent".into(), json!(group));
        }
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
        // an end alone locks the end, a duration alone the duration (upstream
        // createEventPatch), so the time just typed is the one that stays
        if entry_type(entry) == "event" && entry_edits::touches_times(&sanitised) {
            let times = entry_edits::event_times(entry, &sanitised);
            if let Some(obj) = sanitised.as_object_mut() {
                obj.extend(times);
            }
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
        // each place in the schedule keeps its times, its link and its group;
        // what is in it moves (upstream swapEventData)
        let (new_a, new_b) = entry_edits::swap_schedules(&a, &b);
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
        if entry_type(&source) != "group" {
            return Some(self.add_entry(&source, at));
        }
        // a group is copied with copies of its entries, never sharing them
        // (upstream makeDeepClone): an entry in two groups breaks the order
        let children: Vec<String> = source
            .get("entries")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let title = match source
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "" => "Untitled".to_string(),
            title => title.to_string(),
        };
        let mut group = source;
        if let Some(object) = group.as_object_mut() {
            object.remove("id");
            object.insert("entries".into(), json!([]));
            object.insert("title".into(), json!(format!("{title} (copy)")));
        }
        let group = self.add_entry(&group, at);
        let group_id = group["id"].as_str().unwrap_or_default().to_string();
        for child in children {
            let Some(mut copy) = self.entries.get(&child).cloned() else {
                continue;
            };
            if let Some(object) = copy.as_object_mut() {
                object.remove("id");
                object.insert("parent".into(), json!(group_id));
            }
            self.add_entry(&copy, Insert::End);
        }
        self.entries.get(&group_id).cloned()
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

    /// Gives `ids`, in that order, consecutive cues from `start` by `increment`.
    /// Every id must be an event; nothing changes otherwise.
    pub fn renumber(
        &mut self,
        ids: &[String],
        prefix: &str,
        start: CueNumber,
        increment: CueNumber,
    ) -> Result<(), NotAnEvent> {
        if !ids
            .iter()
            .all(|id| self.entries.get(id).map(entry_type) == Some("event"))
        {
            return Err(NotAnEvent);
        }
        let prefix = separated_prefix(prefix);
        for (index, id) in (0_i64..).zip(ids) {
            let cue = nth_cue(&prefix, start, increment, index);
            if let Some(entry) = self.entries.get_mut(id) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("cue".into(), json!(cue));
                }
                bump(entry);
            }
        }
        self.recompute();
        Ok(())
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

    /// The group an entry is in, if any.
    pub fn parent_id(&self, id: &str) -> Option<String> {
        self.parent_of(id)
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
}

// #region helpers ================================================

fn wrap_day(value: i64) -> i64 {
    value.rem_euclid(MS_PER_DAY)
}

pub fn entry_type(entry: &Value) -> &str {
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
