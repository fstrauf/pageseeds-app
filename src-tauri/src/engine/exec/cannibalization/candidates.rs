//! Step 5: Select merge candidates from clusters.

use super::*;

use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Max pages in any merge candidate (exact-keyword groups and high-sim components).
const MAX_CANDIDATE_PAGES: usize = 4;

/// Pairwise cosine similarity required to emit a high-sim merge candidate.
/// Soft TF-IDF clustering in `build_context` still uses 0.15 — do not raise that.
const PAIR_CANDIDATE_SIMILARITY_THRESHOLD: f64 = 0.45;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Select Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic candidate selection from cannibalization cluster artifacts.
///
/// Reads `cannibalization_clusters.json` and optional `similarity_pairs` from
/// `cannibalization_audit_context.json`. Emits candidates only from evidence
/// lanes: exact same `target_keyword` groups (≥2 pages) and high pairwise
/// similarity (≥ [`PAIR_CANDIDATE_SIMILARITY_THRESHOLD`]). Soft TF-IDF theme
/// bags are not merge authority — size-1 keyword groups are dropped, never
/// re-expanded into a whole-theme grab-bag. Caps pages per candidate at
/// [`MAX_CANDIDATE_PAGES`]. Writes `cannibalization_candidates.json`.
pub(crate) fn exec_can_select_candidates(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load clusters from DB (primary) or JSON fallback
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

    // Build candidates from clusters, but merge clusters that share the same theme.
    // Multiple connected components can have the same theme (e.g., 11 separate
    // 2-page clusters all about "iron condor"). Without merging, the agent analyzes
    // each one separately and may return duplicate-looking recommendations.
    //
    // Fail-closed: theme bags are only a source of pages. We emit a candidate
    // only when ≥2 pages share an exact target_keyword. Soft topical cohesion
    // alone never becomes a merge set.
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
    // Track URL pairs already covered so high-sim emission can skip duplicates.
    let mut covered_url_pairs: HashSet<(String, String)> = HashSet::new();

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
            let mut seen_urls = HashSet::new();
            all_pages.retain(|p| {
                let url = p["url"].as_str().unwrap_or("").to_string();
                if url.is_empty() {
                    return false;
                }
                seen_urls.insert(url)
            });
        }

        // Split by target keyword — only groups with ≥2 pages are merge evidence.
        // Size-1 groups are dropped. Never fall back to the whole soft-theme bag.
        let mut keyword_groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
        for page in &all_pages {
            let kw = page["target_keyword"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            keyword_groups.entry(kw).or_default().push(page.clone());
        }

        let mut groups: Vec<Vec<serde_json::Value>> = keyword_groups
            .into_values()
            .filter(|g| g.len() >= 2)
            .collect();
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

        for group in groups {
            // Cap at MAX_CANDIDATE_PAGES by impressions
            let mut group_pages = group;
            group_pages.sort_by(|a, b| {
                let ia = b["impressions"].as_f64().unwrap_or(0.0);
                let ib = a["impressions"].as_f64().unwrap_or(0.0);
                ia.partial_cmp(&ib).unwrap_or(std::cmp::Ordering::Equal)
            });
            let selected_pages: Vec<serde_json::Value> = group_pages
                .into_iter()
                .take(MAX_CANDIDATE_PAGES)
                .collect();

            record_url_pairs(&selected_pages, &mut covered_url_pairs);

            let total_impressions: f64 = selected_pages
                .iter()
                .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
                .sum();

            let candidate_id = format!("{}_{}", slugify(&theme), candidates.len());
            let page_count = selected_pages.len();
            let compact_pages = compact_cluster_pages(&selected_pages);

            candidates.push(serde_json::json!({
                "candidate_id": candidate_id,
                "candidate_type": "merge_candidate",
                "theme": theme,
                "pages": compact_pages,
                "top_shared_queries": top_shared_queries,
                "shared_query_count": shared_query_count,
                "total_impressions": total_impressions,
                "page_count": page_count,
            }));
        }
    }

    // ── High pairwise similarity candidates ───────────────────────────────────
    // Soft clustering (threshold 0.15) is exploratory only. Merge candidates
    // require strong pair evidence (≥ 0.45). Emit one 2-page candidate per pair;
    // never expand into top-N traffic samples from large components.
    emit_high_similarity_pair_candidates(
        &paths,
        &mut candidates,
        &mut covered_url_pairs,
    );

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

                    // Cap exact-keyword groups at MAX_CANDIDATE_PAGES as well
                    let mut pages = pages;
                    pages.sort_by(|a, b| {
                        let ia = b["gsc"]["impressions"].as_f64().unwrap_or(0.0);
                        let ib = a["gsc"]["impressions"].as_f64().unwrap_or(0.0);
                        ia.partial_cmp(&ib).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let pages: Vec<serde_json::Value> =
                        pages.into_iter().take(MAX_CANDIDATE_PAGES).collect();

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

                    record_url_pairs(&compact_pages, &mut covered_url_pairs);

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

/// Compact cluster-shaped pages into the candidate page payload.
fn compact_cluster_pages(pages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    pages
        .iter()
        .map(|p| {
            let excerpt = p["first_200_words"]
                .as_str()
                .or_else(|| p["excerpt"].as_str())
                .unwrap_or("");
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
        .collect()
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
