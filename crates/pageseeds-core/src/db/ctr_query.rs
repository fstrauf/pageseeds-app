//! Shared CTR query-metric primitives (desk + cannibalization audit).
//!
//! Single source of truth for:
//! - hard same-query grouping constants and pure grouper
//! - project-wide `ctr_query_metrics` load
//!
//! Used by Site State desk builders and cannibalization `shared_query` lane
//! (PR #214 review / issue #204). Keeps `engine/exec` and `engine/site_state`
//! from dual-implementing the same floor/cap/group rules.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;

/// Per-article query impression floor for hard same-query multi-URL groups.
pub const SHARED_QUERY_MIN_IMPRESSIONS: f64 = 10.0;

/// Max pages/slugs listed per shared-query group (matches candidate page cap).
pub const SHARED_QUERY_MAX_PAGES: usize = 4;

/// Lightweight project-wide `ctr_query_metrics` row for inventory joins.
#[derive(Debug, Clone)]
pub struct CtrQueryRow {
    pub article_id: i64,
    pub query: String,
    pub impressions: f64,
    pub clicks: f64,
}

/// Pure hard same-query grouper.
///
/// Input rows are `(query, article_id, impressions)`. Callers attach clicks,
/// page URLs, or slugs after grouping.
///
/// Behavior:
/// - lowercases query keys
/// - drops rows below [`SHARED_QUERY_MIN_IMPRESSIONS`]
/// - keeps the highest-impressions row per `(query, article_id)`
/// - requires ≥2 distinct article_ids
/// - sorts groups by total impressions desc
/// - sorts pages within a group by impressions desc
/// - caps each group at [`SHARED_QUERY_MAX_PAGES`] (re-checks ≥2 after cap)
pub fn group_shared_query_articles(
    rows: impl IntoIterator<Item = (String, i64, f64)>,
) -> Vec<(String, Vec<(i64, f64)>)> {
    let mut by_query: HashMap<String, HashMap<i64, f64>> = HashMap::new();
    for (query, article_id, impressions) in rows {
        if impressions < SHARED_QUERY_MIN_IMPRESSIONS {
            continue;
        }
        let q_lower = query.to_lowercase();
        let entry = by_query.entry(q_lower).or_default();
        entry
            .entry(article_id)
            .and_modify(|imp| {
                if impressions > *imp {
                    *imp = impressions;
                }
            })
            .or_insert(impressions);
    }

    let mut groups: Vec<(String, Vec<(i64, f64)>)> = by_query
        .into_iter()
        .filter_map(|(query, by_article)| {
            if by_article.len() < 2 {
                return None;
            }
            let mut pages: Vec<(i64, f64)> = by_article.into_iter().collect();
            pages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            pages.truncate(SHARED_QUERY_MAX_PAGES);
            if pages.len() < 2 {
                return None;
            }
            Some((query, pages))
        })
        .collect();

    groups.sort_by(|a, b| {
        let ta: f64 = a.1.iter().map(|(_, imp)| *imp).sum();
        let tb: f64 = b.1.iter().map(|(_, imp)| *imp).sum();
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });
    groups
}

/// Load all `ctr_query_metrics` rows for a project (article_id, query, impressions, clicks).
pub fn list_ctr_query_metrics_for_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<CtrQueryRow>> {
    let mut stmt = conn.prepare(
        "SELECT article_id, query, impressions, clicks
         FROM ctr_query_metrics
         WHERE project_id = ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        Ok(CtrQueryRow {
            article_id: row.get(0)?,
            query: row.get(1)?,
            impressions: row.get(2)?,
            clicks: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_shared_query_articles_respects_floor_cardinality_and_sort() {
        let rows = vec![
            ("Best Cold Brew".into(), 1_i64, 100.0),
            ("best cold brew".into(), 2, 50.0),
            ("best cold brew".into(), 3, 5.0), // below floor
            ("solo query".into(), 1, 200.0),  // only one page
            ("monthly sub".into(), 10, 40.0),
            ("monthly sub".into(), 11, 30.0),
            // total imp cold brew (150) > monthly (70) → cold brew first
        ];
        let groups = group_shared_query_articles(rows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "best cold brew");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[0].1[0], (1, 100.0));
        assert_eq!(groups[1].0, "monthly sub");
        assert!(!groups.iter().any(|(q, _)| q == "solo query"));
    }

    #[test]
    fn group_shared_query_articles_keeps_max_impressions_per_article() {
        let rows = vec![
            ("q".into(), 1_i64, 20.0),
            ("q".into(), 1, 80.0), // same article, higher imp wins
            ("q".into(), 2, 30.0),
        ];
        let groups = group_shared_query_articles(rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1, vec![(1, 80.0), (2, 30.0)]);
    }

    #[test]
    fn group_shared_query_articles_caps_pages_per_group() {
        let rows: Vec<(String, i64, f64)> = (1..=6)
            .map(|i| ("crowded".into(), i, 100.0 - i as f64))
            .collect();
        let groups = group_shared_query_articles(rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), SHARED_QUERY_MAX_PAGES);
        assert_eq!(groups[0].1[0].0, 1);
        assert_eq!(groups[0].1[3].0, 4);
    }
}
