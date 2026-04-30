/// Keyword cannibalization audit execution module.
///
/// Covers:
///   - exec_can_build_context   (deterministic TF-IDF clustering + link graph + hub gaps + territory analysis)
///   - exec_can_analyze         (agentic analysis with cannibalization-strategy skill)
///   - create_can_fix_tasks     (spawn follow-up fix tasks)
use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::models::task::{Task, TaskReviewSurface, FollowUpPolicy};

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
        let word_count = first_200_words.split_whitespace().count();

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
        });
    }

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
    let hub_gaps = detect_hub_gaps(&records, &clusters);

    // ── 8. Analyse territories ────────────────────────────────────────────────
    let territory_analysis = analyze_territories(&records);

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
                        "url": format!("/blog/{}", r.url_slug),
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

    let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
    let clusters_doc = serde_json::json!({
        "generated_at": &now_iso,
        "clusters": &clusters_json,
    });
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

    let territory_path = paths.automation_dir.join("territory_analysis.json");
    let territory_doc = serde_json::json!({
        "generated_at": &now_iso,
        "territory_analysis": &territory_analysis,
    });
    if let Err(e) = std::fs::write(
        &territory_path,
        serde_json::to_string_pretty(&territory_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write territory_analysis.json: {}",
            e
        );
    }

    // ── 12. Build full structured cluster context for the agent ───────────────
    let agent_context = serde_json::json!({
        "site_summary": {
            "total_pages": articles_json.len(),
            "total_impressions": total_impressions,
            "period_days": period_days,
        },
        "clusters": clusters_json,
        "hub_gaps": hub_gaps,
        "territory_analysis": territory_analysis,
        "calculator_opportunities": {},
    });
    let agent_context_str = serde_json::to_string_pretty(&agent_context).unwrap_or_default() + "\n";

    StepResult {
        success: true,
        message: format!(
            "Cannibalization context built: {} articles, {} similar pairs, {} keyword groups, {} clusters, {} hub gaps, {} territories",
            articles_json.len(),
            similarity_pairs.len(),
            keyword_groups_json.len(),
            clusters.len(),
            hub_gaps.len(),
            territory_analysis["saturated_themes"].as_array().map(|a| a.len()).unwrap_or(0) +
                territory_analysis["open_territories"].as_array().map(|a| a.len()).unwrap_or(0),
        ),
        output: Some(agent_context_str),
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
            project_id: String::new(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
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
fn detect_hub_gaps(records: &[ArticleRecord], clusters: &[Cluster]) -> Vec<serde_json::Value> {
    // Find existing hub-like pages across the whole site.
    // Recognise both URL-path prefixes (hub/) and slug prefixes (hub_).
    let existing_hubs: HashSet<String> = records
        .iter()
        .filter(|r| {
            let slug = &r.url_slug;
            slug.starts_with("hub/")
                || slug.starts_with("guide/")
                || slug.starts_with("hub_")
                || slug.starts_with("guide_")
        })
        .flat_map(|r| {
            let mut topics = Vec::new();
            // 1. target_keyword if populated
            let kw = r.target_keyword.trim().to_lowercase();
            if !kw.is_empty() {
                topics.push(kw);
            }
            // 2. Derive topic from slug by stripping prefix
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
            let stripped = stripped.trim().replace('_', " ").to_lowercase();
            if !stripped.is_empty() {
                topics.push(stripped);
            }
            // 3. Fall back to title words (without common suffixes)
            let title = r.title.trim().to_lowercase();
            let title_topic = title
                .trim_end_matches(": complete guide")
                .trim_end_matches(": the complete guide")
                .trim_end_matches(" complete guide")
                .trim()
                .to_string();
            if !title_topic.is_empty() && title_topic != title {
                topics.push(title_topic);
            } else if !title.is_empty() {
                topics.push(title);
            }
            topics
        })
        .collect();

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
                    "url": format!("/blog/{}", r.url_slug),
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
// Territory analysis
// ═══════════════════════════════════════════════════════════════════════════════

/// Analyse content coverage to find saturated themes and open territories.
fn analyze_territories(records: &[ArticleRecord]) -> serde_json::Value {
    let mut theme_counts: HashMap<String, Vec<i64>> = HashMap::new();
    for r in records {
        let kw = r.target_keyword.trim().to_lowercase();
        if kw.is_empty() {
            continue;
        }
        theme_counts.entry(kw).or_default().push(r.id);
    }

    let mut saturated_themes: Vec<serde_json::Value> = Vec::new();
    let mut open_territories: Vec<serde_json::Value> = Vec::new();

    for (theme, ids) in &theme_counts {
        let total_impressions: f64 = ids
            .iter()
            .filter_map(|&id| records.iter().find(|r| r.id == id))
            .map(|r| r.gsc["impressions"].as_f64().unwrap_or(0.0))
            .sum();

        if ids.len() > 5 {
            saturated_themes.push(serde_json::json!({
                "theme": theme,
                "article_count": ids.len(),
                "total_impressions": total_impressions,
                "reason": "More than 5 articles target the same narrow theme.",
            }));
        } else if ids.len() <= 1 {
            // Only flag as open if it has some impressions (evidence of demand)
            if total_impressions > 1000.0 {
                open_territories.push(serde_json::json!({
                    "theme": theme,
                    "article_count": ids.len(),
                    "total_impressions": total_impressions,
                    "reason": "Low coverage but existing impressions suggest demand.",
                }));
            }
        }
    }

    // Sort by total impressions descending
    saturated_themes.sort_by(|a, b| {
        let ta = a["total_impressions"].as_f64().unwrap_or(0.0);
        let tb = b["total_impressions"].as_f64().unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });
    open_territories.sort_by(|a, b| {
        let ta = a["total_impressions"].as_f64().unwrap_or(0.0);
        let tb = b["total_impressions"].as_f64().unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });

    serde_json::json!({
        "saturated_themes": saturated_themes,
        "open_territories": open_territories,
        "total_themes": theme_counts.len(),
    })
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
// Step 2: Analyze
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the cannibalization strategy analysis using an LLM agent.
///
/// Loads the "cannibalization-strategy" skill, builds a prompt with the skill
/// content and the provided structured cluster context, and delegates to the agent.
pub(crate) fn exec_can_analyze(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "cannibalization-strategy") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message:
                    "Skill 'cannibalization-strategy' not found in .github/skills/ or app defaults"
                        .to_string(),
                output: None,
            };
        }
    };

    // Use string concatenation to avoid format! panics if skill content contains { or }
    let prompt = skill.content
        + "\n\n---\n\n## Cannibalization Audit Context\n\n"
        + context_json
        + "\n\nPlease analyze the above context and provide a cannibalization resolution strategy."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the Output Contract above."
        + " Do not include markdown prose, summaries, tables, or explanations outside the JSON."
        + " Do not write files. Output the JSON directly in your response.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            // Extract JSON if present so downstream steps receive clean structured data
            let mut value = crate::engine::text::extract_json(&output)
                .unwrap_or_else(|| serde_json::Value::String(output));

            // Inject generated_at if missing so deserialization always succeeds
            if let serde_json::Value::Object(ref mut map) = value {
                if !map.contains_key("generated_at") {
                    map.insert(
                        "generated_at".to_string(),
                        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                    );
                }
            }

            let final_output = serde_json::to_string_pretty(&value).unwrap_or_default();

            // Also write to automation dir so the file fallback works
            let paths = ProjectPaths::from_path(project_path);
            let strategy_path = paths.automation_dir.join("cannibalization_strategy.json");
            if let Err(e) = std::fs::create_dir_all(&paths.automation_dir) {
                log::warn!(
                    "[cannibalization_audit] Failed to create automation dir: {}",
                    e
                );
            } else if let Err(e) = std::fs::write(&strategy_path, &final_output) {
                log::warn!(
                    "[cannibalization_audit] Failed to write strategy file: {}",
                    e
                );
            } else {
                log::info!(
                    "[cannibalization_audit] Wrote strategy to {:?}",
                    strategy_path
                );
            }

            StepResult {
                success: true,
                message: "Cannibalization analysis completed".to_string(),
                output: Some(final_output),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error during cannibalization analysis: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Create Fix Tasks
// ═══════════════════════════════════════════════════════════════════════════════

/// No longer auto-spawns destructive fix tasks.
///
/// Phase 2 requires explicit approval via the review UI before any merge,
/// hub, or territory tasks are created. The strategy is persisted as an
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
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_build_context(&task, &path);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();

        // Site summary
        assert_eq!(output["site_summary"]["total_pages"].as_i64().unwrap(), 4);
        assert!(
            output["site_summary"]["total_impressions"]
                .as_f64()
                .unwrap()
                > 0.0
        );

        // Clusters
        let clusters = output["clusters"].as_array().unwrap();
        assert!(!clusters.is_empty(), "Should find at least one cluster");

        // The cash-secured-puts cluster should have 3 articles
        let csp_cluster = clusters.iter().find(|c| {
            c["theme"]
                .as_str()
                .unwrap_or("")
                .contains("cash secured puts")
        });
        assert!(
            csp_cluster.is_some(),
            "Should find cash secured puts cluster"
        );
        let csp_cluster = csp_cluster.unwrap();
        assert_eq!(csp_cluster["pages"].as_array().unwrap().len(), 3);
        assert!(csp_cluster["total_impressions"].as_f64().unwrap() > 46000.0);

        // Shared query overlap (falls back to target_keyword proxy in tests without DB query data)
        assert_eq!(csp_cluster["shared_query_count"].as_i64().unwrap(), 1);
        let top_queries = csp_cluster["top_shared_queries"].as_array().unwrap();
        assert!(!top_queries.is_empty());
        assert!(top_queries[0]
            .as_str()
            .unwrap()
            .contains("cash secured puts"));

        // Hub gaps
        let hub_gaps = output["hub_gaps"].as_array().unwrap();
        assert!(
            !hub_gaps.is_empty(),
            "Should detect hub gaps for 3+ article clusters"
        );

        // Territory analysis
        let territory = &output["territory_analysis"];
        assert!(territory["saturated_themes"].is_array());
        assert!(territory["open_territories"].is_array());

        // Artifacts should be written
        let auto_dir = Path::new(&path).join(".github").join("automation");
        assert!(auto_dir.join("cannibalization_audit_context.json").exists());
        assert!(auto_dir.join("cannibalization_clusters.json").exists());
        assert!(auto_dir.join("hub_gaps.json").exists());
        assert!(auto_dir.join("territory_analysis.json").exists());

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
        assert_eq!(output["site_summary"]["total_pages"].as_i64().unwrap(), 2);
        assert_eq!(
            output["site_summary"]["total_impressions"]
                .as_f64()
                .unwrap(),
            0.0
        );

        let clusters = output["clusters"].as_array().unwrap();
        assert!(!clusters.is_empty());
        assert_eq!(clusters[0]["total_impressions"].as_f64().unwrap(), 0.0);

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
        let gaps = detect_hub_gaps(&records, &clusters);

        // Cluster includes hub page (id 4), so no gap should be reported
        assert!(
            gaps.is_empty(),
            "Should not report hub gap when hub exists in cluster"
        );
    }

    #[test]
    fn test_territory_analysis() {
        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "a".to_string(),
                title: "A".to_string(),
                h1: "A".to_string(),
                target_keyword: "saturated theme".to_string(),
                first_200_words: "...".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 1000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
            ArticleRecord {
                id: 2,
                url_slug: "b".to_string(),
                title: "B".to_string(),
                h1: "B".to_string(),
                target_keyword: "saturated theme".to_string(),
                first_200_words: "...".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 1000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
            ArticleRecord {
                id: 3,
                url_slug: "c".to_string(),
                title: "C".to_string(),
                h1: "C".to_string(),
                target_keyword: "saturated theme".to_string(),
                first_200_words: "...".to_string(),
                file: "c.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 1000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
            ArticleRecord {
                id: 4,
                url_slug: "d".to_string(),
                title: "D".to_string(),
                h1: "D".to_string(),
                target_keyword: "saturated theme".to_string(),
                first_200_words: "...".to_string(),
                file: "d.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 1000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
            ArticleRecord {
                id: 5,
                url_slug: "e".to_string(),
                title: "E".to_string(),
                h1: "E".to_string(),
                target_keyword: "saturated theme".to_string(),
                first_200_words: "...".to_string(),
                file: "e.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 1000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
            ArticleRecord {
                id: 6,
                url_slug: "f".to_string(),
                title: "F".to_string(),
                h1: "F".to_string(),
                target_keyword: "saturated theme".to_string(),
                first_200_words: "...".to_string(),
                file: "f.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 1000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
            ArticleRecord {
                id: 7,
                url_slug: "g".to_string(),
                title: "G".to_string(),
                h1: "G".to_string(),
                target_keyword: "open territory".to_string(),
                first_200_words: "...".to_string(),
                file: "g.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 5000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 100,
            },
        ];

        let analysis = analyze_territories(&records);
        let saturated = analysis["saturated_themes"].as_array().unwrap();
        let open = analysis["open_territories"].as_array().unwrap();

        assert_eq!(saturated.len(), 1, "Should detect saturated theme");
        assert_eq!(saturated[0]["theme"].as_str().unwrap(), "saturated theme");

        assert_eq!(
            open.len(),
            1,
            "Should detect open territory with impressions"
        );
        assert_eq!(open[0]["theme"].as_str().unwrap(), "open territory");
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
            },
        ];

        let indices = vec![0, 1];
        // No DB connection — should fall back to target_keyword proxy
        let (count, top) = compute_query_overlap(None, "proj", &records, &indices);

        assert_eq!(count, 1, "Proxy should find 1 distinct target_keyword");
        assert_eq!(top[0], "cash secured puts");
    }
}
