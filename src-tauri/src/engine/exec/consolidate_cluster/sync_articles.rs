use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 8: Sync Articles
// ═══════════════════════════════════════════════════════════════════════════════

/// Sync merged content back to SQLite and articles.json.
pub(crate) fn exec_merge_sync_articles(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = std::path::Path::new(project_path);

    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to open DB for sync: {}", e));
        }
    };

    match crate::content::ops::sync_and_validate(
        &paths.automation_dir,
        repo_root,
        true, // apply_sync
        &conn,
        &task.project_id,
    ) {
        Ok(report) => StepResult {
            success: true,
            message: format!(
                "Synced {} checked entries, {} dates patched",
                report.checked_entries, report.dates_synced
            ),
            output: Some(
                serde_json::json!({
                    "checked_entries": report.checked_entries,
                    "content_files": report.content_files,
                    "orphan_files": report.orphan_files,
                    "dates_synced": report.dates_synced,
                })
                .to_string(),
            ),
        },
        Err(e) => StepResult::fail(format!("Failed to sync merged articles: {}", e)),
    }
}

