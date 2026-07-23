//! Keeping a project on disk.
//!
//! The Worker cannot do this at all — it is one of the reasons this binary
//! exists. One JSON file per room, written whole.

use std::path::{Path, PathBuf};

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
