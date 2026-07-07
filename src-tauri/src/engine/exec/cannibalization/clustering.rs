//! Connected-component clustering via Union-Find.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Clustering
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute real query overlap between articles in a cluster.
/// Uses `ctr_query_metrics` when available; falls back to target_keyword proxy otherwise.
pub(crate) fn compute_query_overlap(
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
pub(crate) fn build_clusters(
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
pub(crate) fn slugify(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}
