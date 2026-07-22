//! Step 5: Select merge candidates from evidence lanes.

use super::*;

use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Max pages in any merge candidate (exact-keyword groups and high-sim pairs).
const MAX_CANDIDATE_PAGES: usize = 4;

/// Pairwise cosine similarity required to emit a high-sim merge candidate.
/// Soft TF-IDF clustering in `build_context` still uses 0.15 — do not raise that.
const PAIR_CANDIDATE_SIMILARITY_THRESHOLD: f64 = 0.45;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Select Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic candidate selection for the cannibalization shortlist.
///
/// Soft TF-IDF clusters (from `cannibalization_clusters.json`) are exploratory
/// only — not merge authority. Emits candidates from two evidence lanes only:
/// 1. Exact same `target_keyword` groups via `exact_keyword_duplicates.json`
///    (`candidate_type: "exact_keyword_dupe"`) — single source of truth.
/// 2. High pairwise similarity pairs (≥ [`PAIR_CANDIDATE_SIMILARITY_THRESHOLD`])
///    as `merge_candidate` with `pair_similarity`.
///
/// Caps pages per candidate at [`MAX_CANDIDATE_PAGES`]. Writes
/// `cannibalization_candidates.json`.
pub(crate) fn exec_can_select_candidates(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load clusters from DB (primary) or JSON fallback — used for early exit /
    // messaging counts only. Soft clusters are never merge authority.
    let clusters_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_latest_audit_artifact(
                    &conn,
                    &task.project_id,
                    "cannibalization_clusters",
                )
                .ok()
                .flatten()
            });
        match db_doc {
            Some(v) => v,
            None => {
                let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
                match crate::engine::exec::common::read_json(
                    &clusters_path,
                    "cannibalization_clusters.json",
                ) {
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
            artifact_key: None,
        };
    }

    let mut candidates: Vec<serde_json::Value> = Vec::new();
    // Track URL pairs already covered so high-sim emission can skip duplicates
    // of exact-keyword sets (exact lane runs first and takes priority).
    let mut covered_url_pairs: HashSet<(String, String)> = HashSet::new();

    // ── Lane 1: exact-keyword duplicates (single source of truth) ─────────────
    // Guaranteed overlap cases (identical non-empty target_keyword). Empty
    // keywords are skipped by exact_dupes generation and again here.
    inject_exact_keyword_dupe_candidates(&paths, &mut candidates, &mut covered_url_pairs);

    // ── Lane 2: high pairwise similarity ──────────────────────────────────────
    // Soft clustering (threshold 0.15) is exploratory only. Merge candidates
    // require strong pair evidence (≥ 0.45). Emit one 2-page candidate per pair;
    // skip pairs already covered by an exact-keyword set.
    emit_high_similarity_pair_candidates(
        &paths,
        &mut candidates,
        &mut covered_url_pairs,
    );

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

    let message = if candidates.is_empty() {
        format!(
            "Selected 0 merge candidates from {} clusters — no cannibalization evidence (exact keyword groups or high pairwise similarity).",
            clusters_len
        )
    } else {
        format!(
            "Selected {} merge candidates from {} clusters",
            candidates.len(),
            clusters_len
        )
    };

    StepResult {
        success: true,
        message,
        output: Some(serde_json::to_string_pretty(&candidates_doc).unwrap_or_default()),
        artifact_key: None,
    }
}

