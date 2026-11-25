//! The room's own undo history on the Durable Object (plan 018). The policy
//! (when a copy is due, how many are kept) is `ontime_core::snapshots`.
//!
//! - Before a write, the project as it was stored is copied: before any
//!   change that removes things, and every ten minutes for ordinary edits.
//!   A mistake can be undone back to just before it.
//! - `GET /data/snapshots` lists the copies; `POST /data/snapshots/<at>/restore`
//!   puts one back, after copying the current state so the restore can itself
//!   be undone. Both are for an owner or the admin, as the activity log is.
//!
//! A child of `room`, so it reaches the Durable Object's own fields.

use serde_json::json;
use worker::*;

use ontime_core::api::{self, restore_into, ApiRequest};
use ontime_core::audit::{Action, Actor, Note};
use ontime_core::project::Project;
use ontime_core::snapshots::{self, Reason, INDEX_KEY};

use super::{now_ms, OntimeProject, STORAGE_KEY};

/// Whether a request removes things, and so is copied before whatever it changes:
/// a delete, a whole rundown replaced, a backup restored.
pub(super) fn is_destructive(method: &Method, path: &str) -> bool {
    let in_rundowns = path.contains("/data/rundowns");
    match method {
        Method::Delete => in_rundowns,
        // `PUT /data/rundowns/<id>` replaces every entry; `.../entry` and
        // `.../batch` are ordinary edits
        Method::Put => in_rundowns && !path.ends_with("/entry") && !path.ends_with("/batch"),
        Method::Post => path.ends_with("/data/db/restore"),
        _ => false,
    }
}

/// Whether a path is the undo history's, as the Durable Object receives it.
pub(super) fn is_snapshot_path(path: &str) -> bool {
    path.ends_with("/data/snapshots") || path.contains("/data/snapshots/")
}

impl OntimeProject {
    async fn snapshot_index(&mut self) -> Vec<i64> {
        if let Some(index) = &self.snapshot_index {
            return index.clone();
        }
        let index = self
            .state
            .storage()
            .get::<Vec<i64>>(INDEX_KEY)
            .await
            .unwrap_or_default();
        self.snapshot_index = Some(index.clone());
        index
    }

    /// Copies the stored project when a copy is due for `reason`. Never fails
    /// the write it comes before: a history that could not be kept this hour is
    /// not a reason to lose the change.
    pub(super) async fn snapshot_before_write(&mut self, reason: Reason) {
        let mut index = self.snapshot_index().await;
        let mut candidate = index.clone();
        let Some(take) = snapshots::take(&mut candidate, now_ms(), reason, self.changed_since_copy)
        else {
            return;
        };
        let mut storage = self.state.storage();
        // nothing stored yet is nothing to undo
        let Ok(stored) = storage.get::<String>(STORAGE_KEY).await else {
            return;
        };
        if storage.put(&take.key, stored).await.is_err() {
            return;
        }
        index = candidate;
        let _ = storage.put(INDEX_KEY, &index).await;
        if !take.drop.is_empty() {
            let _ = storage.delete_multiple(take.drop).await;
        }
        self.snapshot_index = Some(index);
        self.changed_since_copy = false;
    }

    /// `GET /data/snapshots` and `POST /data/snapshots/<at>/restore`.
    pub(super) async fn snapshots_http(
        &mut self,
        req: &Request,
        access: &ApiRequest,
        actor: Actor,
    ) -> Result<Response> {
        if !api::is_authorised(&self.project, access) {
            return Response::error("Unauthorized", 401);
        }
        if !api::is_owner_or_admin(access) {
            return Response::error("Only an owner of this room can undo its changes", 403);
        }
        if self.is_new {
            return Response::error("This room does not exist", 404);
        }
        let path = req.path();
        let tail = path.split("/data/snapshots").nth(1).unwrap_or_default();
        match (
            req.method(),
            tail.trim_matches('/')
                .split('/')
                .collect::<Vec<_>>()
                .as_slice(),
        ) {
            (Method::Get, [""]) => {
                let index = self.snapshot_index().await;
                // newest first, as a list of undo points reads
                let at: Vec<i64> = index.iter().rev().copied().collect();
                Response::from_json(&json!({ "snapshots": at }))
            }
            (Method::Post, [at, "restore"]) => {
                let Ok(at) = at.parse::<i64>() else {
                    return Response::error("No such snapshot", 404);
                };
                if !self.snapshot_index().await.contains(&at) {
                    return Response::error("No such snapshot", 404);
                }
                let storage = self.state.storage();
                let Some(restored) = storage
                    .get::<String>(&snapshots::key(at))
                    .await
                    .ok()
                    .and_then(|raw| serde_json::from_str::<Project>(&raw).ok())
                else {
                    return Response::error("That snapshot could not be read", 500);
                };
                // the state being replaced is kept, so this can be undone too
                self.snapshot_before_write(Reason::Destructive).await;
                let now = now_ms();
                restore_into(&mut self.project, restored, now);
                let writes = self.audit(actor, Note::new(Action::Restore), now).await;
                self.persist(Some(writes)).await?;
                self.broadcast_refetch("all", None);
                self.broadcast_runtime(now);
                self.sync_alarm().await?;
                Response::from_json(&json!({ "restored": at }))
            }
            _ => Response::error("Not found", 404),
        }
    }
}
