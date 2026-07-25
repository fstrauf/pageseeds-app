use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 8: Sync Articles
// ═══════════════════════════════════════════════════════════════════════════════

/// Sync merged content back to SQLite and articles.json.
///
/// Before syncing, every redirect source from the merge plan is depublished:
/// its MDX frontmatter and its SQLite row are set to `status = "redirected"`.
/// The files stay on disk (recovery path preserved), but without this they
/// would remain `published` in articles.json forever — zombie pages that the
/// next cannibalization audit re-clusters and re-recommends for merging.
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

    let depublished = match depublish_redirect_sources(task, project_path, &conn) {
        Ok(n) => n,
        Err(e) => {
            return StepResult {
                success: false,
                message: e,
                output: None,
                artifact_key: None,
            };
        }
    };

    match crate::content::ops::sync_and_validate(
        &paths.automation_dir,
        repo_root,
        true, // apply_sync
        &conn,
        &task.project_id,
    ) {
        Ok(report) => {
            // Export SQLite state to articles.json so `status: "redirected"`
            // is reflected in the committed projection.
            if let Err(e) =
                crate::db::export::write_articles_to_repo(&conn, &task.project_id, repo_root)
            {
                return StepResult {
                    success: false,
                    message: format!("Failed to export articles.json after merge: {}", e),
                    output: None,
                    artifact_key: None,
                };
            }
            StepResult {
                success: true,
                message: format!(
                    "Synced {} checked entries, {} dates patched, {} redirect sources depublished",
                    report.checked_entries, report.dates_synced, depublished
                ),
                output: Some(
                    serde_json::json!({
                        "checked_entries": report.checked_entries,
                        "content_files": report.content_files,
                        "orphan_files": report.orphan_files,
                        "dates_synced": report.dates_synced,
                        "redirect_sources_depublished": depublished,
                    })
                    .to_string(),
                ),
                artifact_key: None,
            }
        }
        Err(e) => StepResult::fail(format!("Failed to sync merged articles: {}", e)),
    }
}

/// Depublish every redirect source from the merge plan.
///
/// Thin wrapper: loads the plan, then calls the shared
/// [`crate::engine::merge_apply::depublish_redirect_slugs`] primitive
/// (also used by Path B `submit_merge`).
///
/// Returns the number of depublished sources.
pub(crate) fn depublish_redirect_sources(
    task: &Task,
    project_path: &str,
    conn: &rusqlite::Connection,
) -> std::result::Result<usize, String> {
    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = serde_json::from_str(&plan_json)
        .map_err(|e| format!("Invalid merge plan JSON: {}", e))?;

    let keeper_slug =
        crate::content::slug::normalize_url_slug(plan["keep_url"].as_str().unwrap_or(""));
    let redirect_slugs: Vec<String> = plan["redirect_urls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| crate::content::slug::normalize_url_slug(s))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if redirect_slugs.is_empty() {
        return Ok(0);
    }

    crate::engine::merge_apply::depublish_redirect_slugs(
        conn,
        &task.project_id,
        std::path::Path::new(project_path),
        &keeper_slug,
        &redirect_slugs,
    )
}
