/// Keyword cannibalization audit execution module.
///
/// Covers:
///   - exec_can_build_context   (deterministic TF-IDF clustering + link graph + hub gaps)
///   - create_can_fix_tasks     (spawn follow-up fix tasks)
use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Data structures
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
struct ArticleRecord {
    id: i64,
    url_slug: String,
    title: String,
    h1: String,
    target_keyword: String,
    first_200_words: String,
    file: String,
    gsc: serde_json::Value,
    tokens: Vec<String>,
    incoming_links: usize,
    outgoing_links: usize,
    published_date: String,
    word_count: usize,
    page_type: Option<String>,
}

#[derive(Debug)]
struct TfIdfVector {
    weights: HashMap<String, f64>,
    norm: f64,
}

#[derive(Debug)]
struct Cluster {
    cluster_id: String,
    theme: String,
    candidate_intent: String,
    total_impressions: f64,
    total_clicks: f64,
    avg_position: f64,
    shared_query_count: usize,
    hub_exists: bool,
    page_ids: Vec<i64>,
    top_shared_queries: Vec<String>,
}

// ─── Union-Find for connected-component clustering ────────────────────────────

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        match self.rank[rx].cmp(&self.rank[ry]) {
            std::cmp::Ordering::Less => self.parent[rx] = ry,
            std::cmp::Ordering::Greater => self.parent[ry] = rx,
            std::cmp::Ordering::Equal => {
                self.parent[ry] = rx;
                self.rank[rx] += 1;
            }
        }
    }
}

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
    let articles_path = paths.automation_dir.join("articles.json");

    let doc: serde_json::Value =
        match crate::engine::exec::common::read_json(&articles_path, "articles.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    let empty = vec![];
    let articles = doc["articles"].as_array().unwrap_or(&empty);

    if articles.is_empty() {
        return StepResult {
            success: false,
            message: "No articles found in articles.json".to_string(),
            output: None,
        };
    }

    // ── 1. Build article records with content extraction ──────────────────────
    let mut records: Vec<ArticleRecord> = Vec::new();
    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
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

    // Keep JSON write as export during transition
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

    StepResult {
        success: true,
        message: format!(
            "Cannibalization context built: {} articles, {} similar pairs, {} keyword groups, {} clusters, {} hub gaps",
            articles_json.len(),
            similarity_pairs.len(),
            keyword_groups_json.len(),
            clusters.len(),
            hub_gaps.len(),
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TF-IDF
// ═══════════════════════════════════════════════════════════════════════════════

/// Tokenize text into normalized terms suitable for TF-IDF.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

/// Compute raw term frequencies for a token list.
fn compute_tf(tokens: &[String]) -> HashMap<String, f64> {
    let mut tf: HashMap<String, f64> = HashMap::new();
    if tokens.is_empty() {
        return tf;
    }
    for token in tokens {
        *tf.entry(token.clone()).or_insert(0.0) += 1.0;
    }
    let n = tokens.len() as f64;
    for count in tf.values_mut() {
        *count /= n;
    }
    tf
}

/// Compute inverse document frequency across the corpus.
fn compute_idf(documents: &[Vec<String>]) -> HashMap<String, f64> {
    let n = documents.len() as f64;
    let mut df: HashMap<String, f64> = HashMap::new();
    for doc in documents {
        let unique: HashSet<&String> = doc.iter().collect();
        for term in unique {
            *df.entry(term.clone()).or_insert(0.0) += 1.0;
        }
    }
    let mut idf: HashMap<String, f64> = HashMap::new();
    for (term, doc_count) in df {
        idf.insert(term, (n / doc_count).ln() + 1.0);
    }
    idf
}

/// Build a TF-IDF vector from term frequencies and IDF map.
fn build_tf_idf_vector(tf: &HashMap<String, f64>, idf: &HashMap<String, f64>) -> TfIdfVector {
    let mut weights: HashMap<String, f64> = HashMap::new();
    let mut norm_sq = 0.0;
    for (term, tf_val) in tf {
        let idf_val = idf.get(term).copied().unwrap_or(0.0);
        let w = tf_val * idf_val;
        weights.insert(term.clone(), w);
        norm_sq += w * w;
    }
    TfIdfVector {
        weights,
        norm: norm_sq.sqrt(),
    }
}

/// Compute cosine similarity between two TF-IDF vectors.
fn cosine_similarity(a: &TfIdfVector, b: &TfIdfVector) -> f64 {
    if a.norm == 0.0 || b.norm == 0.0 {
        return 0.0;
    }
    let mut dot = 0.0;
    for (term, w_a) in &a.weights {
        if let Some(w_b) = b.weights.get(term) {
            dot += w_a * w_b;
        }
    }
    dot / (a.norm * b.norm)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Clustering
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute real query overlap between articles in a cluster.
/// Uses `ctr_query_metrics` when available; falls back to target_keyword proxy otherwise.
fn compute_query_overlap(
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
    records: &[ArticleRecord],
    indices: &[usize],
) -> (usize, Vec<String>) {
    let mut query_sets: Vec<HashSet<String>> = Vec::new();
    let mut has_db_data = false;

    if let Some(conn) = conn {
        for &i in indices {
            let article_id = records[i].id;
            if let Ok(rows) = crate::db::get_ctr_query_metrics(conn, project_id, article_id) {
                if !rows.is_empty() {
                    has_db_data = true;
                    let queries: HashSet<String> =
                        rows.into_iter().map(|r| r.query.to_lowercase()).collect();
                    query_sets.push(queries);
                }
            }
        }
    }

    if !has_db_data {
        // Fall back to target_keyword proxy
        let shared: HashSet<String> = indices
            .iter()
            .map(|&i| records[i].target_keyword.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let count = shared.len();
        let top: Vec<String> = shared.into_iter().take(5).collect();
        return (count, top);
    }

    if query_sets.len() <= 1 {
        let count = query_sets.first().map(|s| s.len()).unwrap_or(0);
        let top: Vec<String> = query_sets
            .first()
            .map(|s| s.iter().cloned().take(5).collect())
            .unwrap_or_default();
        return (count, top);
    }

    // Queries that appear in at least 2 pages (pairwise overlap)
    let mut shared_queries: HashSet<String> = HashSet::new();
    for i in 0..query_sets.len() {
        for j in (i + 1)..query_sets.len() {
            for q in query_sets[i].intersection(&query_sets[j]) {
                shared_queries.insert(q.clone());
            }
        }
    }

    let count = shared_queries.len();
    let mut top: Vec<String> = shared_queries.into_iter().take(5).collect();
    top.sort();
    (count, top)
}

/// Build connected-component clusters from similarity edges and keyword edges.
fn build_clusters(
    records: &[ArticleRecord],
    edges: &[(usize, usize, f64)],
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
) -> Vec<Cluster> {
    let n = records.len();
    let mut uf = UnionFind::new(n);

    for (i, j, _sim) in edges {
        uf.union(*i, *j);
    }

    // Group indices by root
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    for (_root, indices) in groups {
        if indices.len() < 2 {
            continue; // Only clusters with 2+ articles
        }

        let page_ids: Vec<i64> = indices.iter().map(|&i| records[i].id).collect();

        // Determine theme from the most common target_keyword
        let mut kw_counts: HashMap<String, usize> = HashMap::new();
        for &i in &indices {
            let kw = records[i].target_keyword.trim().to_lowercase();
            if !kw.is_empty() {
                *kw_counts.entry(kw).or_insert(0) += 1;
            }
        }
        let theme = kw_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(kw, _)| kw.clone())
            .unwrap_or_else(|| records[indices[0]].title.clone());

        let candidate_intent = theme.clone();

        // Aggregate GSC metrics
        let mut total_impressions = 0.0;
        let mut total_clicks = 0.0;
        let mut position_sum = 0.0;
        let mut position_count = 0;
        for &i in &indices {
            total_impressions += records[i].gsc["impressions"].as_f64().unwrap_or(0.0);
            total_clicks += records[i].gsc["clicks"].as_f64().unwrap_or(0.0);
            if let Some(pos) = records[i].gsc["avg_position"].as_f64() {
                if pos > 0.0 {
                    position_sum += pos;
                    position_count += 1;
                }
            }
        }
        let avg_position = if position_count > 0 {
            position_sum / position_count as f64
        } else {
            0.0
        };

        // Real query overlap from ctr_query_metrics (falls back to target_keyword proxy)
        let (shared_query_count, top_shared_queries) =
            compute_query_overlap(conn, project_id, records, &indices);

        // Hub existence: check if any page in cluster has a hub-like URL
        let hub_exists = indices.iter().any(|&i| {
            let slug = &records[i].url_slug;
            slug.starts_with("hub/") || slug.starts_with("guide/")
        });

        clusters.push(Cluster {
            cluster_id: slugify(&theme),
            theme,
            candidate_intent,
            total_impressions,
            total_clicks,
            avg_position,
            shared_query_count,
            hub_exists,
            page_ids,
            top_shared_queries,
        });
    }

    // Sort clusters by total impressions descending
    clusters.sort_by(|a, b| {
        b.total_impressions
            .partial_cmp(&a.total_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    clusters
}

/// Simple slugify: lowercase, replace non-alphanumeric with underscores.
fn slugify(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Link graph
// ═══════════════════════════════════════════════════════════════════════════════

/// Enrich article records with incoming/outgoing internal link counts.
fn enrich_link_metrics(records: &mut [ArticleRecord], project_path: &str) {
    let repo_root = Path::new(project_path);
    let content_resolution = crate::content::locator::resolve(repo_root, None);
    let Some(content_dir) = content_resolution.selected else {
        log::warn!("[cannibalization_audit] Could not find content directory for link scan");
        return;
    };

    // Build minimal Article structs for scan_links
    let articles: Vec<crate::models::article::Article> = records
        .iter()
        .map(|r| crate::models::article::Article {
            id: r.id,
            title: r.title.clone(),
            url_slug: r.url_slug.clone(),
            file: r.file.clone(),
            target_keyword: Some(r.target_keyword.clone()),
            keyword_difficulty: None,
            target_volume: 0,
            published_date: Some(r.published_date.clone()),
            word_count: r.word_count as i64,
            status: "published".to_string(),
            review_status: None,
            review_started_at: None,
            last_reviewed_at: None,
            review_count: 0,
            content_gaps_addressed: vec![],
            estimated_traffic_monthly: None,
            page_type: None,
            project_id: String::new(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
            content_hash: None,
            last_edited_at: None,
        })
        .collect();

    match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(result) => {
            for profile in &result.profiles {
                if let Some(record) = records.iter_mut().find(|r| r.id == profile.id) {
                    record.incoming_links = profile.incoming_ids.len();
                    record.outgoing_links = profile.outgoing_ids.len();
                }
            }
        }
        Err(e) => {
            log::warn!("[cannibalization_audit] Link scan failed: {}", e);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hub gaps
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect clusters that lack a hub/pillar page.
/// Uses DB-tracked page_type='hub' first, then falls back to URL prefix heuristics.
fn detect_hub_gaps(
    records: &[ArticleRecord],
    clusters: &[Cluster],
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
) -> Vec<serde_json::Value> {
    let mut existing_hubs: HashSet<String> = HashSet::new();

    // 1. Primary: DB-tracked hub pages (page_type = 'hub')
    if let Some(conn) = conn {
        match conn.prepare(
            "SELECT url_slug, target_keyword, title FROM articles WHERE project_id = ?1 AND page_type = 'hub'",
        ) {
            Ok(mut stmt) => {
                let rows = stmt.query_map([project_id], |row| {
                    let slug: String = row.get(0)?;
                    let kw: Option<String> = row.get(1)?;
                    let title: String = row.get(2)?;
                    Ok((slug, kw, title))
                });
                if let Ok(rows) = rows {
                    for row in rows.filter_map(|r| r.ok()) {
                        let (slug, kw, title) = row;
                        if let Some(kw) = kw.filter(|s| !s.is_empty()) {
                            existing_hubs.insert(kw.trim().to_lowercase());
                        }
                        // Derive topic from slug
                        let stripped = if slug.starts_with("hub/") {
                            &slug[4..]
                        } else if slug.starts_with("guide/") {
                            &slug[6..]
                        } else if slug.starts_with("hub_") {
                            &slug[4..]
                        } else if slug.starts_with("guide_") {
                            &slug[6..]
                        } else {
                            &slug
                        };
                        let stripped = stripped.trim().replace('_', " ").replace('-', " ").to_lowercase();
                        if !stripped.is_empty() {
                            existing_hubs.insert(stripped);
                        }
                        // Title topic
                        let title_topic = title
                            .trim()
                            .to_lowercase()
                            .trim_end_matches(": complete guide")
                            .trim_end_matches(": the complete guide")
                            .trim_end_matches(" complete guide")
                            .trim_end_matches(": ultimate guide")
                            .trim_end_matches(" ultimate guide")
                            .trim()
                            .to_string();
                        if !title_topic.is_empty() {
                            existing_hubs.insert(title_topic);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("[detect_hub_gaps] Failed to query DB hubs: {}", e);
            }
        }
    }

    // 2. From article records: explicit page_type='hub' or heuristic detection
    for r in records {
        let is_hub_explicit = r.page_type.as_deref() == Some("hub");
        let is_hub_heuristic = !is_hub_explicit
            && (r.url_slug.starts_with("hub/")
                || r.url_slug.starts_with("guide/")
                || r.url_slug.starts_with("hub_")
                || r.url_slug.starts_with("guide_")
                || r.title.to_lowercase().contains("complete guide")
                || r.title.to_lowercase().contains("ultimate guide"));

        if !is_hub_explicit && !is_hub_heuristic {
            continue;
        }

        let kw = r.target_keyword.trim().to_lowercase();
        if !kw.is_empty() {
            existing_hubs.insert(kw);
        }
        let slug = &r.url_slug;
        let stripped = if slug.starts_with("hub/") {
            &slug[4..]
        } else if slug.starts_with("guide/") {
            &slug[6..]
        } else if slug.starts_with("hub_") {
            &slug[4..]
        } else if slug.starts_with("guide_") {
            &slug[6..]
        } else {
            ""
        };
        let stripped = stripped
            .trim()
            .replace('_', " ")
            .replace('-', " ")
            .to_lowercase();
        if !stripped.is_empty() {
            existing_hubs.insert(stripped);
        }
        let title_topic = r
            .title
            .trim()
            .to_lowercase()
            .trim_end_matches(": complete guide")
            .trim_end_matches(": the complete guide")
            .trim_end_matches(" complete guide")
            .trim_end_matches(": ultimate guide")
            .trim_end_matches(" ultimate guide")
            .trim()
            .to_string();
        if !title_topic.is_empty() {
            existing_hubs.insert(title_topic);
        }
    }

    let mut gaps: Vec<serde_json::Value> = Vec::new();
    for cluster in clusters {
        if cluster.hub_exists {
            continue;
        }
        if cluster.page_ids.len() < 3 {
            continue; // Only suggest hubs for clusters with 3+ articles
        }

        let theme_kw = cluster.theme.trim().to_lowercase();
        let has_related_hub = existing_hubs
            .iter()
            .any(|hub_kw| theme_kw.contains(hub_kw) || hub_kw.contains(&theme_kw));

        if has_related_hub {
            continue;
        }

        let spoke_pages: Vec<serde_json::Value> = cluster
            .page_ids
            .iter()
            .filter_map(|&pid| records.iter().find(|r| r.id == pid))
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "url": crate::content::slug::format_blog_link(&r.url_slug),
                    "title": r.title,
                    "impressions": r.gsc["impressions"].as_f64().unwrap_or(0.0),
                })
            })
            .collect();

        gaps.push(serde_json::json!({
            "cluster_id": &cluster.cluster_id,
            "theme": &cluster.theme,
            "suggested_url": format!("/hub/{}", cluster.cluster_id.replace('_', "-")),
            "suggested_title": format!("{}: Complete Guide", capitalize_words(&cluster.theme)),
            "spoke_count": cluster.page_ids.len(),
            "total_impressions": cluster.total_impressions,
            "spoke_pages": spoke_pages,
            "reason": format!("Cluster has {} articles with {} total impressions but no broad parent hub.", cluster.page_ids.len(), cluster.total_impressions as i64),
        }));
    }

    gaps.sort_by(|a, b| {
        let ta = a["total_impressions"].as_f64().unwrap_or(0.0);
        let tb = b["total_impressions"].as_f64().unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });

    gaps
}

fn capitalize_words(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Read an MDX file and extract (h1, first_200_words, published_date).
fn read_article_head_and_words(project_path: &str, file_ref: &str) -> (String, String, String) {
    if file_ref.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let repo_root = Path::new(project_path);
    let p = Path::new(file_ref);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };

    let content = match std::fs::read_to_string(&full) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "[cannibalization_audit] Could not read {}: {}",
                full.display(),
                e
            );
            return (String::new(), String::new(), String::new());
        }
    };

    let (frontmatter_raw, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((fm, b)) => (fm, b),
        None => ("", content.as_str()),
    };

    // Extract date from frontmatter if available
    let published_date = crate::content::frontmatter::top_level_scalars(frontmatter_raw)
        .into_iter()
        .find(|f| f.key == "date")
        .map(|f| f.raw_value.trim_matches('"').trim_matches('\'').to_string())
        .unwrap_or_default();

    // Extract h1
    let h1 = body
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("# ") && !t.starts_with("## ")
        })
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();

    // Extract first 200 words from body (strip markdown syntax roughly)
    let plain = body
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && !t.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join(" ");

    let words: Vec<&str> = plain.split_whitespace().collect();
    let first_200_words = words.into_iter().take(200).collect::<Vec<_>>().join(" ");

    (h1, first_200_words, published_date)
}

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
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Select Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic candidate selection from cannibalization cluster artifacts.
///
/// Reads `cannibalization_clusters.json`, scores clusters, splits giant components
/// by target keyword, caps pages per candidate at 8, and writes
/// `cannibalization_candidates.json`.
pub(crate) fn exec_can_select_candidates(_task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");

    let clusters_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &clusters_path,
        "cannibalization_clusters.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
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
            &_task.project_id,
            "cannibalization_candidates",
            &now_iso,
            &serde_json::to_string(&candidates_doc).unwrap_or_default(),
        );
    }

    // Keep JSON write as export during transition
    let candidates_path = paths.automation_dir.join("cannibalization_candidates.json");
    if let Err(e) = std::fs::write(
        &candidates_path,
        serde_json::to_string_pretty(&candidates_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write candidates file: {}",
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

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Analyze Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic analysis of individual merge candidates with byte-budgeted prompts.
///
/// Why not deterministic: each candidate is a cluster of 2–8 pages competing for the
/// same keyword(s). Deciding which page to keep, which to redirect, and how to merge
/// unique valuable content requires judgment about content quality, user intent,
/// URL authority, and GSC performance. No finite rule set can correctly resolve all
/// valid inputs because the "best" keeper depends on nuanced semantic comparison.
/// The output is a structured `CandidateAnalysisOutput` per candidate, extracted
/// via Rig's `extract_structured`.
///
/// Reads `cannibalization_candidates.json`, calls the agent once per candidate,
/// and writes `cannibalization_batch_outputs.json`.
pub(crate) fn exec_can_analyze_candidates(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    let candidates_path = paths.automation_dir.join("cannibalization_candidates.json");
    let candidates_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &candidates_path,
        "cannibalization_candidates.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let candidates = candidates_doc["candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let candidates_len = candidates.len();
    if candidates.is_empty() {
        return StepResult {
            success: true,
            message: "No candidates to analyze.".to_string(),
            output: None,
        };
    }

    const TARGET_PROMPT_BYTES: usize = 15_000;
    const HARD_PROMPT_BYTES: usize = 20_000;

    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "cannibalization-strategy") {
        Ok(s) => s,
        Err(msg) => {
            return StepResult { success: false, message: msg, output: None };
        }
    };

    let mut batch_outputs: Vec<serde_json::Value> = Vec::new();
    let mut failed_candidates: Vec<String> = Vec::new();

    for candidate in &candidates {
        let candidate_id = candidate["candidate_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let (prompt, prompt_bytes) = build_merge_prompt(&skill.content, &candidate);

        let chosen_prompt = if prompt_bytes > HARD_PROMPT_BYTES {
            let (trimmed, trimmed_bytes) = build_merge_prompt_trimmed(&skill.content, &candidate);
            if trimmed_bytes > HARD_PROMPT_BYTES {
                log::warn!(
                    "[cannibalization_audit] Candidate {} still exceeds hard limit after trimming ({} bytes). Skipping.",
                    candidate_id,
                    trimmed_bytes
                );
                failed_candidates.push(candidate_id.clone());
                batch_outputs.push(serde_json::json!({
                    "candidate_id": candidate_id,
                    "success": false,
                    "message": format!("Prompt exceeded hard limit ({} bytes)", trimmed_bytes),
                    "merge_recommendation": null,
                }));
                continue;
            }
            log::info!(
                "[cannibalization_audit] Candidate {} trimmed from {} to {} bytes",
                candidate_id,
                prompt_bytes,
                trimmed_bytes
            );
            trimmed
        } else {
            prompt
        };

        // Additional safety: warn if we're over target but under hard
        if chosen_prompt.len() > TARGET_PROMPT_BYTES && chosen_prompt.len() <= HARD_PROMPT_BYTES {
            log::info!(
                "[cannibalization_audit] Candidate {} prompt is {} bytes (over target {})",
                candidate_id,
                chosen_prompt.len(),
                TARGET_PROMPT_BYTES
            );
        }

        // Run the structured extractor inside a fresh runtime because this
        // function is called from within tokio::task::spawn_blocking.
        let extract_result = {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    log::warn!(
                        "[cannibalization_audit] Failed to create runtime for candidate {}: {}",
                        candidate_id,
                        e
                    );
                    failed_candidates.push(candidate_id.clone());
                    batch_outputs.push(serde_json::json!({
                        "candidate_id": candidate_id,
                        "success": false,
                        "message": format!("Runtime error: {}", e),
                        "merge_recommendation": null,
                    }));
                    continue;
                }
            };
            rt.block_on(async {
                crate::rig::extraction::extract_structured::<
                    crate::models::cannibalization::CandidateAnalysisOutput,
                >(
                    agent_provider,
                    &chosen_prompt,
                    Some("You are an expert SEO strategist. Analyze the candidate and return structured JSON."),
                    Some("direct"),
                    None,
                )
                .await
            })
        };

        match extract_result {
            Ok(mut rec) => {
                // Defensive normalization: ensure required fields are present.
                if rec.cluster_id.is_empty() {
                    rec.cluster_id = candidate_id.clone();
                }
                if rec.cluster_theme.is_empty() {
                    rec.cluster_theme = candidate["theme"].as_str().unwrap_or("").to_string();
                }
                if rec.confidence.is_empty() {
                    rec.confidence = "medium".to_string();
                }
                if rec.keep_url.is_empty() && !rec.no_action {
                    rec.no_action = true;
                    rec.reason =
                        "Model did not provide a keep_url or explicit no_action".to_string();
                }
                let rec_json = match serde_json::to_value(&rec) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!(
                            "[cannibalization_audit] Failed to serialize analysis for candidate {}: {}",
                            candidate_id,
                            e
                        );
                        failed_candidates.push(candidate_id.clone());
                        batch_outputs.push(serde_json::json!({
                            "candidate_id": candidate_id,
                            "success": false,
                            "message": format!("Serialize error: {}", e),
                            "merge_recommendation": null,
                        }));
                        continue;
                    }
                };
                batch_outputs.push(serde_json::json!({
                    "candidate_id": candidate_id,
                    "success": true,
                    "message": "Analyzed successfully",
                    "merge_recommendation": rec_json,
                }));
            }
            Err(e) => {
                log::warn!(
                    "[cannibalization_audit] Structured extraction failed for candidate {}: {}",
                    candidate_id,
                    e
                );
                failed_candidates.push(candidate_id.clone());
                batch_outputs.push(serde_json::json!({
                    "candidate_id": candidate_id,
                    "success": false,
                    "message": format!("Extraction error: {}", e),
                    "merge_recommendation": null,
                }));
            }
        }
    }

    let batch_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "batch_outputs": batch_outputs,
        "failed_candidates": failed_candidates,
    });

    let batch_path = paths
        .automation_dir
        .join("cannibalization_batch_outputs.json");
    if let Err(e) = std::fs::write(
        &batch_path,
        serde_json::to_string_pretty(&batch_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write batch outputs: {}",
            e
        );
    }

    let success_count = batch_outputs
        .iter()
        .filter(|o| o["success"].as_bool().unwrap_or(false))
        .count();

    StepResult {
        success: failed_candidates.is_empty() || success_count > 0,
        message: format!(
            "Analyzed {}/{} candidates successfully. Failed: {}",
            success_count,
            candidates_len,
            failed_candidates.len()
        ),
        output: Some(serde_json::to_string_pretty(&batch_doc).unwrap_or_default()),
    }
}

