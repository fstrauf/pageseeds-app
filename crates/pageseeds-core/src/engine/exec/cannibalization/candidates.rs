//! Step 5: Select merge candidates from evidence lanes.

use super::*;

use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Max pages in any merge candidate (exact-keyword groups, shared-query groups, near-dupe pairs).
const MAX_CANDIDATE_PAGES: usize = 4;

/// Pairwise TF-IDF cosine similarity required to emit a near_dupe candidate
/// when embedding neighbors are unavailable.
/// Soft TF-IDF clustering in `build_context` still uses 0.15 — do not raise that.
const PAIR_CANDIDATE_SIMILARITY_THRESHOLD: f64 = 0.45;

/// Cosine similarity floor for embedding-based near_dupe neighbors.
const EMBEDDING_NEAR_DUPE_MIN_SIMILARITY: f64 = 0.85;

/// Min impressions per page for a GSC query to count toward the shared_query lane.
const SHARED_QUERY_MIN_IMPRESSIONS: f64 = 10.0;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Select Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic candidate selection for the cannibalization shortlist.
///
/// Soft TF-IDF clusters (from `cannibalization_clusters.json`) are exploratory
/// only — not merge authority and **not** a precondition. Emits candidates from
/// three evidence lanes only:
/// 1. Exact same `target_keyword` groups via `exact_keyword_duplicates.json`
///    (`lane: ExactKeyword` → `candidate_type: "exact_keyword_dupe"`).
/// 2. Same GSC query on ≥2 articles via `ctr_query_metrics`
///    (`lane: SharedQuery` → `candidate_type: "shared_query"`).
/// 3. High pairwise similarity pairs / small cliques
///    (`lane: NearDupe` → `candidate_type: "near_dupe"`).
///
/// Caps pages per candidate at [`MAX_CANDIDATE_PAGES`]. Writes rich
/// `cannibalization_candidates.json` for analyze and ID-based
/// `cannibalization_evidence.json` matching the #117 shortlist shape.
pub(crate) fn exec_can_select_candidates(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Soft clusters are optional messaging only — never gate the shortlist.
    let clusters_len = load_soft_cluster_count(&paths, &task.project_id);

    // Optional shared DB connection for shared_query + embedding near_dupe lanes.
    let db_conn = rusqlite::Connection::open(crate::db::default_db_path()).ok();

    // Context articles (for shared_query / near_dupe page packaging).
    let context_doc = load_audit_context(&paths);
    let articles_by_id = index_context_articles(context_doc.as_ref());

    let mut candidates: Vec<MergeCandidate> = Vec::new();
    // Track URL pairs already covered so later lanes skip duplicates of earlier ones.
    // Priority order: exact_keyword → shared_query → near_dupe.
    let mut covered_url_pairs: HashSet<(String, String)> = HashSet::new();

    // ── Lane 1: exact-keyword duplicates (single source of truth) ─────────────
    inject_exact_keyword_dupe_candidates(&paths, &mut candidates, &mut covered_url_pairs);

    // ── Lane 2: shared GSC query (≥2 article_ids, min impression floor) ───────
    if let Some(ref conn) = db_conn {
        emit_shared_query_candidates(
            conn,
            &task.project_id,
            &articles_by_id,
            &mut candidates,
            &mut covered_url_pairs,
        );
    }

    // ── Lane 3: near_dupe (embedding neighbors preferred, TF-IDF pairs fallback)
    emit_near_dupe_candidates(
        &paths,
        db_conn.as_ref(),
        &task.project_id,
        &articles_by_id,
        context_doc.as_ref(),
        &mut candidates,
        &mut covered_url_pairs,
    );

    // Sort candidates by total impressions descending
    candidates.sort_by(|a, b| {
        b.total_impressions
            .partial_cmp(&a.total_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let rich_candidates: Vec<serde_json::Value> =
        candidates.iter().map(MergeCandidate::to_rich_json).collect();
    let candidates_doc = serde_json::json!({
        "generated_at": &now_iso,
        "candidate_count": rich_candidates.len(),
        "candidates": rich_candidates,
    });

    // #117 ID-based evidence shortlist (contract consumers).
    let evidence_doc = build_evidence_shortlist(&now_iso, &candidates);

    // Save to database (new primary storage)
    if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let _ = crate::db::content_audit::save_audit_artifact(
            &db,
            &task.project_id,
            "cannibalization_candidates",
            &now_iso,
            &serde_json::to_string(&candidates_doc).unwrap_or_default(),
        );
        let _ = crate::db::content_audit::save_audit_artifact(
            &db,
            &task.project_id,
            "cannibalization_evidence",
            &now_iso,
            &serde_json::to_string(&evidence_doc).unwrap_or_default(),
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

    let evidence_path = paths.automation_dir.join("cannibalization_evidence.json");
    if let Err(e) = std::fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&evidence_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write cannibalization_evidence.json: {}",
            e
        );
    }

    let message = if candidates.is_empty() {
        format!(
            "Selected 0 merge candidates from {} clusters — no cannibalization evidence (exact_keyword, shared_query, or near_dupe).",
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

/// Load soft TF-IDF cluster count for success messaging only.
/// Missing or empty clusters do not fail the step and do not block lanes.
fn load_soft_cluster_count(paths: &ProjectPaths, project_id: &str) -> usize {
    let clusters_doc: Option<serde_json::Value> = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_latest_audit_artifact(
                    &conn,
                    project_id,
                    "cannibalization_clusters",
                )
                .ok()
                .flatten()
            });
        match db_doc {
            Some(v) => Some(v),
            None => {
                let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
                std::fs::read_to_string(&clusters_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            }
        }
    };

    clusters_doc
        .as_ref()
        .and_then(|d| d["clusters"].as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

/// Build the #117 ID-based evidence shortlist from typed candidates.
fn build_evidence_shortlist(
    generated_at: &str,
    candidates: &[MergeCandidate],
) -> serde_json::Value {
    let evidence_candidates: Vec<serde_json::Value> = candidates
        .iter()
        .map(MergeCandidate::to_evidence_json)
        .collect();

    serde_json::json!({
        "generated_at": generated_at,
        "candidates": evidence_candidates,
    })
}

fn load_audit_context(paths: &ProjectPaths) -> Option<serde_json::Value> {
    let context_path = paths
        .automation_dir
        .join("cannibalization_audit_context.json");
    std::fs::read_to_string(&context_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn index_context_articles(
    context_doc: Option<&serde_json::Value>,
) -> HashMap<i64, serde_json::Value> {
    let mut articles_by_id: HashMap<i64, serde_json::Value> = HashMap::new();
    let Some(doc) = context_doc else {
        return articles_by_id;
    };
    if let Some(articles) = doc["articles"].as_array() {
        for a in articles {
            if let Some(id) = a["id"].as_i64() {
                articles_by_id.insert(id, a.clone());
            }
        }
    }
    articles_by_id
}

/// Inject `exact_keyword_duplicates.json` groups as typed `ExactKeyword`
/// candidates. Empty keywords and groups with fewer than 2 pages are skipped.
fn inject_exact_keyword_dupe_candidates(
    paths: &ProjectPaths,
    candidates: &mut Vec<MergeCandidate>,
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

        let candidate_id = format!(
            "exact_{}_{}",
            slugify(&keyword),
            candidates.len() + 1
        );

        candidates.push(MergeCandidate {
            candidate_id,
            lane: EvidenceLane::ExactKeyword,
            theme: keyword.clone(),
            pages: compact_pages,
            shared_queries: vec![keyword],
            total_impressions,
            page_count,
            max_pairwise_sim: None,
            best_performer: dupe.get("best_performer").cloned(),
        });
    }
}

/// Pure shared-query grouping used by the emit path and unit tests.
///
/// `rows` is `(lowercased_query, article_id, impressions, page_url)`.
/// Filters rows below [`SHARED_QUERY_MIN_IMPRESSIONS`], dedupes article_ids per
/// query, requires ≥2 pages, sorts groups by total impressions desc, and caps
/// each group at [`MAX_CANDIDATE_PAGES`].
pub(crate) fn group_shared_query_rows(
    rows: impl IntoIterator<Item = (String, i64, f64, String)>,
) -> Vec<(String, Vec<(i64, f64, String)>)> {
    let mut by_query: HashMap<String, Vec<(i64, f64, String)>> = HashMap::new();
    for (q, article_id, impressions, page_url) in rows {
        if impressions < SHARED_QUERY_MIN_IMPRESSIONS {
            continue;
        }
        let entry = by_query.entry(q).or_default();
        if entry.iter().any(|(id, _, _)| *id == article_id) {
            continue;
        }
        entry.push((article_id, impressions, page_url));
    }

    let mut query_groups: Vec<(String, Vec<(i64, f64, String)>)> = by_query
        .into_iter()
        .filter(|(_, pages)| pages.len() >= 2)
        .collect();
    query_groups.sort_by(|a, b| {
        let ta: f64 = a.1.iter().map(|(_, imp, _)| *imp).sum();
        let tb: f64 = b.1.iter().map(|(_, imp, _)| *imp).sum();
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });
    for (_, pages) in &mut query_groups {
        pages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        pages.truncate(MAX_CANDIDATE_PAGES);
    }
    query_groups.retain(|(_, pages)| pages.len() >= 2);
    query_groups
}

/// Emit shared_query lane candidates from `ctr_query_metrics`.
///
/// Groups by lowercased query; requires ≥2 distinct article_ids where each
/// article's impressions for that query meet [`SHARED_QUERY_MIN_IMPRESSIONS`].
/// Fail-closed: if the DB is unavailable or has no rows, emits nothing.
fn emit_shared_query_candidates(
    conn: &rusqlite::Connection,
    project_id: &str,
    articles_by_id: &HashMap<i64, serde_json::Value>,
    candidates: &mut Vec<MergeCandidate>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) {
    // Efficient single-query aggregation: rows that meet the impression floor.
    let mut stmt = match conn.prepare(
        r#"SELECT lower(query) AS q, article_id, impressions, page_url
           FROM ctr_query_metrics
           WHERE project_id = ?1 AND impressions >= ?2
           ORDER BY q, impressions DESC"#,
    ) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "[cannibalization_audit] shared_query lane skipped — prepare failed: {}",
                e
            );
            return;
        }
    };

    let rows = match stmt.query_map(
        rusqlite::params![project_id, SHARED_QUERY_MIN_IMPRESSIONS],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            log::warn!(
                "[cannibalization_audit] shared_query lane skipped — query failed: {}",
                e
            );
            return;
        }
    };

    let collected: Vec<(String, i64, f64, String)> = rows.flatten().collect();
    let query_groups = group_shared_query_rows(collected);

    for (query, page_rows) in query_groups {
        let compact_pages: Vec<serde_json::Value> = page_rows
            .iter()
            .filter_map(|(id, query_imps, page_url)| {
                if let Some(article) = articles_by_id.get(id) {
                    Some(compact_context_article(article))
                } else {
                    // Minimal page when context is missing — still allow shared_query
                    // evidence when GSC metrics exist (fail-open on packaging only).
                    let slug = page_url
                        .trim_start_matches("/blog/")
                        .trim_start_matches("blog/")
                        .trim_matches('/');
                    Some(serde_json::json!({
                        "id": id,
                        "url": crate::content::slug::format_blog_link(slug),
                        "title": slug,
                        "h1": slug,
                        "target_keyword": "",
                        "impressions": query_imps,
                        "clicks": 0.0,
                        "avg_position": 0.0,
                        "word_count": 0,
                        "incoming_internal_links": 0,
                        "outgoing_internal_links": 0,
                        "published_date": "",
                        "excerpt": "",
                    }))
                }
            })
            .collect();

        if compact_pages.len() < 2 {
            continue;
        }

        // Skip if every pairwise URL pair is already covered by exact_keyword.
        let urls: Vec<String> = compact_pages
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
        let mut all_pairs_covered = true;
        let mut any_pair = false;
        for i in 0..urls.len() {
            for j in (i + 1)..urls.len() {
                any_pair = true;
                if !covered_url_pairs.contains(&normalize_url_pair(&urls[i], &urls[j])) {
                    all_pairs_covered = false;
                }
            }
        }
        if any_pair && all_pairs_covered {
            continue;
        }

        record_url_pairs(&compact_pages, covered_url_pairs);

        let total_impressions: f64 = compact_pages
            .iter()
            .map(|p| p["impressions"].as_f64().unwrap_or(0.0))
            .sum();
        let page_count = compact_pages.len();
        let theme = query.clone();
        let candidate_id = format!("query_{}_{}", slugify(&theme), candidates.len() + 1);

        candidates.push(MergeCandidate {
            candidate_id,
            lane: EvidenceLane::SharedQuery,
            theme,
            pages: compact_pages,
            shared_queries: vec![query],
            total_impressions,
            page_count,
            max_pairwise_sim: None,
            best_performer: None,
        });
    }
}

/// Emit near_dupe candidates: embedding neighbors (preferred) or high TF-IDF pairs.
fn emit_near_dupe_candidates(
    paths: &ProjectPaths,
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
    articles_by_id: &HashMap<i64, serde_json::Value>,
    context_doc: Option<&serde_json::Value>,
    candidates: &mut Vec<MergeCandidate>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) {
    // Prefer embedding neighbors when the evidence index has vectors.
    if let Some(conn) = conn {
        let _ = emit_embedding_near_dupe_pairs(
            conn,
            project_id,
            articles_by_id,
            candidates,
            covered_url_pairs,
        );
    }

    // TF-IDF high-sim pairs always fill pairs not already covered by earlier
    // lanes or embedding near_dupes (via covered_url_pairs).
    emit_tfidf_near_dupe_pairs(
        paths,
        context_doc,
        articles_by_id,
        candidates,
        covered_url_pairs,
    );
}

/// Emit near_dupe pairs from `article_evidence::nearest_neighbors` (min sim 0.85).
/// Returns true if any embedding-backed pair was considered (index non-empty).
fn emit_embedding_near_dupe_pairs(
    conn: &rusqlite::Connection,
    project_id: &str,
    articles_by_id: &HashMap<i64, serde_json::Value>,
    candidates: &mut Vec<MergeCandidate>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) -> bool {
    if articles_by_id.is_empty() {
        return false;
    }

    let mut any_embedding = false;
    // Collect unique unordered pairs (id_a, id_b, sim) with id_a < id_b.
    let mut pair_sims: HashMap<(i64, i64), f64> = HashMap::new();

    for (id, article) in articles_by_id {
        let slug = article["url_slug"].as_str().unwrap_or("").trim();
        if slug.is_empty() {
            continue;
        }
        let neighbors = match crate::content::article_evidence::nearest_neighbors(
            conn,
            project_id,
            slug,
            MAX_CANDIDATE_PAGES,
            EMBEDDING_NEAR_DUPE_MIN_SIMILARITY,
        ) {
            Ok(n) => n,
            Err(_) => continue,
        };
        if !neighbors.is_empty() {
            any_embedding = true;
        }
        for n in neighbors {
            if !articles_by_id.contains_key(&n.article_id) {
                continue;
            }
            let key = if *id < n.article_id {
                (*id, n.article_id)
            } else {
                (n.article_id, *id)
            };
            let entry = pair_sims.entry(key).or_insert(0.0);
            if n.similarity > *entry {
                *entry = n.similarity;
            }
        }
    }

    if !any_embedding {
        return false;
    }

    let mut pairs: Vec<((i64, i64), f64)> = pair_sims.into_iter().collect();
    pairs.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });

    for ((id_a, id_b), sim) in pairs {
        push_near_dupe_pair(
            id_a,
            id_b,
            sim,
            articles_by_id,
            candidates,
            covered_url_pairs,
        );
    }

    true
}

