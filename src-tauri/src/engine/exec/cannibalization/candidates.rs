//! Step 5: Select merge candidates from clusters.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Select Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic candidate selection from cannibalization cluster artifacts.
///
/// Reads `cannibalization_clusters.json`, scores clusters, splits giant components
/// by target keyword, caps pages per candidate at 8, and writes
/// `cannibalization_candidates.json`.
pub(crate) fn exec_can_select_candidates(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load clusters from DB (primary) or JSON fallback
    let clusters_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_latest_audit_artifact(&conn, &task.project_id, "cannibalization_clusters").ok().flatten()
            });
        match db_doc {
            Some(v) => v,
            None => {
                let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
                match crate::engine::exec::common::read_json(&clusters_path, "cannibalization_clusters.json") {
                    Ok(v) => v,
                    Err(e) => return e,
                }
            }
        }
    };

    let clusters = clusters_doc["clusters"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let clusters_len = clusters.len();
    if clusters.is_empty() {
        return StepResult {
            success: true,
            message: "No clusters found — nothing to select.".to_string(),
            output: None,
        };
    }

    // Build candidates from clusters, but merge clusters that share the same theme.
    // Multiple connected components can have the same theme (e.g., 11 separate
    // 2-page clusters all about "iron condor"). Without merging, the agent analyzes
    // each one separately and may return duplicate-looking recommendations.
    let mut theme_groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for cluster in &clusters {
        let pages = cluster["pages"].as_array().cloned().unwrap_or_default();
        if pages.len() < 2 {
            continue;
        }
        let theme = cluster["theme"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if theme.is_empty() {
            continue;
        }
        theme_groups.entry(theme).or_default().push(cluster.clone());
    }

    let mut candidates: Vec<serde_json::Value> = Vec::new();

    for (theme, group_clusters) in theme_groups {
        // Collect all pages from all clusters with this theme
        let mut all_pages: Vec<serde_json::Value> = Vec::new();
        let mut top_shared_queries: Vec<String> = Vec::new();
        let mut shared_query_count: i64 = 0;
        for cluster in &group_clusters {
            if let Some(pages) = cluster["pages"].as_array() {
                all_pages.extend(pages.clone());
            }
            if let Some(arr) = cluster["top_shared_queries"].as_array() {
                for q in arr {
                    if let Some(s) = q.as_str() {
                        if !top_shared_queries.contains(&s.to_string()) {
                            top_shared_queries.push(s.to_string());
                        }
                    }
                }
            }
            shared_query_count =
                shared_query_count.max(cluster["shared_query_count"].as_i64().unwrap_or(0));
        }

        if all_pages.len() < 2 {
            continue;
        }

        // Deduplicate pages by URL
        {
            let mut seen_urls = std::collections::HashSet::new();
            all_pages.retain(|p| {
                let url = p["url"].as_str().unwrap_or("").to_string();
                if url.is_empty() {
                    return false;
                }
                seen_urls.insert(url)
            });
        }

        // Split by target keyword if the merged group is large
        let mut keyword_groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
        for page in &all_pages {
            let kw = page["target_keyword"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            keyword_groups.entry(kw).or_default().push(page.clone());
        }

        let groups_to_process: Vec<Vec<serde_json::Value>> = if keyword_groups.len() == 1
            && all_pages.len() <= 8
        {
            vec![all_pages.clone()]
        } else {
            let mut groups: Vec<Vec<serde_json::Value>> = keyword_groups.into_values().collect();
            groups.sort_by(|a, b| {
                let ia: f64 = a
                    .iter()
                    .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
                    .sum();
                let ib: f64 = b
                    .iter()
                    .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
                    .sum();
                ib.partial_cmp(&ia).unwrap_or(std::cmp::Ordering::Equal)
            });
            groups
        };

        for group in groups_to_process {
            if group.len() < 2 {
                continue;
            }

            // Cap at 8 pages by impressions
            let mut group_pages = group;
            group_pages.sort_by(|a, b| {
                let ia = b["impressions"].as_f64().unwrap_or(0.0);
                let ib = a["impressions"].as_f64().unwrap_or(0.0);
                ia.partial_cmp(&ib).unwrap_or(std::cmp::Ordering::Equal)
            });
            let selected_pages: Vec<serde_json::Value> = group_pages.into_iter().take(8).collect();

            let total_impressions: f64 = selected_pages
                .iter()
                .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
                .sum();

            let candidate_id = format!("{}_{}", slugify(&theme), candidates.len());

            let compact_pages: Vec<serde_json::Value> = selected_pages
                .iter()
                .map(|p| {
                    let excerpt = p["first_200_words"].as_str().unwrap_or("");
                    let excerpt_words: Vec<&str> = excerpt.split_whitespace().take(60).collect();
                    serde_json::json!({
                        "id": p["id"],
                        "url": p["url"],
                        "title": p["title"],
                        "h1": p["h1"],
                        "target_keyword": p["target_keyword"],
                        "impressions": p["impressions"],
                        "clicks": p["clicks"],
                        "avg_position": p["avg_position"],
                        "word_count": p["word_count"],
                        "incoming_internal_links": p["incoming_internal_links"],
                        "outgoing_internal_links": p["outgoing_internal_links"],
                        "published_date": p["published_date"],
                        "excerpt": excerpt_words.join(" "),
                    })
                })
                .collect();

            candidates.push(serde_json::json!({
                "candidate_id": candidate_id,
                "candidate_type": "merge_candidate",
                "theme": theme,
                "pages": compact_pages,
                "top_shared_queries": top_shared_queries,
                "shared_query_count": shared_query_count,
                "total_impressions": total_impressions,
                "page_count": compact_pages.len(),
            }));
        }
    }

    // ── Inject exact-keyword-duplicate candidates ─────────────────────────────
    // These are guaranteed overlap cases (identical target_keyword). They take
    // priority over cluster-based candidates because the overlap is unambiguous.
    let dupes_path = paths.automation_dir.join("exact_keyword_duplicates.json");
    if let Ok(dupes_json) = std::fs::read_to_string(&dupes_path) {
        if let Ok(dupes_doc) = serde_json::from_str::<serde_json::Value>(&dupes_json) {
            if let Some(dupes_arr) = dupes_doc["duplicates"].as_array() {
                for dupe in dupes_arr {
                    let keyword = dupe["keyword"].as_str().unwrap_or("").to_string();
                    if keyword.is_empty() {
                        continue;
                    }
                    let pages = dupe["pages"].as_array().cloned().unwrap_or_default();
                    if pages.len() < 2 {
                        continue;
                    }

                    // Build compact pages in the same shape as cluster candidates
                    let compact_pages: Vec<serde_json::Value> = pages
                        .iter()
                        .map(|p| {
                            let excerpt = p["first_200_words"].as_str().unwrap_or("");
                            let excerpt_words: Vec<&str> =
                                excerpt.split_whitespace().take(60).collect();
                            serde_json::json!({
                                "id": p["id"],
                                "url": crate::content::slug::format_blog_link(p["url_slug"].as_str().unwrap_or("")),
                                "title": p["title"],
                                "h1": p["h1"],
                                "target_keyword": p["target_keyword"],
                                "impressions": p["gsc"]["impressions"].as_f64().unwrap_or(0.0),
                                "clicks": p["gsc"]["clicks"].as_f64().unwrap_or(0.0),
                                "avg_position": p["gsc"]["avg_position"].as_f64().unwrap_or(0.0),
                                "word_count": p["word_count"],
                                "incoming_internal_links": p["incoming_internal_links"],
                                "outgoing_internal_links": p["outgoing_internal_links"],
                                "published_date": p["published_date"],
                                "excerpt": excerpt_words.join(" "),
                            })
                        })
                        .collect();

                    let total_impressions: f64 = compact_pages
                        .iter()
                        .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
                        .sum();

                    let candidate_id = format!("exact_{}_{}", slugify(&keyword), candidates.len());

                    candidates.push(serde_json::json!({
                        "candidate_id": candidate_id,
                        "candidate_type": "exact_keyword_dupe",
                        "theme": keyword,
                        "pages": compact_pages,
                        "top_shared_queries": vec![keyword.clone()],
                        "shared_query_count": 1,
                        "total_impressions": total_impressions,
                        "page_count": compact_pages.len(),
                        "best_performer": dupe["best_performer"],
                    }));
                }
            }
        }
    }

    // Sort candidates by total impressions descending
    candidates.sort_by(|a, b| {
        let ia = a["total_impressions"].as_f64().unwrap_or(0.0);
        let ib = b["total_impressions"].as_f64().unwrap_or(0.0);
        ib.partial_cmp(&ia).unwrap_or(std::cmp::Ordering::Equal)
    });

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let candidates_doc = serde_json::json!({
        "generated_at": &now_iso,
        "candidate_count": candidates.len(),
        "candidates": candidates,
    });

    // Save to database (new primary storage)
    if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let _ = crate::db::content_audit::save_audit_artifact(
            &db,
            &task.project_id,
            "cannibalization_candidates",
            &now_iso,
            &serde_json::to_string(&candidates_doc).unwrap_or_default(),
        );
    }

    // Also write candidates artifact to disk for downstream consumers
    let candidates_path = paths.automation_dir.join("cannibalization_candidates.json");
    if let Err(e) = std::fs::write(
        &candidates_path,
        serde_json::to_string_pretty(&candidates_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write cannibalization_candidates.json: {}",
            e
        );
    }

    StepResult {
        success: true,
        message: format!(
            "Selected {} merge candidates from {} clusters",
            candidates.len(),
            clusters_len
        ),
        output: Some(serde_json::to_string_pretty(&candidates_doc).unwrap_or_default()),
    }
}
