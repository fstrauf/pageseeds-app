//! Step 4: Detect exact keyword duplicates.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Exact Keyword Duplicates
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic detection of exact duplicate target keywords.
///
/// Reads `cannibalization_audit_context.json`, groups articles by identical
/// target_keyword, enriches each group with GSC performance ranking, and writes
/// `exact_keyword_duplicates.json`. These are guaranteed merge candidates — the
/// agent only decides which page to keep and how to redirect.
pub(crate) fn exec_can_exact_keyword_dupes(_task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let context_path = paths
        .automation_dir
        .join("cannibalization_audit_context.json");

    let context_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &context_path,
        "cannibalization_audit_context.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let articles = context_doc["articles"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if articles.is_empty() {
        return StepResult {
            success: true,
            message: "No articles found — nothing to check for exact duplicates.".to_string(),
            output: None,
            artifact_key: None,
        };
    }

    // Group by exact target_keyword (trimmed, lowercase)
    let mut groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for article in &articles {
        let kw = article["target_keyword"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if kw.is_empty() {
            continue;
        }
        groups.entry(kw).or_default().push(article.clone());
    }

    let mut dupes: Vec<serde_json::Value> = Vec::new();
    for (kw, mut pages) in groups {
        if pages.len() < 2 {
            continue;
        }

        // Sort by GSC performance: impressions desc, clicks desc, position asc
        pages.sort_by(|a, b| {
            let ia = a["gsc"]["impressions"].as_f64().unwrap_or(0.0);
            let ib = b["gsc"]["impressions"].as_f64().unwrap_or(0.0);
            let ca = a["gsc"]["clicks"].as_f64().unwrap_or(0.0);
            let cb = b["gsc"]["clicks"].as_f64().unwrap_or(0.0);
            let pa = a["gsc"]["avg_position"].as_f64().unwrap_or(999.0);
            let pb = b["gsc"]["avg_position"].as_f64().unwrap_or(999.0);

            ib.partial_cmp(&ia)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal))
        });

        let total_impressions: f64 = pages
            .iter()
            .map(|p| p["gsc"]["impressions"].as_f64().unwrap_or(0.0))
            .sum();

        dupes.push(serde_json::json!({
            "keyword": kw,
            "article_count": pages.len(),
            "total_impressions": total_impressions,
            "pages": pages,
            "best_performer": {
                "id": pages[0]["id"],
                "title": pages[0]["title"],
                "url": pages[0]["url_slug"],
                "impressions": pages[0]["gsc"]["impressions"].as_f64().unwrap_or(0.0),
                "clicks": pages[0]["gsc"]["clicks"].as_f64().unwrap_or(0.0),
                "avg_position": pages[0]["gsc"]["avg_position"].as_f64().unwrap_or(0.0),
            },
        }));
    }

    // Sort by total impressions descending
    dupes.sort_by(|a, b| {
        let ta = a["total_impressions"].as_f64().unwrap_or(0.0);
        let tb = b["total_impressions"].as_f64().unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });

    let dupes_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "dupe_count": dupes.len(),
        "duplicates": dupes,
    });

    let dupes_path = paths.automation_dir.join("exact_keyword_duplicates.json");
    if let Err(e) = std::fs::write(
        &dupes_path,
        serde_json::to_string_pretty(&dupes_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write exact_keyword_duplicates.json: {}",
            e
        );
    }

    StepResult {
        success: true,
        message: format!("Found {} exact keyword duplicates", dupes.len()),
        output: Some(serde_json::to_string_pretty(&dupes_doc).unwrap_or_default()),
        artifact_key: None,
    }
}
