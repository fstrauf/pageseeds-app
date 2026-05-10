/// Deterministic territory analysis for keyword research.
///
/// Extracted from the cannibalization audit. Groups articles by target_keyword,
/// uses semantic Jaccard grouping to collapse variations, and identifies:
///   - Open territories: low coverage (≤1 article) + high impressions (≥5k)
///   - Saturated themes: high coverage (>5 articles) competing for same theme
///
/// Results are synced to the `research_shortlist` SQLite table for consumption
/// by the keyword research pipeline.
use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::db::research_shortlist::{upsert_entry, ResearchShortlistEntry};
use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

const OPEN_TERRITORY_IMPRESSION_THRESHOLD: f64 = 5000.0;
const SATURATION_THRESHOLD: usize = 5;
const MAX_OPEN_TERRITORIES: usize = 10;

/// A lightweight article record for territory analysis.
struct ArticleSummary {
    id: i64,
    target_keyword: String,
    gsc_impressions: f64,
}

/// Run territory analysis and sync results to the research_shortlist table.
pub(crate) fn exec_research_territory_analysis(task: &Task, _project_path: &str) -> StepResult {
    let db_path = crate::db::default_db_path();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open DB: {}", e),
                output: None,
            };
        }
    };

    // 1. Load articles + GSC metadata
    let articles = match load_articles_with_gsc(&conn, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load articles: {}", e),
                output: None,
            };
        }
    };

    if articles.is_empty() {
        return StepResult {
            success: true,
            message: "No articles with GSC data found for territory analysis".to_string(),
            output: None,
        };
    }

    // 2. Run analysis
    let analysis = analyze_territories(&articles);

    // 3. Sync open territories to shortlist
    let open_territories = analysis.open_territories.clone();
    let saturated_themes = analysis.saturated_themes.clone();

    let mut synced = 0usize;
    for territory in &open_territories {
        let theme = territory.theme.clone();
        let seeds = territory.source_keywords.clone();
        let entry = ResearchShortlistEntry::new(
            &task.project_id,
            &theme,
            seeds,
            "territory_analysis",
            "high",
            Some(territory.article_count as i64),
            Some(territory.total_impressions),
        );
        match upsert_entry(&conn, &entry) {
            Ok(_) => synced += 1,
            Err(e) => log::warn!("[territory_analysis] Failed to upsert shortlist entry for '{}': {}", theme, e),
        }
    }

    for theme in &saturated_themes {
        let entry = ResearchShortlistEntry::new(
            &task.project_id,
            &theme.theme,
            theme.source_keywords.clone(),
            "territory_analysis",
            "medium",
            Some(theme.article_count as i64),
            Some(theme.total_impressions),
        );
        // Saturated themes get a special status so keyword research can deprioritize them
        let mut saturated_entry = entry;
        saturated_entry.status = "saturated".to_string();
        match upsert_entry(&conn, &saturated_entry) {
            Ok(_) => synced += 1,
            Err(e) => log::warn!("[territory_analysis] Failed to upsert saturated theme '{}': {}", theme.theme, e),
        }
    }

    // 4. Prune old covered entries
    let _ = crate::db::research_shortlist::prune_covered(&conn, &task.project_id, 30);

    // 5. Return summary
    let output = serde_json::json!({
        "open_territories": open_territories,
        "saturated_themes": saturated_themes,
        "total_themes": analysis.total_themes,
        "synced_to_shortlist": synced,
    });

    StepResult {
        success: true,
        message: format!(
            "Territory analysis: {} open territories, {} saturated themes, {} synced to shortlist",
            open_territories.len(),
            saturated_themes.len(),
            synced
        ),
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data loading
// ═══════════════════════════════════════════════════════════════════════════════

fn load_articles_with_gsc(
    conn: &Connection,
    project_id: &str,
) -> crate::error::Result<Vec<ArticleSummary>> {
    // Load all articles
    let articles = crate::engine::task_store::list_articles(conn, project_id)?;

    // Load GSC metadata in bulk (graceful if article_metadata table doesn't exist yet)
    let metadata = crate::db::list_project_metadata(conn, project_id).unwrap_or_default();
    let mut gsc_by_article: HashMap<i64, serde_json::Value> = HashMap::new();
    for (article_id, namespace, payload) in metadata {
        if namespace == "gsc" {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload) {
                gsc_by_article.insert(article_id, json);
            }
        }
    }

    let mut summaries = Vec::new();
    for article in articles {
        let gsc = gsc_by_article.get(&article.id);
        let impressions = gsc
            .and_then(|v| v["impressions"].as_f64())
            .unwrap_or(0.0);

        let kw = article.target_keyword.unwrap_or_default();
        summaries.push(ArticleSummary {
            id: article.id,
            target_keyword: kw,
            gsc_impressions: impressions,
        });
    }

    Ok(summaries)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Analysis
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize)]
pub struct TerritoryTheme {
    pub theme: String,
    pub article_count: usize,
    pub total_impressions: f64,
    pub source_keywords: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TerritoryAnalysis {
    pub open_territories: Vec<TerritoryTheme>,
    pub saturated_themes: Vec<TerritoryTheme>,
    pub total_themes: usize,
}

fn analyze_territories(articles: &[ArticleSummary]) -> TerritoryAnalysis {
    // Raw grouping by exact target_keyword
    let mut raw_groups: HashMap<String, Vec<i64>> = HashMap::new();
    for article in articles {
        let kw = article.target_keyword.trim().to_lowercase();
        if kw.is_empty() {
            continue;
        }
        raw_groups.entry(kw).or_default().push(article.id);
    }

    // Semantic grouping: merge keywords that are canonical duplicates or high Jaccard overlap
    let mut merged_groups: HashMap<String, (Vec<i64>, Vec<String>)> = HashMap::new();

    for (kw, ids) in raw_groups {
        let canonical = canonical_keyword(&kw);
        let mut merged = false;

        for (rep, (existing_ids, existing_kws)) in merged_groups.iter_mut() {
            if keyword_jaccard(&kw, rep) > 0.5 {
                existing_ids.extend(ids.clone());
                existing_kws.push(kw.clone());
                merged = true;
                break;
            }
        }

        if !merged {
            merged_groups.insert(canonical, (ids, vec![kw]));
        }
    }

    // Deduplicate article IDs within each merged group
    for (ids, _) in merged_groups.values_mut() {
        ids.sort_unstable();
        ids.dedup();
    }

    let mut open_territories: Vec<TerritoryTheme> = Vec::new();
    let mut saturated_themes: Vec<TerritoryTheme> = Vec::new();

    for (representative, (ids, source_kws)) in &merged_groups {
        let total_impressions: f64 = ids
            .iter()
            .filter_map(|&id| articles.iter().find(|a| a.id == id))
            .map(|a| a.gsc_impressions)
            .sum();

        if ids.len() > SATURATION_THRESHOLD {
            saturated_themes.push(TerritoryTheme {
                theme: representative.clone(),
                article_count: ids.len(),
                total_impressions,
                source_keywords: source_kws.clone(),
            });
        } else if ids.len() <= 1 && total_impressions >= OPEN_TERRITORY_IMPRESSION_THRESHOLD {
            open_territories.push(TerritoryTheme {
                theme: representative.clone(),
                article_count: ids.len(),
                total_impressions,
                source_keywords: source_kws.clone(),
            });
        }
    }

    // Sort by total impressions descending
    open_territories.sort_by(|a, b| {
        b.total_impressions
            .partial_cmp(&a.total_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    saturated_themes.sort_by(|a, b| {
        b.total_impressions
            .partial_cmp(&a.total_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Cap open territories
    let dropped = open_territories.len().saturating_sub(MAX_OPEN_TERRITORIES);
    if open_territories.len() > MAX_OPEN_TERRITORIES {
        open_territories.truncate(MAX_OPEN_TERRITORIES);
    }
    let _ = dropped; // unused for now

    TerritoryAnalysis {
        open_territories,
        saturated_themes,
        total_themes: merged_groups.len(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn canonical_keyword(kw: &str) -> String {
    let mut words: Vec<String> = kw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    words.sort_unstable();
    words.join(" ")
}

fn keyword_jaccard(a: &str, b: &str) -> f64 {
    let set_a: HashSet<String> = a
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    let set_b: HashSet<String> = b
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }
    let intersection: HashSet<&String> = set_a.intersection(&set_b).collect();
    let union_count = set_a.len() + set_b.len() - intersection.len();
    if union_count == 0 {
        return 0.0;
    }
    intersection.len() as f64 / union_count as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_territories_detects_saturated_and_open() {
        let articles = vec![
            ArticleSummary { id: 1, target_keyword: "saturated theme".to_string(), gsc_impressions: 1000.0 },
            ArticleSummary { id: 2, target_keyword: "saturated theme".to_string(), gsc_impressions: 1000.0 },
            ArticleSummary { id: 3, target_keyword: "saturated theme".to_string(), gsc_impressions: 1000.0 },
            ArticleSummary { id: 4, target_keyword: "saturated theme".to_string(), gsc_impressions: 1000.0 },
            ArticleSummary { id: 5, target_keyword: "saturated theme".to_string(), gsc_impressions: 1000.0 },
            ArticleSummary { id: 6, target_keyword: "saturated theme".to_string(), gsc_impressions: 1000.0 },
            ArticleSummary { id: 7, target_keyword: "open territory".to_string(), gsc_impressions: 5000.0 },
        ];

        let analysis = analyze_territories(&articles);
        assert_eq!(analysis.saturated_themes.len(), 1, "Should detect saturated theme");
        assert_eq!(analysis.saturated_themes[0].theme, "saturated theme");
        assert_eq!(analysis.open_territories.len(), 1, "Should detect open territory");
        assert_eq!(analysis.open_territories[0].theme, "open territory");
    }

    #[test]
    fn test_canonical_keyword_normalises() {
        assert_eq!(canonical_keyword("covered calls"), canonical_keyword("calls covered"));
        assert_eq!(canonical_keyword("Coffee-Maker"), "coffee maker");
    }

    #[test]
    fn test_keyword_jaccard_range() {
        assert_eq!(keyword_jaccard("a b c", "a b c"), 1.0);
        assert_eq!(keyword_jaccard("a b c", "x y z"), 0.0);
        let sim = keyword_jaccard("coffee maker", "best coffee maker");
        assert!(sim > 0.0 && sim < 1.0, "Partial overlap should give 0 < jaccard < 1");
    }
}