/// Build the full merge-analysis prompt for a single candidate.
fn build_merge_prompt(skill_content: &str, candidate: &serde_json::Value) -> (String, usize) {
    let candidate_json = serde_json::to_string_pretty(candidate).unwrap_or_default();
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## Merge Candidate\n\n"
        + &candidate_json
        + "\n\nAnalyze ONLY this candidate cluster. Decide if the pages represent true cannibalization (same search intent competing in SERPs) or just topical similarity.\n\n"
        + "If true cannibalization: recommend a keeper URL, redirect URLs, and merge instructions.\n"
        + "If not: return no_action with a reason.\n\n"
        + "CRITICAL: Return ONLY a single JSON object matching the Output Contract. Do not include markdown prose outside the JSON.";
    let bytes = prompt.len();
    (prompt, bytes)
}

/// Build a trimmed prompt without page excerpts (second-level budget fallback).
fn build_merge_prompt_trimmed(
    skill_content: &str,
    candidate: &serde_json::Value,
) -> (String, usize) {
    let mut trimmed = candidate.clone();
    if let Some(pages) = trimmed["pages"].as_array_mut() {
        for page in pages {
            if let serde_json::Value::Object(ref mut map) = page {
                map.remove("excerpt");
            }
        }
    }
    let candidate_json = serde_json::to_string_pretty(&trimmed).unwrap_or_default();
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## Merge Candidate (Trimmed)\n\n"
        + &candidate_json
        + "\n\nAnalyze ONLY this candidate cluster. Decide if the pages represent true cannibalization (same search intent competing in SERPs) or just topical similarity.\n\n"
        + "If true cannibalization: recommend a keeper URL, redirect URLs, and merge instructions.\n"
        + "If not: return no_action with a reason.\n\n"
        + "CRITICAL: Return ONLY a single JSON object matching the Output Contract. Do not include markdown prose outside the JSON.";
    let bytes = prompt.len();
    (prompt, bytes)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Reduce Strategy
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic reducer that merges batch outputs into the final
/// `cannibalization_strategy.json`.
///
/// Validates merge recommendations and includes deterministic hub data.
pub(crate) fn exec_can_reduce_strategy(_task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let batch_path = paths
        .automation_dir
        .join("cannibalization_batch_outputs.json");
    let batch_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &batch_path,
        "cannibalization_batch_outputs.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let hub_gaps_path = paths.automation_dir.join("hub_gaps.json");
    let hub_gaps_doc: serde_json::Value = std::fs::read_to_string(&hub_gaps_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "hub_gaps": [] }));

    let mut merge_recommendations: Vec<serde_json::Value> = Vec::new();
    let mut risks: Vec<String> = Vec::new();

    if let Some(outputs) = batch_doc["batch_outputs"].as_array() {
        for output in outputs {
            if !output["success"].as_bool().unwrap_or(false) {
                if let Some(cid) = output["candidate_id"].as_str() {
                    risks.push(format!(
                        "Candidate {} failed: {}",
                        cid,
                        output["message"].as_str().unwrap_or("unknown error")
                    ));
                }
                continue;
            }

            if let Some(rec) = output["merge_recommendation"].as_object() {
                if rec
                    .get("no_action")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    continue;
                }

                let keep_url = rec.get("keep_url").and_then(|v| v.as_str()).unwrap_or("");
                if keep_url.is_empty() {
                    risks.push(format!(
                        "Missing keep_url for candidate {}",
                        output["candidate_id"].as_str().unwrap_or("?")
                    ));
                    continue;
                }

                let redirect_urls: Vec<String> = rec
                    .get("redirect_urls")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                if redirect_urls.is_empty() {
                    risks.push(format!(
                        "No redirect_urls for candidate {} (keeper: {})",
                        output["candidate_id"].as_str().unwrap_or("?"),
                        keep_url
                    ));
                }

                let mut rec = rec.clone();
                if !rec.contains_key("confidence") {
                    rec.insert(
                        "confidence".to_string(),
                        serde_json::Value::String("medium".to_string()),
                    );
                }
                // Defensive fallback: ensure unique cluster_id from candidate_id.
                let has_valid_cluster_id = rec
                    .get("cluster_id")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if !has_valid_cluster_id {
                    rec.insert("cluster_id".to_string(), output["candidate_id"].clone());
                }

                merge_recommendations.push(serde_json::Value::Object(rec));
            }
        }
    }

    // Deduplicate merge recommendations by cluster_id. The agentic step can
    // return the same cluster_id for multiple candidates; without dedup the
    // frontend renders duplicate React keys and the approval/task-creation
    // flow treats them as a single recommendation, causing UI bugs.
    {
        let mut seen = std::collections::HashSet::new();
        merge_recommendations.retain(|rec| {
            let id = rec.get("cluster_id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() {
                return false;
            }
            seen.insert(id.to_string())
        });
    }

    let hub_recommendations: Vec<serde_json::Value> = hub_gaps_doc["hub_gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|gap| {
            serde_json::json!({
                "topic": gap["theme"],
                "suggested_url": gap["suggested_url"],
                "suggested_title": gap["suggested_title"],
                "spoke_pages": gap["spoke_pages"].as_array().map(|arr| {
                    arr.iter().filter_map(|p| p["id"].as_i64()).collect::<Vec<i64>>()
                }).unwrap_or_default(),
                "outline_suggestion": "",
                "reason": gap["reason"],
                "deterministic": true,
            })
        })
        .collect();

    let strategy = serde_json::json!({
        "generated_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "merge_recommendations": merge_recommendations,
        "hub_recommendations": hub_recommendations,
        "risks": risks,
    });

    let strategy_path = paths.automation_dir.join("cannibalization_strategy.json");
    // Delete any stale strategy file before writing the new one. This prevents
    // old duplicate recommendations from persisting if a previous audit run
    // produced a larger strategy and the current run produces fewer.
    let _ = std::fs::remove_file(&strategy_path);
    if let Err(e) = std::fs::write(
        &strategy_path,
        serde_json::to_string_pretty(&strategy).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write strategy file: {}",
            e
        );
    }

    StepResult {
        success: true,
        message: format!(
            "Strategy reduced: {} merge recommendations, {} hub recommendations, {} risks",
            merge_recommendations.len(),
            hub_recommendations.len(),
            risks.len()
        ),
        output: Some(serde_json::to_string_pretty(&strategy).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 5: Create Fix Tasks
// ═══════════════════════════════════════════════════════════════════════════════

/// No longer auto-spawns destructive fix tasks.
///
/// Phase 2 requires explicit approval via the review UI before any merge
/// or hub tasks are created. The strategy is persisted as an
/// artifact and in `cannibalization_strategy.json` for review.
pub(crate) fn create_can_fix_tasks(
    _conn: &Connection,
    parent_task: &Task,
    _project_path: &str,
) -> Vec<String> {
    log::info!(
        "[cannibalization_audit] Task {} completed. Review required before spawning fix tasks.",
        parent_task.id
    );
    Vec::new()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("can_audit_test_{}_{}", std::process::id(), n))
            .to_string_lossy()
            .to_string()
    }

    fn setup_project(path: &str) {
        let _ = std::fs::remove_dir_all(path);
        let auto_dir = Path::new(path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "best-stocks-csp",
                    "title": "Best Stocks for Cash-Secured Puts",
                    "target_keyword": "cash secured puts",
                    "file": "content/001_best_stocks_csp.mdx",
                    "gsc": { "impressions": 45000.0, "clicks": 120.0, "ctr": 0.0027, "avg_position": 5.5 }
                },
                {
                    "id": 2,
                    "url_slug": "csp-strategy-explained",
                    "title": "Cash-Secured Puts Strategy Explained",
                    "target_keyword": "cash secured puts",
                    "file": "content/002_csp_strategy.mdx",
                    "gsc": { "impressions": 1200.0, "clicks": 5.0, "ctr": 0.0042, "avg_position": 8.2 }
                },
                {
                    "id": 3,
                    "url_slug": "covered-calls-guide",
                    "title": "Covered Calls Complete Guide",
                    "target_keyword": "covered calls",
                    "file": "content/003_covered_calls.mdx",
                    "gsc": { "impressions": 8000.0, "clicks": 30.0, "ctr": 0.0038, "avg_position": 6.1 }
                },
                {
                    "id": 4,
                    "url_slug": "csp-beginners-guide",
                    "title": "Cash-Secured Puts for Beginners",
                    "target_keyword": "cash secured puts",
                    "file": "content/004_csp_beginners.mdx",
                    "gsc": { "impressions": 500.0, "clicks": 2.0, "ctr": 0.004, "avg_position": 12.0 }
                }
            ]
        });
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let mdx1 = r#"---
title: "Best Stocks for Cash-Secured Puts"
date: "2024-01-01"
---

