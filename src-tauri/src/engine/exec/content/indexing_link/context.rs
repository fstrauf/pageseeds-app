use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ─── Step 1: Context ──────────────────────────────────────────────────────────

/// Build a compact per-target context artifact from the task's target data,
/// current link scan, and source file excerpts.
pub(crate) fn exec_indexing_link_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Parse target artifact
    let target_data = match parse_target_artifact(task) {
        Some(t) => t,
        None => {
            return StepResult {
                success: false,
                message: "Missing or invalid indexing_link_target artifact".to_string(),
                output: None,
            }
        }
    };

    // Check plan — if it had no links, there's nothing to verify
    let plan: serde_json::Value = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_link_plan")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str(c).ok())
        .unwrap_or_default();
    let planned_links = plan["links_to_add"].as_array().cloned().unwrap_or_default();
    if planned_links.is_empty() {
        return StepResult {
            success: true,
            message: "Nothing to verify — no links were planned for this target".to_string(),
            output: Some(serde_json::json!({
                "target_article_id": target_data["article_id"].as_i64().unwrap_or(0),
                "target_slug": target_data["slug"].as_str().unwrap_or(""),
                "planned_links": 0,
                "passed": true,
            }).to_string()),
        };
    }

    let target_article_id = target_data["article_id"].as_i64().unwrap_or(0);
    if target_article_id == 0 {
        return StepResult {
            success: false,
            message: "Target article_id is 0 — no matching article found in DB".to_string(),
            output: None,
        };
    }

    let target_slug = crate::content::slug::normalize_url_slug(target_data["slug"].as_str().unwrap_or(""));
    let target_keyword = target_data["target_keyword"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Load link scan — trigger fresh scan if missing or stale (>1 hour)
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_scan: Option<serde_json::Value> = {
        let stale = match std::fs::metadata(&link_scan_path) {
            Ok(m) => m
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs() > 3600)
                .unwrap_or(true),
            Err(_) => true,
        };
        let fresh_scan = if stale {
            log::info!("[indexing_link_context] link_scan.json missing or stale — triggering fresh scan");
            let repo_root = std::path::Path::new(project_path);
            if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
                if let Ok(articles) = crate::content::article_index::list_articles(&db, &task.project_id) {
                    let articles: Vec<_> = articles.into_iter().filter(|a| !a.file.is_empty()).collect();
                    if let Some(content_dir) = crate::content::locator::resolve(repo_root, None).selected {
                        if let Ok(scan_result) = crate::content::linking::scan_links(&content_dir, &articles) {
                            let scan_json = serde_json::to_string_pretty(&scan_result).unwrap_or_default();
                            let _ = std::fs::write(&link_scan_path, &scan_json);
                            serde_json::from_str(&scan_json).ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        fresh_scan.or_else(|| {
            std::fs::read_to_string(&link_scan_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
    };

    // Find target profile
    let target_profile = link_scan
        .as_ref()
        .and_then(|v| v["profiles"].as_array())
        .and_then(|profiles| {
            profiles
                .iter()
                .find(|p| p["id"].as_i64() == Some(target_article_id))
                .cloned()
        });

    let current_incoming_ids: Vec<i64> = target_profile
        .as_ref()
        .and_then(|p| p["incoming_ids"].as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let current_outgoing_ids: Vec<i64> = target_profile
        .as_ref()
        .and_then(|p| p["outgoing_ids"].as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    // Build source context from source_candidates in the artifact
    let source_candidates = target_data["source_candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut sources: Vec<serde_json::Value> = Vec::new();

    for candidate in &source_candidates {
        let source_id = candidate["article_id"].as_i64().unwrap_or(0);
        let source_slug = candidate["slug"].as_str().unwrap_or("").to_string();
        let source_file = candidate["file"].as_str().unwrap_or("").to_string();

        // Check if already links to target (outgoing_ids is Vec<i64>)
        let already_links = link_scan
            .as_ref()
            .and_then(|v| v["profiles"].as_array())
            .and_then(|profiles| {
                profiles
                    .iter()
                    .find(|p| p["id"].as_i64() == Some(source_id))
                    .and_then(|p| {
                        p["outgoing_ids"].as_array().map(|outgoing| {
                            outgoing
                                .iter()
                                .any(|o| o.as_i64() == Some(target_article_id))
                        })
                    })
            })
            .unwrap_or(false);

        sources.push(serde_json::json!({
            "article_id": source_id,
            "title": candidate["title"],
            "slug": source_slug,
            "file": source_file,
            "gsc_impressions": candidate["gsc_impressions"],
            "score": candidate["score"],
            "already_links_to_target": already_links,
        }));
    }

    let context = serde_json::json!({
        "target": {
            "article_id": target_article_id,
            "title": target_data["title"],
            "slug": target_slug,
            "url": target_data["url"],
            "target_keyword": target_keyword,
            "current_incoming_ids": current_incoming_ids,
            "current_outgoing_ids": current_outgoing_ids,
        },
        "sources": sources,
    });

    StepResult {
        success: true,
        message: format!(
            "Context built for target {}: {} incoming, {} source candidates",
            target_slug,
            current_incoming_ids.len(),
            sources.len()
        ),
        output: Some(context.to_string()),
    }
}

