/// MDX file snapshot and rollback support.
///
/// Creates a `.bak` copy of any MDX file before it is modified,
/// and provides rollback capability for deterministic validation failures.
use std::path::{Path, PathBuf};

/// Snapshot metadata for a single file edit.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub original_path: PathBuf,
    pub backup_path: PathBuf,
    pub created_at: String,
}

/// Create a backup of an MDX file before editing.
///
/// The backup is written to the same directory with a `.bak.{timestamp}` suffix.
/// Returns the snapshot metadata on success.
pub fn snapshot_file(original_path: &Path) -> crate::error::Result<FileSnapshot> {
    if !original_path.exists() {
        return Err(crate::error::Error::Other(format!(
            "Cannot snapshot non-existent file: {}",
            original_path.display()
        )));
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let backup_name = format!(
        "{}.bak.{}",
        original_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown"),
        timestamp
    );
    let backup_path = original_path.with_file_name(&backup_name);

    std::fs::copy(original_path, &backup_path).map_err(|e| {
        crate::error::Error::Other(format!(
            "Failed to create snapshot from {} to {}: {}",
            original_path.display(),
            backup_path.display(),
            e
        ))
    })?;

    Ok(FileSnapshot {
        original_path: original_path.to_path_buf(),
        backup_path,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Rollback a file to its snapshot state.
///
/// Copies the backup back to the original path. The backup file is preserved
/// so multiple rollbacks are possible.
pub fn rollback_file(snapshot: &FileSnapshot) -> crate::error::Result<()> {
    if !snapshot.backup_path.exists() {
        return Err(crate::error::Error::Other(format!(
            "Snapshot backup missing: {}",
            snapshot.backup_path.display()
        )));
    }

    std::fs::copy(&snapshot.backup_path, &snapshot.original_path).map_err(|e| {
        crate::error::Error::Other(format!(
            "Failed to rollback {} from {}: {}",
            snapshot.original_path.display(),
            snapshot.backup_path.display(),
            e
        ))
    })?;

    Ok(())
}

/// Commit a snapshot by deleting the backup file.
///
/// Call this after deterministic validation passes.
pub fn commit_snapshot(snapshot: &FileSnapshot) -> crate::error::Result<()> {
    if snapshot.backup_path.exists() {
        std::fs::remove_file(&snapshot.backup_path).map_err(|e| {
            crate::error::Error::Other(format!(
                "Failed to remove snapshot backup {}: {}",
                snapshot.backup_path.display(),
                e
            ))
        })?;
    }
    Ok(())
}

/// Clean up old snapshot backups in a directory.
///
/// Removes `.bak.*` files older than the given duration.
pub fn cleanup_old_snapshots(
    dir: &Path,
    max_age: std::time::Duration,
) -> crate::error::Result<usize> {
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.contains(".bak.") {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(elapsed) = modified.elapsed() {
                                if elapsed > max_age {
                                    let _ = std::fs::remove_file(&path);
                                    removed += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(removed)
}