# Best Stocks for Cash-Secured Puts

This article covers the best stocks for cash secured puts strategy in 2024.

## Criteria

We look for stable blue chip stocks with weekly options.
"#;
        std::fs::write(content_dir.join("001_best_stocks_csp.mdx"), mdx1).unwrap();

        let mdx2 = r#"---
title: "Cash-Secured Puts Strategy Explained"
date: "2024-01-02"
---

# Cash-Secured Puts Strategy Explained

This article covers the cash secured puts strategy for beginners looking for the best stocks.

## How It Works

You sell put options while holding cash to buy the stock if assigned.
"#;
        std::fs::write(content_dir.join("002_csp_strategy.mdx"), mdx2).unwrap();

        let mdx3 = r#"---
title: "Covered Calls Complete Guide"
date: "2024-01-03"
---

# Covered Calls Complete Guide

This guide covers covered calls strategy for income generation.

## Basics

You sell call options against stock you already own.
"#;
        std::fs::write(content_dir.join("003_covered_calls.mdx"), mdx3).unwrap();

        let mdx4 = r#"---
title: "Cash-Secured Puts for Beginners"
date: "2024-01-04"
---

# Cash-Secured Puts for Beginners

Learn the basics of cash secured puts and how to find the best stocks for this income strategy.

## Introduction

