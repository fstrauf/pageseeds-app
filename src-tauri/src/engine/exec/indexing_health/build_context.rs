use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::indexing_health::{
    DistinctivenessVerdict, IndexingCampaignPlan, IndexingCampaignSummary, IndexingTargetContext,
    IndexingTargetPlan, PrerequisiteCheck, PrerequisiteReport, TargetDiagnosis,
};
use crate::models::task::Task;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Build Target Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Load drift report + cannibalization clusters + content audit,
/// and build per-target context objects for each not-indexed URL.
pub(crate) fn exec_ihc_build_target_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // 1. Load drift report
    let drift_path = paths.automation_dir.join("gsc_recovery_drift.json");
    let drift_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &drift_path,
        "gsc_recovery_drift.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let not_indexed = drift_doc["not_indexed"].as_array().cloned().unwrap_or_default();
    if not_indexed.is_empty() {
        return StepResult {
            success: true,
            message: "No not-indexed URLs found in drift report.".to_string(),
            output: Some("{\"targets\": []}".to_string()),
        };
    }

    // 2. Load cannibalization clusters (DB primary, JSON fallback)
    let clusters_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_latest_audit_artifact(&conn, &task.project_id, "cannibalization_clusters").ok().flatten()
            });
        db_doc.unwrap_or_else(|| {
            let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
            std::fs::read_to_string(&clusters_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({ "clusters": [] }))
        })
    };
    let clusters = clusters_doc["clusters"].as_array().cloned().unwrap_or_default();

    // Build a map from article URL (normalized) → cluster
    let mut url_to_cluster: HashMap<String, &serde_json::Value> = HashMap::new();
    for cluster in &clusters {
        if let Some(pages) = cluster["pages"].as_array() {
            for page in pages {
                if let Some(url) = page["url"].as_str() {
                    let norm = normalize_url(url);
                    url_to_cluster.insert(norm, cluster);
                }
            }
        }
    }

    // 3. Load content audit + articles via canonical helpers (Stage B)
    let audit = crate::engine::exec::common::load_audit_snapshot(&task.project_id, &paths);
    let audit_by_slug = &audit.by_slug;

    let project_articles = crate::engine::exec::common::load_project_articles(&paths);
    let article_by_slug = &project_articles.by_slug;

    // 3c. Load link_scan.json for outgoing-link checks when building source candidates
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_scan: serde_json::Value = std::fs::read_to_string(&link_scan_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "profiles": [] }));
    let profiles = link_scan["profiles"].as_array().cloned().unwrap_or_default();
    let outgoing_by_id: HashMap<i64, HashSet<i64>> = profiles
        .iter()
        .filter_map(|p| {
            let id = p["id"].as_i64()?;
            let outgoing: HashSet<i64> = p["outgoing_ids"]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_i64())
                .collect();
            Some((id, outgoing))
        })
        .collect();

    // 4. Build target contexts
    let mut targets: Vec<IndexingTargetContext> = Vec::new();

    for item in &not_indexed {
        let url = item["url"].as_str().unwrap_or("").to_string();
        let slug = item["slug"].as_str().unwrap_or("").to_string();
        let reason_code = item["reason_code"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        if url.is_empty() {
            continue;
        }

        // Find cluster match
        let cluster = url_to_cluster.get(&normalize_url(&url)).cloned();

        let (sibling_count, siblings, shared_headings) = cluster
            .map(|c| {
                let pages = c["pages"].as_array().cloned().unwrap_or_default();
                let sibs: Vec<crate::models::indexing_health::SiblingArticle> = pages
                    .iter()
                    .filter(|p| {
                        p["url"]
                            .as_str()
                            .map(|u| normalize_url(u) != normalize_url(&url))
                            .unwrap_or(true)
                    })
                    .map(|p| crate::models::indexing_health::SiblingArticle {
                        url: p["url"].as_str().unwrap_or("").to_string(),
                        title: p["title"].as_str().unwrap_or("").to_string(),
                        h1: p["h1"].as_str().unwrap_or("").to_string(),
                        word_count: p["word_count"].as_u64().unwrap_or(0) as usize,
                        impressions: p["impressions"].as_f64(),
                    })
                    .collect();
                let headings: Option<Vec<String>> = c["shared_headings"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                (sibs.len(), sibs, headings)
            })
            .unwrap_or((0, vec![], None));

        // Content audit lookup
        let audit = audit_by_slug.get(&slug);
        let word_count = audit
            .and_then(|a| a["word_count"].as_u64())
            .unwrap_or(0) as usize;
        let incoming_links = audit
            .and_then(|a| a["checks"]["internal_links"]["value"].as_u64())
            .unwrap_or(0) as usize;
        let health = audit
            .and_then(|a| a["health"].as_str())
            .unwrap_or("unknown")
            .to_string();

        let title = audit
            .and_then(|a| a["title"].as_str())
            .unwrap_or("")
            .to_string();
        let h1 = audit
            .and_then(|a| a["checks"]["h1_keyword"]["value"].as_str())
            .unwrap_or("")
            .to_string();

        // Article lookup for id/file
        // Normalize drift slug (may have numeric prefix like "265-hub-coffee-beans")
        let normalized_slug = crate::content::slug::normalize_url_slug(&slug);
        let article = article_by_slug.get(&normalized_slug)
            .or_else(|| article_by_slug.get(&slug))
            .or_else(|| {
                // Fallback: extract normalized slug from the full URL
                let url_slug = crate::content::slug::extract_slug_from_url(&url);
                article_by_slug.get(&url_slug)
            });
        let article_id = article
            .and_then(|a| a["id"].as_i64())
            .unwrap_or(0);
        let file = article
            .and_then(|a| a["file"].as_str())
            .unwrap_or("")
            .to_string();
        let target_keyword = article
            .and_then(|a| a["target_keyword"].as_str())
            .unwrap_or("")
            .to_string();

        let diagnosis = TargetDiagnosis {
            has_links: incoming_links >= 1,
            is_long: word_count >= 600,
            has_cluster_siblings: sibling_count > 0,
            suspected_root_cause: if sibling_count > 0 {
                "cannibalization"
            } else if incoming_links == 0 {
                "insufficient_internal_links"
            } else if word_count < 600 {
                "thin_content"
            } else {
                "unknown"
            }
            .to_string(),
        };

        // Build source candidates for add_links targets. Also built for weakly
        // linked targets (< 2 incoming links) so fallback `fix_indexing`
        // targets mapped to `fix_indexing_internal_links` (spawn.rs) have real
        // candidates to work with instead of silently no-oping.
        let mut source_candidates: Vec<crate::models::indexing_health::LinkSourceCandidate> =
            Vec::new();
        if incoming_links < 2 && article_id > 0 {
            let target_outgoing = outgoing_by_id.get(&article_id);
            for (src_slug, src_art) in article_by_slug {
                if src_slug == &slug {
                    continue;
                }
                let src_id = src_art["id"].as_i64().unwrap_or(0);
                if src_id == 0 || src_id == article_id {
                    continue;
                }
                // Skip if already links to target
                let already_links = target_outgoing
                    .map(|out| out.contains(&src_id))
                    .unwrap_or(false);
                if already_links {
                    continue;
                }
                // Simple topical relevance: same cluster or shared keyword
                let src_kw = src_art["target_keyword"].as_str().unwrap_or("");
                let in_cluster = siblings.iter().any(|s| {
                    crate::content::slug::extract_slug_from_url(&s.url) == *src_slug
                });
                let shares_kw = !target_keyword.is_empty()
                    && !src_kw.is_empty()
                    && target_keyword.to_lowercase() == src_kw.to_lowercase();
                if in_cluster || shares_kw {
                    source_candidates.push(crate::models::indexing_health::LinkSourceCandidate {
                        article_id: src_id,
                        slug: src_slug.clone(),
                        title: src_art["title"].as_str().unwrap_or("").to_string(),
                        file: src_art["file"].as_str().unwrap_or("").to_string(),
                        reason: if in_cluster {
                            "cluster sibling".to_string()
                        } else {
                            "shared target keyword".to_string()
                        },
                    });
                }
            }
            // Limit to top 8 candidates
            source_candidates.truncate(8);
        }

        targets.push(IndexingTargetContext {
            target: crate::models::indexing_health::TargetArticleSummary {
                url,
                slug,
                reason_code,
                title,
                h1,
                target_keyword,
                word_count,
                incoming_links,
                content_audit_health: health,
                article_id,
                file,
            },
            cluster: if sibling_count > 0 {
                Some(crate::models::indexing_health::ClusterContext {
                    cluster_id: cluster
                        .and_then(|c| c["cluster_id"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    theme: cluster
                        .and_then(|c| c["theme"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    sibling_count,
                    siblings,
                    shared_headings,
                    exact_keyword_dupe: false, // populated by reduce step if needed
                })
            } else {
                None
            },
            diagnosis,
            source_candidates,
        });
    }

    // 5. Write contexts to disk
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "project_id": &task.project_id,
        "targets": targets,
    });
    let contexts_json = match serde_json::to_string_pretty(&contexts_doc) {
        Ok(j) => j,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize target contexts: {}", e))
        }
    };
    if let Err(e) = std::fs::write(&contexts_path, &contexts_json) {
        return StepResult::fail(format!("Failed to write target contexts: {}", e));
    }

    StepResult {
        success: true,
        message: format!(
            "Built context for {} not-indexed URL(s), {} with cluster siblings",
            targets.len(),
            targets.iter().filter(|t| t.diagnosis.has_cluster_siblings).count()
        ),
        output: Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "target_count": targets.len(),
                "with_cluster_siblings": targets.iter().filter(|t| t.diagnosis.has_cluster_siblings).count(),
                "contexts_path": contexts_path.display().to_string(),
            }))
            .unwrap_or_default(),
        ),
    }
}

fn normalize_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_lowercase()
}