/// Emit near_dupe from high TF-IDF `similarity_pairs` in audit context.
///
/// Always fills pairs not already present in `covered_url_pairs` (exact_keyword,
/// shared_query, or embedding near_dupe). No exclusive-vs-fill policy flag —
/// TF-IDF is always a gap-filler.
fn emit_tfidf_near_dupe_pairs(
    paths: &ProjectPaths,
    context_doc: Option<&serde_json::Value>,
    articles_by_id: &HashMap<i64, serde_json::Value>,
    candidates: &mut Vec<MergeCandidate>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) {
    let context_doc = match context_doc {
        Some(d) => d.clone(),
        None => {
            let context_path = paths
                .automation_dir
                .join("cannibalization_audit_context.json");
            match std::fs::read_to_string(&context_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
            {
                Some(v) => v,
                None => return,
            }
        }
    };

    let pairs = match context_doc["similarity_pairs"].as_array() {
        Some(arr) => arr,
        None => return,
    };

    // Prefer the pre-built index; fall back to indexing from this doc.
    let local_index = if articles_by_id.is_empty() {
        index_context_articles(Some(&context_doc))
    } else {
        HashMap::new()
    };
    let index = if articles_by_id.is_empty() {
        &local_index
    } else {
        articles_by_id
    };
    if index.is_empty() {
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
        push_near_dupe_pair(id_a, id_b, sim, index, candidates, covered_url_pairs);
    }
}

fn push_near_dupe_pair(
    id_a: i64,
    id_b: i64,
    sim: f64,
    articles_by_id: &HashMap<i64, serde_json::Value>,
    candidates: &mut Vec<MergeCandidate>,
    covered_url_pairs: &mut HashSet<(String, String)>,
) {
    let article_a = match articles_by_id.get(&id_a) {
        Some(a) => a,
        None => return,
    };
    let article_b = match articles_by_id.get(&id_b) {
        Some(a) => a,
        None => return,
    };

    let page_a = compact_context_article(article_a);
    let page_b = compact_context_article(article_b);
    let url_a = page_a["url"].as_str().unwrap_or("").to_string();
    let url_b = page_b["url"].as_str().unwrap_or("").to_string();
    if url_a.is_empty() || url_b.is_empty() {
        return;
    }

    let key = normalize_url_pair(&url_a, &url_b);
    if covered_url_pairs.contains(&key) {
        return;
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
            .unwrap_or("near_dupe")
            .to_lowercase()
    };

    let candidate_id = format!("near_dupe_{}_{}", slugify(&theme), candidates.len() + 1);

    candidates.push(MergeCandidate {
        candidate_id,
        lane: EvidenceLane::NearDupe,
        theme,
        pages: selected_pages,
        shared_queries: vec![],
        total_impressions,
        page_count: 2,
        max_pairwise_sim: Some(sim),
        best_performer: None,
    });
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

/// Compact a context-shaped article (url_slug + nested gsc) into a candidate page.
fn compact_context_article(article: &serde_json::Value) -> serde_json::Value {
    let slug = article["url_slug"].as_str().unwrap_or("");
    // Prefer url_slug; some upstream shapes already use `url`.
    let url = if !slug.is_empty() {
        crate::content::slug::format_blog_link(slug)
    } else {
        let raw = article["url"].as_str().unwrap_or("");
        crate::content::slug::format_blog_link(
            raw.trim_start_matches("/blog/")
                .trim_start_matches("blog/")
                .trim_matches('/'),
        )
    };
    let excerpt = article["first_200_words"]
        .as_str()
        .or_else(|| article["excerpt"].as_str())
        .unwrap_or("");
    let excerpt_words: Vec<&str> = excerpt.split_whitespace().take(60).collect();
    let impressions = article["gsc"]["impressions"]
        .as_f64()
        .or_else(|| article["impressions"].as_f64())
        .unwrap_or(0.0);
    let clicks = article["gsc"]["clicks"]
        .as_f64()
        .or_else(|| article["clicks"].as_f64())
        .unwrap_or(0.0);
    let avg_position = article["gsc"]["avg_position"]
        .as_f64()
        .or_else(|| article["avg_position"].as_f64())
        .unwrap_or(0.0);
    serde_json::json!({
        "id": article["id"],
        "url": url,
        "title": article["title"],
        "h1": article["h1"],
        "target_keyword": article["target_keyword"],
        "impressions": impressions,
        "clicks": clicks,
        "avg_position": avg_position,
        "word_count": article["word_count"],
        "incoming_internal_links": article["incoming_internal_links"],
        "outgoing_internal_links": article["outgoing_internal_links"],
        "published_date": article["published_date"],
        "excerpt": excerpt_words.join(" "),
    })
}

/// Shared_query impression floor (exported for docs/tests).
#[cfg(test)]
pub(crate) fn shared_query_min_impressions() -> f64 {
    SHARED_QUERY_MIN_IMPRESSIONS
}