Cash secured puts are a great way to generate income.
"#;
        std::fs::write(content_dir.join("004_csp_beginners.mdx"), mdx4).unwrap();
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn test_cosine_similarity_range() {
        let a = TfIdfVector {
            weights: [("apple".to_string(), 1.0), ("banana".to_string(), 1.0)]
                .into_iter()
                .collect(),
            norm: (2.0f64).sqrt(),
        };
        let b = TfIdfVector {
            weights: [("apple".to_string(), 1.0), ("banana".to_string(), 1.0)]
                .into_iter()
                .collect(),
            norm: (2.0f64).sqrt(),
        };
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = TfIdfVector {
            weights: [("cherry".to_string(), 1.0)].into_iter().collect(),
            norm: 1.0,
        };
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[test]
    fn test_exec_can_build_context() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test Cannibalization Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_build_context(&task, &path);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();

        // Compact summary shape
        assert_eq!(output["summary"]["total_articles"].as_i64().unwrap(), 4);
        assert!(output["summary"]["total_impressions"].as_f64().unwrap() > 0.0);
        assert_eq!(output["summary"]["candidate_clusters"].as_i64().unwrap(), 1);
        assert!(output["summary"]["hub_gaps"].as_i64().unwrap() >= 1);

        // Artifact paths
        assert!(output["artifact_paths"]["context"]
            .as_str()
            .unwrap()
            .contains("cannibalization_audit_context.json"));
        assert!(output["artifact_paths"]["clusters"]
            .as_str()
            .unwrap()
            .contains("cannibalization_clusters.json"));

        // Full artifacts should still be written to disk
        let auto_dir = Path::new(&path).join(".github").join("automation");
        assert!(auto_dir.join("cannibalization_audit_context.json").exists());
        assert!(auto_dir.join("cannibalization_clusters.json").exists());
        assert!(auto_dir.join("hub_gaps.json").exists());
        // Verify clusters artifact has the expected content
        let clusters_content =
            std::fs::read_to_string(auto_dir.join("cannibalization_clusters.json")).unwrap();
        let clusters_doc: serde_json::Value = serde_json::from_str(&clusters_content).unwrap();
        let clusters = clusters_doc["clusters"].as_array().unwrap();
        assert!(!clusters.is_empty());
        let csp_cluster = clusters.iter().find(|c| {
            c["theme"]
                .as_str()
                .unwrap_or("")
                .contains("cash secured puts")
        });
        assert!(csp_cluster.is_some());
        assert_eq!(csp_cluster.unwrap()["pages"].as_array().unwrap().len(), 3);

        cleanup(&path);
    }

    #[test]
    fn test_missing_gsc_data_graceful() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        // Articles with NO gsc data
        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "article-one",
                    "title": "Article One",
                    "target_keyword": "keyword one",
                    "file": "content/article_one.mdx"
                },
                {
                    "id": 2,
                    "url_slug": "article-two",
                    "title": "Article Two",
                    "target_keyword": "keyword one",
                    "file": "content/article_two.mdx"
                }
            ]
        });
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let mdx = r#"---
title: "Article"
---

