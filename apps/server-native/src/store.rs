//! Keeping a project on disk.
//!
//! The Worker cannot do this at all — it is one of the reasons this binary
//! exists. One JSON file per room, written whole.

use std::path::{Path, PathBuf};

use ontime_core::audit::{chunk_key, Chunk, Writes};
use ontime_core::project::Project;

fn path_for(data_dir: &Path, room: &str) -> PathBuf {
    // room names are validated before they reach here, so this cannot escape
    // the data directory
    data_dir.join(format!("{room}.json"))
}

/// Reads a room's project, falling back to a fresh one.
///
/// A file that cannot be parsed is treated as absent rather than fatal: a
/// corrupt project should not stop the server coming up before a show.
pub async fn load(data_dir: &Path, room: &str) -> Project {
    let path = path_for(data_dir, room);

    let Ok(raw) = tokio::fs::read_to_string(&path).await else {
        return Project::default();
    };

    match serde_json::from_str::<Project>(&raw) {
        Ok(mut project) => {
            project.ensure_consistent();
            project
        }
        Err(error) => {
            eprintln!(
                "could not read {}: {error} — starting fresh",
                path.display()
            );
            Project::default()
        }
    }
}

/// Writes a room's project.
///
/// Written to a temporary file and renamed, so a crash mid-write cannot leave a
/// half-written project where a whole one used to be.
pub async fn save(data_dir: &Path, room: &str, project: &Project) -> std::io::Result<()> {
    let path = path_for(data_dir, room);
    let temporary = path.with_extension("json.tmp");

    let serialised = serde_json::to_string_pretty(project)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    tokio::fs::write(&temporary, serialised).await?;
    tokio::fs::rename(&temporary, &path).await
}

/// Whether a room has ever been written.
///
/// Used by the creation gate: a room with nothing on disk is one that does not
/// exist yet, however confidently its name was typed into a URL.
pub async fn exists(data_dir: &Path, room: &str) -> bool {
    tokio::fs::try_exists(path_for(data_dir, room))
        .await
        .unwrap_or(false)
}

// #region audit log ============================================================

/// A room's audit chunks live beside its project, one file per chunk.
fn audit_dir(data_dir: &Path, room: &str) -> PathBuf {
    data_dir.join(format!("{room}.audit"))
}

fn chunk_path(data_dir: &Path, room: &str, no: u64) -> PathBuf {
    audit_dir(data_dir, room).join(format!("{}.json", chunk_key(no).replace(':', "_")))
}

/// One stored chunk, or `None` when it is missing or unreadable.
pub async fn load_chunk(data_dir: &Path, room: &str, no: u64) -> Option<Chunk> {
    let raw = tokio::fs::read_to_string(chunk_path(data_dir, room, no)).await.ok()?;
    serde_json::from_str(&raw).ok()
}

/// Writes the chunk a change added and drops the chunks the log let go.
///
/// Called before the project is saved. A crash in between leaves a line for
/// a change that was not kept, never a kept change without its line.
pub async fn save_audit(data_dir: &Path, room: &str, writes: &Writes) -> std::io::Result<()> {
    tokio::fs::create_dir_all(audit_dir(data_dir, room)).await?;
    let path = chunk_path(data_dir, room, writes.put.no);
    let temporary = path.with_extension("json.tmp");
    let serialised =
        serde_json::to_string(&writes.put).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    tokio::fs::write(&temporary, serialised).await?;
    tokio::fs::rename(&temporary, &path).await?;
    for no in &writes.delete {
        // already gone is fine
        let _ = tokio::fs::remove_file(chunk_path(data_dir, room, *no)).await;
    }
    Ok(())
}

// #endregion audit log =========================================================
