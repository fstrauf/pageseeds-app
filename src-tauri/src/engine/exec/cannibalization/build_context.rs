//! Step 1: Build cannibalization audit context.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Build Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the cannibalization audit context by reading articles.json, computing
/// TF-IDF cosine similarity between article content fingerprints, grouping by
/// identical target keywords, building connected-component clusters, scanning
/// the internal link graph, detecting hub gaps, and analysing territories.
pub(crate) fn exec_can_build_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Open DB connection for query overlap lookup (optional — falls back to proxy if unavailable)
    let db_path = crate::db::default_db_path();
    let db_conn = rusqlite::Connection::open(&db_path).ok();
    if db_conn.is_none() {
        log::warn!("[cannibalization_audit] Could not open DB at {:?} — using target_keyword proxy for query overlap", db_path);
    }
    let project_articles = crate::engine::exec::common::load_project_articles(&paths);
    let articles = &project_articles.articles;

    if articles.is_empty() {
        return StepResult::fail("No articles found in articles.json".to_string());
    }

    // ── 1. Build article records with content extraction ──────────────────────
    // Merged-away pages stay on disk (status `redirected`); their slugs live in
    // redirects.csv. Exclude them so a completed merge is never re-clustered and
    // re-recommended by the next audit.
    let redirected_slugs = crate::content::redirects::load_redirect_source_slugs(project_path);

    let mut records: Vec<ArticleRecord> = Vec::new();
    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
        if redirected_slugs.contains(&crate::content::slug::normalize_url_slug(&url_slug)) {
            continue;
        }
        let title = article["title"].as_str().unwrap_or("").to_string();
        let target_keyword = article["target_keyword"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();
        let gsc = article["gsc"].clone();
        let published_date = article["published_date"].as_str().unwrap_or("").to_string();

        let (h1, first_200_words, date_from_file) =
            read_article_head_and_words(project_path, &file_ref);
        let published_date = if published_date.is_empty() {
            date_from_file
        } else {
            published_date
        };
        let word_count = crate::content::ops::count_words(&first_200_words);

        let page_type = article["page_type"].as_str().map(String::from);
        let combined_text = format!("{} {} {} {}", title, h1, target_keyword, first_200_words);
        let tokens = tokenize(&combined_text);

        records.push(ArticleRecord {
            id,
            url_slug,
            title,
            h1,
            target_keyword,
            first_200_words,
            file: file_ref,
            gsc,
            tokens,
            incoming_links: 0,
            outgoing_links: 0,
            published_date,
            word_count,
            page_type,
        });
    }

    // ── 1b. Include ALL articles in clustering, even those with no GSC data.
    //     Articles with zero GSC impressions are often the ones most in need of
    //     cannibalization detection (e.g. not_indexed_crawled pages). They are
    //     excluded from GSC-based scoring but still clustered via TF-IDF similarity.
    //     We keep the full list; downstream scoring will handle GSC weighting.
    let mut records: Vec<ArticleRecord> = records;

    // ── 2. Compute TF-IDF vectors ─────────────────────────────────────────────
    let all_tokens: Vec<Vec<String>> = records.iter().map(|r| r.tokens.clone()).collect();
    let idf = compute_idf(&all_tokens);
    let tf_idf_vectors: Vec<TfIdfVector> = all_tokens
        .iter()
        .map(|tokens| {
            let tf = compute_tf(tokens);
            build_tf_idf_vector(&tf, &idf)
        })
        .collect();

    // ── 3. Compute cosine similarity pairs ────────────────────────────────────
    const SIMILARITY_THRESHOLD: f64 = 0.15;
    let mut similarity_pairs: Vec<serde_json::Value> = Vec::new();
    let mut similarity_edges: Vec<(usize, usize, f64)> = Vec::new();

    for i in 0..records.len() {
        for j in (i + 1)..records.len() {
            let sim = cosine_similarity(&tf_idf_vectors[i], &tf_idf_vectors[j]);
            if sim >= SIMILARITY_THRESHOLD {
                similarity_edges.push((i, j, sim));
                similarity_pairs.push(serde_json::json!({
                    "article_a_id": records[i].id,
                    "article_b_id": records[j].id,
                    "article_a_title": records[i].title,
                    "article_b_title": records[j].title,
                    "similarity": sim,
                }));
            }
        }
    }

    similarity_pairs.sort_by(|a, b| {
        let sa = a["similarity"].as_f64().unwrap_or(0.0);
        let sb = b["similarity"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    // ── 4. Group articles by identical target_keyword ─────────────────────────
    let mut keyword_groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for r in &records {
        let kw = r.target_keyword.trim().to_lowercase();
        if kw.is_empty() {
            continue;
        }
        let entry = serde_json::json!({
            "id": r.id,
            "title": r.title,
            "url_slug": r.url_slug,
            "file": r.file,
        });
        keyword_groups.entry(kw.clone()).or_default().push(entry);
    }

    // Build keyword-group edges for clustering (same target_keyword = strong overlap)
    let mut keyword_edges: Vec<(usize, usize, f64)> = Vec::new();
    let mut kw_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, r) in records.iter().enumerate() {
        let kw = r.target_keyword.trim().to_lowercase();
        if !kw.is_empty() {
            kw_to_indices.entry(kw).or_default().push(idx);
        }
    }
    for indices in kw_to_indices.values() {
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                keyword_edges.push((indices[i], indices[j], 1.0));
            }
        }
    }

    let keyword_groups_json: HashMap<String, Vec<serde_json::Value>> = keyword_groups
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .collect();

    // ── 5. Scan internal link graph ───────────────────────────────────────────
    enrich_link_metrics(&mut records, project_path);

    // ── 6. Build connected-component clusters ─────────────────────────────────
    let mut all_edges = similarity_edges.clone();
    all_edges.extend(keyword_edges);
    let clusters = build_clusters(&records, &all_edges, db_conn.as_ref(), &task.project_id);

    // ── 7. Detect hub gaps ────────────────────────────────────────────────────
    let hub_gaps = detect_hub_gaps(&records, &clusters, db_conn.as_ref(), &task.project_id);

    // ── 9. Build serializable article list ────────────────────────────────────
    let articles_json: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "url_slug": r.url_slug,
                "title": r.title,
                "h1": r.h1,
                "target_keyword": r.target_keyword,
                "first_200_words": r.first_200_words,
                "file": r.file,
                "gsc": r.gsc,
                "incoming_internal_links": r.incoming_links,
                "outgoing_internal_links": r.outgoing_links,
                "published_date": r.published_date,
                "word_count": r.word_count,
            })
        })
        .collect();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // ── 10. Compute site summary ──────────────────────────────────────────────
    let total_impressions: f64 = records
        .iter()
        .map(|r| r.gsc["impressions"].as_f64().unwrap_or(0.0))
        .sum();
    let period_days = records
        .iter()
        .filter_map(|r| r.gsc["period_days"].as_i64())
        .next()
        .unwrap_or(90);

    // ── 11. Write artifacts ───────────────────────────────────────────────────
    let full_doc = serde_json::json!({
        "generated_at": &now_iso,
        "total_articles": articles_json.len(),
        "total_impressions": total_impressions,
        "period_days": period_days,
        "articles": &articles_json,
        "similarity_pairs": &similarity_pairs,
        "keyword_groups": &keyword_groups_json,
    });

    let out_path = paths
        .automation_dir
        .join("cannibalization_audit_context.json");
    let full_str = serde_json::to_string_pretty(&full_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &full_str) {
        log::warn!(
            "[cannibalization_audit] Failed to write cannibalization_audit_context.json: {}",
            e
        );
    }

    let clusters_json: Vec<serde_json::Value> = clusters
        .iter()
        .map(|c| {
            let pages: Vec<serde_json::Value> = c
                .page_ids
                .iter()
                .filter_map(|&pid| records.iter().find(|r| r.id == pid))
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "url": crate::content::slug::format_blog_link(&r.url_slug),
                        "title": r.title,
                        "h1": r.h1,
                        "target_keyword": r.target_keyword,
                        "impressions": r.gsc["impressions"].as_f64().unwrap_or(0.0),
                        "clicks": r.gsc["clicks"].as_f64().unwrap_or(0.0),
                        "ctr": r.gsc["ctr"].as_f64().unwrap_or(0.0),
                        "avg_position": r.gsc["avg_position"].as_f64().unwrap_or(0.0),
                        "word_count": r.word_count,
                        "incoming_internal_links": r.incoming_links,
                        "outgoing_internal_links": r.outgoing_links,
                        "published_date": r.published_date,
                        "first_200_words": r.first_200_words,
                    })
                })
                .collect();
            serde_json::json!({
                "cluster_id": c.cluster_id,
                "theme": c.theme,
                "candidate_intent": c.candidate_intent,
                "total_impressions": c.total_impressions,
                "total_clicks": c.total_clicks,
                "avg_position": c.avg_position,
                "shared_query_count": c.shared_query_count,
                "hub_exists": c.hub_exists,
                "pages": pages,
                "top_shared_queries": c.top_shared_queries,
            })
        })
        .collect();

    let clusters_doc = serde_json::json!({
        "generated_at": &now_iso,
        "clusters": &clusters_json,
    });

    // Save to database (new primary storage)
    if let Some(ref db) = db_conn {
        let _ = crate::db::content_audit::save_audit_artifact(
            db,
            &task.project_id,
            "cannibalization_clusters",
            &now_iso,
            &serde_json::to_string(&clusters_doc).unwrap_or_default(),
        );
    }

    // Also write clusters artifact to disk for downstream consumers
    let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
    if let Err(e) = std::fs::write(
        &clusters_path,
        serde_json::to_string_pretty(&clusters_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write cannibalization_clusters.json: {}",
            e
        );
    }

    let hub_gaps_path = paths.automation_dir.join("hub_gaps.json");
    let hub_gaps_doc = serde_json::json!({
        "generated_at": &now_iso,
        "hub_gaps": &hub_gaps,
    });
    if let Err(e) = std::fs::write(
        &hub_gaps_path,
        serde_json::to_string_pretty(&hub_gaps_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write hub_gaps.json: {}",
            e
        );
    }

    // ── 11. Return compact artifact summary (full context stays on disk) ─────
    let summary = serde_json::json!({
        "artifact_paths": {
            "context": ".github/automation/cannibalization_audit_context.json",
            "clusters": ".github/automation/cannibalization_clusters.json",
            "hub_gaps": ".github/automation/hub_gaps.json"
        },
        "summary": {
            "total_articles": articles_json.len(),
            "total_impressions": total_impressions,
            "similarity_pairs": similarity_pairs.len(),
            "candidate_clusters": clusters.len(),
            "hub_gaps": hub_gaps.len()
        }
    });

    // Staleness warning (issue #25): ctr_query_metrics older than the IHC gate
    // tolerance must be visible in the task output. Warning only — never fails.
    let staleness_warning = db_conn
        .as_ref()
        .and_then(|db| crate::engine::exec::common::ctr_metrics_staleness_warning(db, &task.project_id));
    if let Some(ref warning) = staleness_warning {
        log::warn!("[cannibalization_audit] {}", warning);
    }

    StepResult {
        success: true,
        message: format!(
            "Cannibalization context built: {} articles, {} similar pairs, {} keyword groups, {} clusters, {} hub gaps{}",
            articles_json.len(),
            similarity_pairs.len(),
            keyword_groups_json.len(),
            clusters.len(),
            hub_gaps.len(),
            staleness_warning
                .map(|w| format!(" — {}", w))
                .unwrap_or_default(),
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}