/// Inject `exact_keyword_duplicates.json` groups as typed `exact_keyword_dupe`
/// candidates. Empty keywords and groups with fewer than 2 pages are skipped.
fn inject_exact_keyword_dupe_candidates(
    paths: &ProjectPaths,
    candidates: &mut Vec<serde_json::Value>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) {
    let dupes_path = paths.automation_dir.join("exact_keyword_duplicates.json");
    let dupes_json = match std::fs::read_to_string(&dupes_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let dupes_doc: serde_json::Value = match serde_json::from_str(&dupes_json) {
        Ok(v) => v,
        Err(_) => return,
    };
    let Some(dupes_arr) = dupes_doc["duplicates"].as_array() else {
        return;
    };

    for dupe in dupes_arr {
        let keyword = dupe["keyword"].as_str().unwrap_or("").trim().to_string();
        if keyword.is_empty() {
            continue;
        }
        let pages = dupe["pages"].as_array().cloned().unwrap_or_default();
        if pages.len() < 2 {
            continue;
        }

        // Cap exact-keyword groups at MAX_CANDIDATE_PAGES by impressions
        let mut pages = pages;
        pages.sort_by(|a, b| {
            let ia = b["gsc"]["impressions"].as_f64().unwrap_or(0.0);
            let ib = a["gsc"]["impressions"].as_f64().unwrap_or(0.0);
            ia.partial_cmp(&ib).unwrap_or(std::cmp::Ordering::Equal)
        });
        let pages: Vec<serde_json::Value> = pages.into_iter().take(MAX_CANDIDATE_PAGES).collect();

        let compact_pages: Vec<serde_json::Value> =
            pages.iter().map(compact_context_article).collect();

        record_url_pairs(&compact_pages, covered_url_pairs);

        let total_impressions: f64 = compact_pages
            .iter()
            .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
            .sum();
        let page_count = compact_pages.len();

        let candidate_id = format!("exact_{}_{}", slugify(&keyword), candidates.len());

        candidates.push(serde_json::json!({
            "candidate_id": candidate_id,
            "candidate_type": "exact_keyword_dupe",
            "theme": keyword,
            "pages": compact_pages,
            "top_shared_queries": vec![keyword.clone()],
            "shared_query_count": 1,
            "total_impressions": total_impressions,
            "page_count": page_count,
            "best_performer": dupe["best_performer"],
        }));
    }
}

fn normalize_url_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

fn record_url_pairs(pages: &[serde_json::Value], covered: &mut HashSet<(String, String)>) {
    let urls: Vec<String> = pages
        .iter()
        .filter_map(|p| {
            let u = p["url"].as_str().unwrap_or("").trim();
            if u.is_empty() {
                None
            } else {
                Some(u.to_string())
            }
        })
        .collect();
    for i in 0..urls.len() {
        for j in (i + 1)..urls.len() {
            covered.insert(normalize_url_pair(&urls[i], &urls[j]));
        }
    }
}

/// Emit merge candidates from high-similarity pairs in the audit context.
fn emit_high_similarity_pair_candidates(
    paths: &ProjectPaths,
    candidates: &mut Vec<serde_json::Value>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) {
    let context_path = paths
        .automation_dir
        .join("cannibalization_audit_context.json");
    let context_doc: serde_json::Value = match std::fs::read_to_string(&context_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(v) => v,
        None => return,
    };

    let pairs = match context_doc["similarity_pairs"].as_array() {
        Some(arr) => arr,
        None => return,
    };

    // Index articles by id for compact page construction
    let mut articles_by_id: HashMap<i64, &serde_json::Value> = HashMap::new();
    if let Some(articles) = context_doc["articles"].as_array() {
        for a in articles {
            if let Some(id) = a["id"].as_i64() {
                articles_by_id.insert(id, a);
            }
        }
    }
    if articles_by_id.is_empty() {
        return;
    }

    for pair in pairs {
        let sim = pair["similarity"].as_f64().unwrap_or(0.0);
        if sim < PAIR_CANDIDATE_SIMILARITY_THRESHOLD {
            continue;
        }
        let id_a = match pair["article_a_id"].as_i64() {
            Some(id) => id,
            None => continue,
        };
        let id_b = match pair["article_b_id"].as_i64() {
            Some(id) => id,
            None => continue,
        };
        if id_a == id_b {
            continue;
        }
        let article_a = match articles_by_id.get(&id_a) {
            Some(a) => *a,
            None => continue,
        };
        let article_b = match articles_by_id.get(&id_b) {
            Some(a) => *a,
            None => continue,
        };

        let page_a = compact_context_article(article_a);
        let page_b = compact_context_article(article_b);
        let url_a = page_a["url"].as_str().unwrap_or("").to_string();
        let url_b = page_b["url"].as_str().unwrap_or("").to_string();
        if url_a.is_empty() || url_b.is_empty() {
            continue;
        }

        let key = normalize_url_pair(&url_a, &url_b);
        if covered_url_pairs.contains(&key) {
            continue;
        }
        covered_url_pairs.insert(key);

        let selected_pages = vec![page_a, page_b];
        let total_impressions: f64 = selected_pages
            .iter()
            .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
            .sum();

        // Theme: prefer shared non-empty keyword, else first title fragment
        let kw_a = selected_pages[0]["target_keyword"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let kw_b = selected_pages[1]["target_keyword"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let theme = if !kw_a.is_empty() && kw_a == kw_b {
            kw_a
        } else if !kw_a.is_empty() {
            kw_a
        } else if !kw_b.is_empty() {
            kw_b
        } else {
            selected_pages[0]["title"]
                .as_str()
                .unwrap_or("high_similarity")
                .to_lowercase()
        };

        let candidate_id = format!("highsim_{}_{}", slugify(&theme), candidates.len());

        candidates.push(serde_json::json!({
            "candidate_id": candidate_id,
            "candidate_type": "merge_candidate",
            "theme": theme,
            "pages": selected_pages,
            "top_shared_queries": [],
            "shared_query_count": 0,
            "total_impressions": total_impressions,
            "page_count": 2,
            "pair_similarity": sim,
        }));
    }
}

/// Compact a context-shaped article (url_slug + nested gsc) into a candidate page.
fn compact_context_article(article: &serde_json::Value) -> serde_json::Value {
    let slug = article["url_slug"].as_str().unwrap_or("");
    let excerpt = article["first_200_words"].as_str().unwrap_or("");
    let excerpt_words: Vec<&str> = excerpt.split_whitespace().take(60).collect();
    serde_json::json!({
        "id": article["id"],
        "url": crate::content::slug::format_blog_link(slug),
        "title": article["title"],
        "h1": article["h1"],
        "target_keyword": article["target_keyword"],
        "impressions": article["gsc"]["impressions"].as_f64().unwrap_or(0.0),
        "clicks": article["gsc"]["clicks"].as_f64().unwrap_or(0.0),
        "avg_position": article["gsc"]["avg_position"].as_f64().unwrap_or(0.0),
        "word_count": article["word_count"],
        "incoming_internal_links": article["incoming_internal_links"],
        "outgoing_internal_links": article["outgoing_internal_links"],
        "published_date": article["published_date"],
        "excerpt": excerpt_words.join(" "),
    })
}