# Article

Some content here.
"#;
        std::fs::write(content_dir.join("article_one.mdx"), mdx).unwrap();
        std::fs::write(content_dir.join("article_two.mdx"), mdx).unwrap();

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_build_context(&task, &path);
        assert!(
            result.success,
            "Should succeed even with missing GSC data: {}",
            result.message
        );

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        // Articles without GSC data are filtered out from clustering
        assert_eq!(output["summary"]["total_articles"].as_i64().unwrap(), 0);
        assert_eq!(
            output["summary"]["total_impressions"].as_f64().unwrap(),
            0.0
        );
        assert_eq!(output["summary"]["candidate_clusters"].as_i64().unwrap(), 0);

        cleanup(&path);
    }

    #[test]
    fn test_hub_gap_detection() {
        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "best-stocks-csp".to_string(),
                title: "Best Stocks for CSP".to_string(),
                h1: "Best Stocks for CSP".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 10000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-01".to_string(),
                word_count: 100,
                page_type: None,
            },
            ArticleRecord {
                id: 2,
                url_slug: "csp-strategy".to_string(),
                title: "CSP Strategy".to_string(),
                h1: "CSP Strategy".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 5000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-02".to_string(),
                word_count: 100,
                page_type: None,
            },
            ArticleRecord {
                id: 3,
                url_slug: "csp-beginners".to_string(),
                title: "CSP Beginners".to_string(),
                h1: "CSP Beginners".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "c.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 3000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-03".to_string(),
                word_count: 100,
                page_type: None,
            },
            ArticleRecord {
                id: 4,
                url_slug: "hub/cash-secured-puts".to_string(),
                title: "Hub CSP".to_string(),
                h1: "Hub CSP".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "d.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 20000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-04".to_string(),
                word_count: 100,
                page_type: None,
            },
        ];

        let clusters = build_clusters(
            &records,
            &[
                (0, 1, 0.5),
                (1, 2, 0.5),
                (0, 2, 0.5),
                (0, 3, 0.5),
                (1, 3, 0.5),
                (2, 3, 0.5),
            ],
            None,
            "",
        );
        let gaps = detect_hub_gaps(&records, &clusters, None, "");

        // Cluster includes hub page (id 4), so no gap should be reported
        assert!(
            gaps.is_empty(),
            "Should not report hub gap when hub exists in cluster"
        );
    }

    #[test]
    fn test_compute_query_overlap_with_db_data() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();

        let project_id = "proj-overlap";

        // Insert required project row (FK constraint)
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode) VALUES (?1, ?2, ?3, 1, 'workspace')",
            rusqlite::params![project_id, "Test", "/tmp"],
        ).unwrap();

        // Insert query metrics for 3 articles
        // Article 1: queries A, B, C
        crate::db::set_ctr_query_metrics(
            &conn,
            project_id,
            1,
            "/a",
            &[
                ("query a".to_string(), 100.0, 1.0, 0.01, 5.0, None),
                ("query b".to_string(), 80.0, 1.0, 0.01, 6.0, None),
                ("query c".to_string(), 60.0, 1.0, 0.01, 7.0, None),
            ],
            Some("2026-01-01"),
            Some("2026-03-31"),
        )
        .unwrap();

        // Article 2: queries B, C, D
        crate::db::set_ctr_query_metrics(
            &conn,
            project_id,
            2,
            "/b",
            &[
                ("query b".to_string(), 90.0, 1.0, 0.01, 4.0, None),
                ("query c".to_string(), 70.0, 1.0, 0.01, 5.0, None),
                ("query d".to_string(), 50.0, 1.0, 0.01, 8.0, None),
            ],
            Some("2026-01-01"),
            Some("2026-03-31"),
        )
        .unwrap();

        // Article 3: queries C, D, E (no overlap with article 1 except C)
        crate::db::set_ctr_query_metrics(
            &conn,
            project_id,
            3,
            "/c",
            &[
                ("query c".to_string(), 85.0, 1.0, 0.01, 3.0, None),
                ("query d".to_string(), 65.0, 1.0, 0.01, 6.0, None),
                ("query e".to_string(), 45.0, 1.0, 0.01, 9.0, None),
            ],
            Some("2026-01-01"),
            Some("2026-03-31"),
        )
        .unwrap();

        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "a".to_string(),
                title: "A".to_string(),
                h1: "A".to_string(),
                target_keyword: "kw".to_string(),
                first_200_words: "".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
            ArticleRecord {
                id: 2,
                url_slug: "b".to_string(),
                title: "B".to_string(),
                h1: "B".to_string(),
                target_keyword: "kw".to_string(),
                first_200_words: "".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
            ArticleRecord {
                id: 3,
                url_slug: "c".to_string(),
                title: "C".to_string(),
                h1: "C".to_string(),
                target_keyword: "kw".to_string(),
                first_200_words: "".to_string(),
                file: "c.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
        ];

        let indices = vec![0, 1, 2];
        let (count, top) = compute_query_overlap(Some(&conn), project_id, &records, &indices);

        // Pairwise overlaps: (A,B)=B,C; (A,C)=C; (B,C)=C,D
        // Union = B, C, D = 3 queries
        assert_eq!(count, 3, "Should find 3 shared queries (B, C, D)");
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn test_compute_query_overlap_fallback_to_proxy() {
        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "a".to_string(),
                title: "A".to_string(),
                h1: "A".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
            ArticleRecord {
                id: 2,
                url_slug: "b".to_string(),
                title: "B".to_string(),
                h1: "B".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
        ];

        let indices = vec![0, 1];
        // No DB connection — should fall back to target_keyword proxy
        let (count, top) = compute_query_overlap(None, "proj", &records, &indices);

        assert_eq!(count, 1, "Proxy should find 1 distinct target_keyword");
        assert_eq!(top[0], "cash secured puts");
    }

    #[test]
    fn test_can_select_candidates_produces_merge_candidates() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let build_result = exec_can_build_context(&task, &path);
        assert!(build_result.success);

        let select_result = exec_can_select_candidates(&task, &path);
        assert!(
            select_result.success,
            "select_candidates failed: {}",
            select_result.message
        );

        let auto_dir = Path::new(&path).join(".github").join("automation");
        assert!(auto_dir.join("cannibalization_candidates.json").exists());

        let candidates_doc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(auto_dir.join("cannibalization_candidates.json")).unwrap(),
        )
        .unwrap();
        let candidates = candidates_doc["candidates"].as_array().unwrap();
        assert!(
            !candidates.is_empty(),
            "Should produce at least one candidate"
        );

        // All candidates should be merge candidates with ≤8 pages
        for c in candidates {
            assert_eq!(c["candidate_type"].as_str().unwrap(), "merge_candidate");
            assert!(c["pages"].as_array().unwrap().len() <= 8);
            assert!(c["total_impressions"].as_f64().unwrap() >= 0.0);
        }

        cleanup(&path);
    }

    #[test]
    fn test_can_reduce_strategy_merges_batch_outputs() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        // Write fake batch outputs
        let batch_doc = serde_json::json!({
            "batch_outputs": [
                {
                    "candidate_id": "test_0",
                    "success": true,
                    "message": "ok",
                    "merge_recommendation": {
                        "cluster_theme": "cash secured puts",
                        "keep_url": "/blog/best-stocks-csp",
                        "redirect_urls": ["/blog/csp-strategy-explained"],
                        "merge_instructions": "Merge content",
                        "reason": "Higher impressions",
                        "confidence": "high"
                    }
                },
                {
                    "candidate_id": "test_1",
                    "success": true,
                    "message": "ok",
                    "merge_recommendation": {
                        "no_action": true,
                        "reason": "Topical overlap only"
                    }
                },
                {
                    "candidate_id": "test_2",
                    "success": false,
                    "message": "Agent error"
                }
            ]
        });
        std::fs::write(
            auto_dir.join("cannibalization_batch_outputs.json"),
            serde_json::to_string_pretty(&batch_doc).unwrap(),
        )
        .unwrap();

        // Write minimal hub gaps
        let hub_doc = serde_json::json!({
            "hub_gaps": [
                {
                    "theme": "cash secured puts",
                    "suggested_url": "/hub/cash-secured-puts",
                    "suggested_title": "Cash Secured Puts: Complete Guide",
                    "spoke_pages": [{"id": 1, "url": "/blog/a", "title": "A"}],
                    "reason": "No hub exists"
                }
            ]
        });
        std::fs::write(
            auto_dir.join("hub_gaps.json"),
            serde_json::to_string_pretty(&hub_doc).unwrap(),
        )
        .unwrap();

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_reduce_strategy(&task, &path);
        assert!(result.success, "reduce_strategy failed: {}", result.message);

        let strategy_path = auto_dir.join("cannibalization_strategy.json");
        assert!(strategy_path.exists());

        let strategy: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&strategy_path).unwrap()).unwrap();

        // Should include the one valid merge recommendation (test_0)
        let merges = strategy["merge_recommendations"].as_array().unwrap();
        assert_eq!(merges.len(), 1);
        assert_eq!(
            merges[0]["keep_url"].as_str().unwrap(),
            "/blog/best-stocks-csp"
        );
        assert_eq!(merges[0]["confidence"].as_str().unwrap(), "high");

        // Should include hub from deterministic data
        let hubs = strategy["hub_recommendations"].as_array().unwrap();
        assert_eq!(hubs.len(), 1);

        // Should record the failed candidate as a risk
        let risks = strategy["risks"].as_array().unwrap();
        assert!(risks.iter().any(|r| r.as_str().unwrap().contains("test_2")));

        cleanup(&path);
    }

    #[test]
    fn test_merge_prompt_budget_and_trim() {
        let skill = "# Skill\n\nSome instructions here.".to_string();
        let candidate = serde_json::json!({
            "candidate_id": "test",
            "pages": [
                {
                    "id": 1,
                    "title": "Page 1",
                    "excerpt": "word ".repeat(100)
                },
                {
                    "id": 2,
                    "title": "Page 2",
                    "excerpt": "word ".repeat(100)
                }
            ]
        });

        let (full_prompt, full_bytes) = build_merge_prompt(&skill, &candidate);
        let (trimmed_prompt, trimmed_bytes) = build_merge_prompt_trimmed(&skill, &candidate);

        // Trimmed prompt should be smaller because excerpts are removed
        assert!(
            trimmed_bytes < full_bytes,
            "Trimmed prompt should be smaller: {} < {}",
            trimmed_bytes,
            full_bytes
        );
        assert!(!trimmed_prompt.contains("excerpt"));
        assert!(full_prompt.contains("excerpt"));
    }
}
