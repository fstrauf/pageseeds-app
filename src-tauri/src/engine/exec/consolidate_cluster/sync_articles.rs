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
            return StepResult {
                success: false,
                message: format!("Failed to open DB for sync: {}", e),
                output: None,
            };
        }
    };

    let depublished = match depublish_redirect_sources(task, project_path, &conn) {
        Ok(n) => n,
        Err(e) => {
            return StepResult {
                success: false,
                message: e,
                output: None,
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
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Failed to sync merged articles: {}", e),
            output: None,
        },
    }
}

/// Depublish every redirect source from the merge plan.
///
/// For each slug in the plan's `redirect_urls` (the same source
/// `merge_validate_output` uses):
///   1. MDX frontmatter `status` is set to `redirected` via
///      `frontmatter::replace_scalar` (file stays on disk).
///   2. The matching SQLite `articles` row is set to `status = 'redirected'`.
///
/// Frontmatter is patched first because `sync_article_metadata_from_disk`
/// copies frontmatter status into the DB — both must agree or a later sync
/// would resurrect `published`. Any failure (missing file, missing DB row,
/// ambiguous slug) fails the step loudly rather than leaving a zombie page.
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
    let redirect_urls: Vec<&str> = plan["redirect_urls"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if redirect_urls.is_empty() {
        return Ok(0);
    }

    let articles = crate::engine::task_store::list_articles(conn, &task.project_id)
        .map_err(|e| format!("Failed to list articles for depublish: {}", e))?;

    let mut depublished = 0usize;
    for url in redirect_urls {
        let slug = crate::content::slug::normalize_url_slug(url);
        if slug.is_empty() || slug == keeper_slug {
            continue;
        }

        // 1. Frontmatter status → redirected.
        let file = find_file_by_slug(project_path, &slug)?
            .ok_or_else(|| format!("Cannot depublish '{}': no content file matches", slug))?;
        let content = std::fs::read_to_string(&file)
            .map_err(|e| format!("Cannot depublish '{}': read failed: {}", slug, e))?;
        let (fm, body) = crate::content::frontmatter::split_mdx(&content).ok_or_else(|| {
            format!(
                "Cannot depublish '{}': no frontmatter in {}",
                slug,
                file.display()
            )
        })?;
        let new_fm = crate::content::frontmatter::replace_scalar(fm, "status", "redirected");
        std::fs::write(&file, crate::content::cleaner::rebuild_mdx(&new_fm, body))
            .map_err(|e| format!("Cannot depublish '{}': write failed: {}", slug, e))?;

        // 2. SQLite articles row → redirected.
        let article = articles
            .iter()
            .find(|a| {
                a.url_slug == slug
                    || crate::content::slug::normalize_url_slug(&a.url_slug) == slug
            })
            .ok_or_else(|| {
                format!("Cannot depublish '{}': no articles row matches the slug", slug)
            })?;
        conn.execute(
            "UPDATE articles SET status = 'redirected' WHERE id = ?1 AND project_id = ?2",
            rusqlite::params![article.id, task.project_id],
        )
        .map_err(|e| format!("Cannot depublish '{}': DB update failed: {}", slug, e))?;

        depublished += 1;
    }

    Ok(depublished)
}

