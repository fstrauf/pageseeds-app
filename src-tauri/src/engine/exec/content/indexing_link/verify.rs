use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ─── Step 4: Verify ───────────────────────────────────────────────────────────

/// Rescan the link graph and verify the target gained at least one inbound link.
pub(crate) fn exec_indexing_link_verify(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = std::path::Path::new(project_path);

    // Parse target artifact
    let target_data = match parse_target_artifact(task) {
        Some(t) => t,
        None => {
            return StepResult::fail("Missing or invalid indexing_link_target artifact".to_string())
        }
    };

    let target_article_id = target_data["article_id"].as_i64().unwrap_or(0);
    if target_article_id == 0 {
        return StepResult::fail("Target article_id is 0 — no matching article found in DB".to_string());
    }

    let target_slug = crate::content::slug::normalize_url_slug(target_data["slug"].as_str().unwrap_or(""));
    let incoming_before = target_data["incoming_link_count_before"]
        .as_u64()
        .unwrap_or(0) as usize;

    // Re-scan link graph
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to open DB for verification: {}", e))
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a
            .into_iter()
            .filter(|a| !a.file.is_empty())
            .collect::<Vec<_>>(),
        Err(e) => {
            return StepResult::fail(format!("Failed to load articles: {}", e))
        }
    };

    let content_dir = match crate::content::locator::resolve(repo_root, None).selected {
        Some(d) => d,
        None => {
            return StepResult::fail("Could not locate content directory for verification".to_string())
        }
    };

    let scan_result = match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(r) => r,
        Err(e) => {
            return StepResult::fail(format!("Link scan failed during verification: {}", e))
        }
    };

    // Update link_scan.json
    let scan_json = serde_json::to_string_pretty(&scan_result).unwrap_or_default();
    let scan_path = paths.automation_dir.join("link_scan.json");
    if let Err(e) = std::fs::write(&scan_path, &scan_json) {
        log::warn!(
            "[indexing_link_verify] failed to write link_scan.json: {}",
            e
        );
    }

    // Find target's new incoming count
    let target_profile = scan_result
        .profiles
        .iter()
        .find(|p| p.id == target_article_id);

    let incoming_after = target_profile.map(|p| p.incoming_ids.len()).unwrap_or(0);

    let links_added = if incoming_after > incoming_before {
        incoming_after - incoming_before
    } else {
        0
    };

    // Also check that at least one source file contains the target slug
    // Check source files modified by reading the link scan profiles
    // We need to find profiles that now have outgoing links to the target slug
    let source_files_modified: Vec<String> = scan_result
        .profiles
        .iter()
        .filter(|p| {
            // Re-read file content to check if it links to target
            // This is a simple heuristic: check if the file contains the target slug link
            let file_path = content_dir.join(&p.file);
            std::fs::read_to_string(&file_path)
                .ok()
                .map(|content| content.contains(&crate::content::slug::format_blog_link(&target_slug)))
                .unwrap_or(false)
        })
        .map(|p| p.file.clone())
        .collect();

    let passed = incoming_after > incoming_before;

    let verification = serde_json::json!({
        "target_article_id": target_article_id,
        "target_slug": target_slug,
        "incoming_link_count_before": incoming_before,
        "incoming_link_count_after": incoming_after,
        "links_added": links_added,
        "source_files_modified": source_files_modified,
        "passed": passed,
    });

    StepResult {
        success: passed,
        message: if passed {
            format!(
                "Verification passed: target {} gained {} inbound link(s) ({} → {})",
                target_slug, links_added, incoming_before, incoming_after
            )
        } else {
            format!(
                "Verification FAILED: target {} still has {} inbound link(s) (expected > {})",
                target_slug, incoming_after, incoming_before
            )
        },
        output: Some(verification.to_string()),
    }
}

