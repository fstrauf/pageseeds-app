/// Deterministic territory analysis for keyword research.
///
/// Groups articles by target_keyword, uses semantic Jaccard grouping to collapse
/// variations, and identifies:
///   - Open territories: low coverage (≤1 article) + high impressions (≥5k)
///   - Mid-coverage themes: 2–5 articles (expansion candidates for shortlist)
///   - Saturated themes: high coverage (>5 articles) competing for same theme
///
/// Impressions come from the desk SoT daily tape (`gsc_page_daily`, 28-day window
/// ending yesterday), with fallback to `article_metadata` namespace `gsc` when the
/// tape has no row for an article.
///
/// Results are synced to the `research_shortlist` SQLite table for consumption
/// by the keyword research pipeline.
use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::content::slug::normalize_url_slug;
use crate::db::research_shortlist::{upsert_entry, ResearchShortlistEntry};
use crate::db::{
    build_page_index, recent_window, rollup_impressions_for_slug,
};
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

const OPEN_TERRITORY_IMPRESSION_THRESHOLD: f64 = 5000.0;
const SATURATION_THRESHOLD: usize = 5;
const MAX_OPEN_TERRITORIES: usize = 10;
const MAX_MID_COVERAGE_THEMES: usize = 10;
/// Desk recent window: 28 days ending yesterday (matches site_state / `db::recent_window`).
const GSC_WINDOW_DAYS: i64 = 28;

/// A lightweight article record for territory analysis.
#[derive(Debug, Clone)]
struct ArticleSummary {
    id: i64,
    target_keyword: String,
    url_slug: String,
    gsc_impressions: f64,
}

/// GSC load provenance for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GscSource {
    GscPageDaily,
    ArticleMetadataFallback,
    Mixed,
    None,
}

impl GscSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::GscPageDaily => "gsc_page_daily",
            Self::ArticleMetadataFallback => "article_metadata_fallback",
            Self::Mixed => "mixed",
            Self::None => "none",
        }
    }

    fn from_flags(used_tape: bool, used_fallback: bool) -> Self {
        match (used_tape, used_fallback) {
            (true, true) => Self::Mixed,
            (true, false) => Self::GscPageDaily,
            (false, true) => Self::ArticleMetadataFallback,
            (false, false) => Self::None,
        }
    }
}

struct LoadResult {
    articles: Vec<ArticleSummary>,
    gsc_source: GscSource,
}

/// Diagnostics from a territory analysis run (shared by task step + research-context ensure).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TerritoryRunDiagnostics {
    pub open_territories: Vec<TerritoryTheme>,
    pub mid_coverage_themes: Vec<TerritoryTheme>,
    pub saturated_themes: Vec<TerritoryTheme>,
    pub total_themes: usize,
    pub synced_to_shortlist: usize,
    pub gsc_source: String,
    pub buckets: serde_json::Value,
    pub skip_reasons: Vec<String>,
    pub message: String,
}

impl TerritoryRunDiagnostics {
    /// JSON shape historically stored as the territory step artifact/output.
    pub fn to_output_json(&self) -> serde_json::Value {
        serde_json::json!({
            "open_territories": self.open_territories,
            "mid_coverage_themes": self.mid_coverage_themes,
            "saturated_themes": self.saturated_themes,
            "total_themes": self.total_themes,
            "synced_to_shortlist": self.synced_to_shortlist,
            "gsc_source": self.gsc_source,
            "buckets": self.buckets,
            "skip_reasons": self.skip_reasons,
        })
    }
}

/// Core territory analysis: load articles, classify themes, upsert shortlist.
///
/// Callable from the workflow step and from `ensure_research_shortlist_fresh`
/// (CLI `research-context`) with an already-open connection — no nested DB open.
pub fn run_territory_analysis(
    conn: &Connection,
    project_id: &str,
) -> crate::error::Result<TerritoryRunDiagnostics> {
    // 1. Load articles + GSC (daily tape preferred, metadata fallback)
    let load = load_articles_with_gsc(conn, project_id)?;

    if load.articles.is_empty() {
        return Ok(TerritoryRunDiagnostics {
            open_territories: vec![],
            mid_coverage_themes: vec![],
            saturated_themes: vec![],
            total_themes: 0,
            synced_to_shortlist: 0,
            gsc_source: load.gsc_source.as_str().to_string(),
            buckets: serde_json::json!({
                "open": 0,
                "mid_coverage": 0,
                "saturated": 0,
                "thin_below_impression_threshold": 0,
                "articles_without_target_keyword": 0,
                "articles_with_zero_gsc": 0,
            }),
            skip_reasons: vec!["No articles found for territory analysis".to_string()],
            message: "No articles found for territory analysis".to_string(),
        });
    }

    // 2. Run analysis
    let analysis = analyze_territories(&load.articles);

    // 3. Sync open / mid-coverage / saturated to shortlist, annotated with the
    // project.md strategy cluster the theme maps to (issue #255). Best-effort:
    // missing/empty strategy leaves the annotation NULL, never fails analysis.
    let strategy = crate::strategy::load_for_project(conn, project_id);

    let open_territories = analysis.open_territories.clone();
    let mid_coverage_themes = analysis.mid_coverage_themes.clone();
    let saturated_themes = analysis.saturated_themes.clone();

    let mut synced = 0usize;
    for territory in &open_territories {
        if sync_theme_to_shortlist(conn, project_id, territory, "high", "pending", &strategy) {
            synced += 1;
        }
    }
    for theme in &mid_coverage_themes {
        if sync_theme_to_shortlist(conn, project_id, theme, "medium", "pending", &strategy) {
            synced += 1;
        }
    }
    for theme in &saturated_themes {
        // Saturated themes get a special status so keyword research can deprioritize them
        if sync_theme_to_shortlist(conn, project_id, theme, "medium", "saturated", &strategy) {
            synced += 1;
        }
    }

    // Inject Primary/ACTIVE strategy seeds as pending fuel (issue #274).
    // Territory GSC rows stay; this adds product-gap terms with 0 impressions.
    if let Err(e) =
        crate::engine::research_shortlist_refresh::inject_strategy_shortlist_seeds(conn, project_id)
    {
        log::warn!(
            "[territory_analysis] strategy shortlist inject failed (non-fatal): {}",
            e
        );
    }

    // Inject uncovered GSC query demand as pending fuel (issue #304).
    if let Err(e) =
        crate::engine::research_shortlist_refresh::inject_gsc_uncovered_seeds(conn, project_id)
    {
        log::warn!(
            "[territory_analysis] gsc_uncovered shortlist inject failed (non-fatal): {}",
            e
        );
    }

    // 4. Prune old covered entries
    let _ = crate::db::research_shortlist::prune_covered(conn, project_id, 30);

    // 5. Diagnostics
    let articles_without_target_keyword = load
        .articles
        .iter()
        .filter(|a| a.target_keyword.trim().is_empty())
        .count();
    let articles_with_zero_gsc = load
        .articles
        .iter()
        .filter(|a| a.gsc_impressions <= 0.0)
        .count();

    let buckets = serde_json::json!({
        "open": open_territories.len(),
        "mid_coverage": mid_coverage_themes.len(),
        "saturated": saturated_themes.len(),
        "thin_below_impression_threshold": analysis.thin_below_impression_threshold,
        "articles_without_target_keyword": articles_without_target_keyword,
        "articles_with_zero_gsc": articles_with_zero_gsc,
    });

    let skip_reasons = if synced == 0 {
        build_skip_reasons(
            &load,
            &analysis,
            articles_without_target_keyword,
            articles_with_zero_gsc,
        )
    } else {
        Vec::new()
    };

    let gsc_source = load.gsc_source.as_str().to_string();
    let message = format!(
        "Territory analysis: {} open, {} mid-coverage, {} saturated, {} synced to shortlist (gsc={})",
        open_territories.len(),
        mid_coverage_themes.len(),
        saturated_themes.len(),
        synced,
        gsc_source
    );

    Ok(TerritoryRunDiagnostics {
        open_territories,
        mid_coverage_themes,
        saturated_themes,
        total_themes: analysis.total_themes,
        synced_to_shortlist: synced,
        gsc_source,
        buckets,
        skip_reasons,
        message,
    })
}

/// Run territory analysis and sync results to the research_shortlist table.
pub(crate) fn exec_research_territory_analysis(task: &Task, _project_path: &str) -> StepResult {
    let db_path = crate::db::default_db_path();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to open DB: {}", e));
        }
    };

    match run_territory_analysis(&conn, &task.project_id) {
        Ok(diag) => StepResult {
            success: true,
            message: diag.message.clone(),
            output: Some(
                serde_json::to_string_pretty(&diag.to_output_json()).unwrap_or_default(),
            ),
            artifact_key: None,
        },
        Err(e) => StepResult::fail(format!("Failed to run territory analysis: {}", e)),
    }
}

fn build_skip_reasons(
    load: &LoadResult,
    analysis: &TerritoryAnalysis,
    articles_without_kw: usize,
    articles_with_zero_gsc: usize,
) -> Vec<String> {
    let mut reasons = Vec::new();

    if load.gsc_source == GscSource::None {
        reasons.push(
            "No GSC data from gsc_page_daily or article_metadata; all article impressions are 0"
                .to_string(),
        );
    }

    if analysis.total_themes == 0 {
        reasons.push(
            "No themes to classify: no articles have a non-empty target_keyword".to_string(),
        );
    } else {
        if analysis.thin_below_impression_threshold > 0
            && analysis.open_territories.is_empty()
            && analysis.mid_coverage_themes.is_empty()
            && analysis.saturated_themes.is_empty()
        {
            reasons.push(format!(
                "{} theme(s) are thin (≤1 article) with impressions below the open threshold ({})",
                analysis.thin_below_impression_threshold, OPEN_TERRITORY_IMPRESSION_THRESHOLD as i64
            ));
        }
        if analysis.open_territories.is_empty()
            && analysis.mid_coverage_themes.is_empty()
            && analysis.saturated_themes.is_empty()
            && analysis.thin_below_impression_threshold == 0
        {
            reasons.push(
                "No open, mid-coverage, or saturated themes matched classification rules"
                    .to_string(),
            );
        }
    }

    if articles_without_kw > 0 {
        reasons.push(format!(
            "{} article(s) lack a target_keyword and were excluded from theme grouping",
            articles_without_kw
        ));
    }
    if articles_with_zero_gsc > 0 && load.gsc_source != GscSource::None {
        reasons.push(format!(
            "{} article(s) have zero GSC impressions in the analysis window",
            articles_with_zero_gsc
        ));
    }

    if reasons.is_empty() {
        reasons.push(
            "synced_to_shortlist is 0 (no open/mid/saturated themes to upsert)".to_string(),
        );
    }

    reasons
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Upsert one territory theme into the research shortlist.
/// Returns true when the row was written successfully.
///
/// Hard-blocked themes (`do_not_expand` / LEGACY via
/// [`crate::strategy::strategy_blocks_expansion`]) are **not** written as
/// expandable `pending` research fuel (issue #258). Saturated inventory rows
/// are still annotated for package visibility.
fn sync_theme_to_shortlist(
    conn: &Connection,
    project_id: &str,
    theme: &TerritoryTheme,
    priority: &str,
    status: &str,
    strategy: &crate::strategy::ProjectStrategy,
) -> bool {
    // Produce-side gate: never leave hard-blocked themes as pending fuel.
    // Consume-side uses the same helper so residual rows stay out of seeds.
    if status == "pending"
        && crate::strategy::strategy_blocks_expansion(&theme.theme, strategy)
    {
        log::info!(
            "[territory_analysis] shortlist_strategy_skipped theme='{}' (do_not_expand/LEGACY; not pending fuel)",
            theme.theme
        );
        return false;
    }

    let mut entry = ResearchShortlistEntry::new(
        project_id,
        &theme.theme,
        theme.source_keywords.clone(),
        "territory_analysis",
        priority,
        Some(theme.article_count as i64),
        Some(theme.total_impressions),
    );
    entry.status = status.to_string();
    if let Some((cluster, cluster_status)) = crate::strategy::match_cluster(strategy, &theme.theme) {
        entry.strategy_cluster = Some(cluster.to_string());
        entry.strategy_status = Some(cluster_status.as_str().to_string());
    }
    match upsert_entry(conn, &entry) {
        Ok(_) => true,
        Err(e) => {
            log::warn!(
                "[territory_analysis] Failed to upsert shortlist entry for '{}' (status={}): {}",
                theme.theme,
                status,
                e
            );
            false
        }
    }
}

fn load_articles_with_gsc(
    conn: &Connection,
    project_id: &str,
) -> crate::error::Result<LoadResult> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)?;

    // Desk SoT: 28-day daily tape rollup by page, joined via shared slug helpers.
    // DB failures propagate — only empty data yields gsc_source=none.
    let page_index = build_page_index(conn, project_id)?;
    let (start, end) = recent_window(GSC_WINDOW_DAYS);
    let metrics_by_page =
        crate::db::gsc_page_daily_window_metrics_bulk(conn, project_id, &start, &end)?;

    // Fallback: article_metadata namespace gsc (legacy / when tape has no row).
    let metadata = crate::db::list_project_metadata(conn, project_id)?;
    let mut gsc_meta_by_article: HashMap<i64, serde_json::Value> = HashMap::new();
    for (article_id, namespace, payload) in metadata {
        if namespace == "gsc" {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload) {
                gsc_meta_by_article.insert(article_id, json);
            }
        }
    }

    let mut used_tape = false;
    let mut used_fallback = false;
    let mut summaries = Vec::with_capacity(articles.len());

    for article in articles {
        let slug = normalize_url_slug(&article.url_slug);
        let (impressions, from_tape) =
            if let Some(imp) = rollup_impressions_for_slug(&page_index, &metrics_by_page, &slug) {
                (imp, true)
            } else if let Some(gsc) = gsc_meta_by_article.get(&article.id) {
                let imp = gsc["impressions"].as_f64().unwrap_or(0.0);
                (imp, false)
            } else {
                (0.0, false)
            };

        if from_tape {
            used_tape = true;
        } else if impressions > 0.0 || gsc_meta_by_article.contains_key(&article.id) {
            // Count as fallback only when we actually consulted metadata for this article.
            if gsc_meta_by_article.contains_key(&article.id) {
                used_fallback = true;
            }
        }

        let kw = article.target_keyword.unwrap_or_default();
        summaries.push(ArticleSummary {
            id: article.id,
            target_keyword: kw,
            url_slug: slug,
            gsc_impressions: impressions,
        });
    }

    Ok(LoadResult {
        articles: summaries,
        gsc_source: GscSource::from_flags(used_tape, used_fallback),
    })
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
    pub mid_coverage_themes: Vec<TerritoryTheme>,
    pub saturated_themes: Vec<TerritoryTheme>,
    pub total_themes: usize,
    /// Themes with ≤1 article and impressions below the open threshold (not shortlisted).
    #[serde(skip)]
    pub thin_below_impression_threshold: usize,
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
    let mut mid_coverage_themes: Vec<TerritoryTheme> = Vec::new();
    let mut saturated_themes: Vec<TerritoryTheme> = Vec::new();
    let mut thin_below_impression_threshold = 0usize;

    for (representative, (ids, source_kws)) in &merged_groups {
        let total_impressions: f64 = ids
            .iter()
            .filter_map(|&id| articles.iter().find(|a| a.id == id))
            .map(|a| a.gsc_impressions)
            .sum();

        let theme = TerritoryTheme {
            theme: representative.clone(),
            article_count: ids.len(),
            total_impressions,
            source_keywords: source_kws.clone(),
        };

        if ids.len() > SATURATION_THRESHOLD {
            saturated_themes.push(theme);
        } else if ids.len() >= 2 && ids.len() <= SATURATION_THRESHOLD {
            // Mid-coverage: 2..=5 articles (expansion shortlist candidates)
            mid_coverage_themes.push(theme);
        } else if ids.len() <= 1 && total_impressions >= OPEN_TERRITORY_IMPRESSION_THRESHOLD {
            open_territories.push(theme);
        } else if ids.len() <= 1 {
            // Thin + low impressions — do not shortlist
            thin_below_impression_threshold += 1;
        }
    }

    // Sort by total impressions descending
    let sort_by_impressions = |a: &TerritoryTheme, b: &TerritoryTheme| {
        b.total_impressions
            .partial_cmp(&a.total_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    open_territories.sort_by(sort_by_impressions);
    mid_coverage_themes.sort_by(sort_by_impressions);
    saturated_themes.sort_by(sort_by_impressions);

    // Cap open and mid-coverage shortlist candidates
    if open_territories.len() > MAX_OPEN_TERRITORIES {
        open_territories.truncate(MAX_OPEN_TERRITORIES);
    }
    if mid_coverage_themes.len() > MAX_MID_COVERAGE_THEMES {
        mid_coverage_themes.truncate(MAX_MID_COVERAGE_THEMES);
    }

    TerritoryAnalysis {
        open_territories,
        mid_coverage_themes,
        saturated_themes,
        total_themes: merged_groups.len(),
        thin_below_impression_threshold,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn canonical_keyword(kw: &str) -> String {
    // Normalize: lowercase, strip non-alphanumeric, collapse to single spaces.
    // Preserve original word order so the theme remains readable.
    // Jaccard similarity (used for merging) already handles word-reorder
    // duplicates — sorting here only destroys readability.
    kw.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
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

    fn art(id: i64, kw: &str, impressions: f64) -> ArticleSummary {
        ArticleSummary {
            id,
            target_keyword: kw.to_string(),
            url_slug: format!("slug-{}", id),
            gsc_impressions: impressions,
        }
    }

    #[test]
    fn test_analyze_territories_detects_saturated_and_open() {
        let articles = vec![
            art(1, "saturated theme", 1000.0),
            art(2, "saturated theme", 1000.0),
            art(3, "saturated theme", 1000.0),
            art(4, "saturated theme", 1000.0),
            art(5, "saturated theme", 1000.0),
            art(6, "saturated theme", 1000.0),
            art(7, "open territory", 5000.0),
        ];

        let analysis = analyze_territories(&articles);
        assert_eq!(analysis.saturated_themes.len(), 1, "Should detect saturated theme");
        assert_eq!(analysis.saturated_themes[0].theme, "saturated theme");
        assert_eq!(analysis.open_territories.len(), 1, "Should detect open territory");
        assert_eq!(analysis.open_territories[0].theme, "open territory");
        assert!(
            analysis.mid_coverage_themes.is_empty(),
            "Six-article theme is saturated, not mid"
        );
    }

    #[test]
    fn test_mid_coverage_two_to_five_articles() {
        // 3 articles → mid, not open/saturated
        let articles = vec![
            art(1, "mid theme", 800.0),
            art(2, "mid theme", 700.0),
            art(3, "mid theme", 600.0),
        ];
        let analysis = analyze_territories(&articles);
        assert_eq!(analysis.mid_coverage_themes.len(), 1);
        assert_eq!(analysis.mid_coverage_themes[0].theme, "mid theme");
        assert_eq!(analysis.mid_coverage_themes[0].article_count, 3);
        assert!(analysis.open_territories.is_empty());
        assert!(analysis.saturated_themes.is_empty());
        assert_eq!(analysis.thin_below_impression_threshold, 0);

        // Boundaries: 2 and 5 articles are mid
        let two = vec![art(1, "pair", 100.0), art(2, "pair", 100.0)];
        let two_a = analyze_territories(&two);
        assert_eq!(two_a.mid_coverage_themes.len(), 1);
        assert_eq!(two_a.mid_coverage_themes[0].article_count, 2);

        let five: Vec<_> = (1..=5).map(|i| art(i, "quintet", 50.0)).collect();
        let five_a = analyze_territories(&five);
        assert_eq!(five_a.mid_coverage_themes.len(), 1);
        assert_eq!(five_a.mid_coverage_themes[0].article_count, 5);
        assert!(five_a.saturated_themes.is_empty());
    }

    #[test]
    fn test_bands_six_open_and_three() {
        // Six → saturated; one with 5k+ → open; three → mid
        let mut articles: Vec<_> = (1..=6).map(|i| art(i, "saturated kw", 200.0)).collect();
        articles.push(art(10, "open kw", 5500.0));
        articles.push(art(11, "mid kw", 300.0));
        articles.push(art(12, "mid kw", 400.0));
        articles.push(art(13, "mid kw", 500.0));

        let analysis = analyze_territories(&articles);
        assert_eq!(analysis.saturated_themes.len(), 1);
        assert_eq!(analysis.saturated_themes[0].article_count, 6);
        assert_eq!(analysis.open_territories.len(), 1);
        assert_eq!(analysis.open_territories[0].theme, "open kw");
        assert_eq!(analysis.mid_coverage_themes.len(), 1);
        assert_eq!(analysis.mid_coverage_themes[0].theme, "mid kw");
        assert_eq!(analysis.mid_coverage_themes[0].article_count, 3);
        assert_eq!(analysis.total_themes, 3);
    }

    #[test]
    fn test_thin_low_impressions_not_shortlisted() {
        // Single article under 5k impressions → thin, not open/mid/saturated
        let articles = vec![
            art(1, "thin theme", 4999.0),
            art(2, "zero theme", 0.0),
            art(3, "", 10000.0), // empty keyword excluded from themes
        ];
        let analysis = analyze_territories(&articles);
        assert!(analysis.open_territories.is_empty());
        assert!(analysis.mid_coverage_themes.is_empty());
        assert!(analysis.saturated_themes.is_empty());
        assert_eq!(analysis.thin_below_impression_threshold, 2);
        assert_eq!(analysis.total_themes, 2);

        // Exactly at threshold still opens
        let at_bar = vec![art(1, "border open", 5000.0)];
        let open = analyze_territories(&at_bar);
        assert_eq!(open.open_territories.len(), 1);
        assert_eq!(open.thin_below_impression_threshold, 0);
    }

    #[test]
    fn test_mid_coverage_capped_by_impressions() {
        let mut articles = Vec::new();
        for t in 0..12 {
            let kw = format!("mid theme {}", t);
            // Higher t → higher impressions so sort order is deterministic
            let imp = (t as f64 + 1.0) * 100.0;
            articles.push(art(t * 2, &kw, imp));
            articles.push(art(t * 2 + 1, &kw, imp));
        }
        let analysis = analyze_territories(&articles);
        assert_eq!(
            analysis.mid_coverage_themes.len(),
            MAX_MID_COVERAGE_THEMES,
            "mid-coverage should be capped"
        );
        // Top by impressions: themes 11 down to 2
        assert_eq!(analysis.mid_coverage_themes[0].theme, "mid theme 11");
        assert!(analysis.mid_coverage_themes[0].total_impressions
            >= analysis.mid_coverage_themes.last().unwrap().total_impressions);
    }

    #[test]
    fn test_gsc_source_from_flags() {
        assert_eq!(
            GscSource::from_flags(true, false).as_str(),
            "gsc_page_daily"
        );
        assert_eq!(
            GscSource::from_flags(false, true).as_str(),
            "article_metadata_fallback"
        );
        assert_eq!(GscSource::from_flags(true, true).as_str(), "mixed");
        assert_eq!(GscSource::from_flags(false, false).as_str(), "none");
    }

    #[test]
    fn test_canonical_keyword_normalises() {
        // Normalization preserves word order (readability) but lowercases and strips punctuation.
        assert_eq!(canonical_keyword("covered calls"), "covered calls");
        assert_eq!(canonical_keyword("Coffee-Maker"), "coffee maker");
        // Word-reorder duplicates are still merged by keyword_jaccard (similarity = 1.0)
        assert_eq!(keyword_jaccard("covered calls", "calls covered"), 1.0);
    }

    #[test]
    fn test_keyword_jaccard_range() {
        assert_eq!(keyword_jaccard("a b c", "a b c"), 1.0);
        assert_eq!(keyword_jaccard("a b c", "x y z"), 0.0);
        let sim = keyword_jaccard("coffee maker", "best coffee maker");
        assert!(sim > 0.0 && sim < 1.0, "Partial overlap should give 0 < jaccard < 1");
    }

    #[test]
    fn test_skip_reasons_when_all_thin() {
        let articles = vec![art(1, "thin", 100.0)];
        let analysis = analyze_territories(&articles);
        let load = LoadResult {
            articles: articles.clone(),
            gsc_source: GscSource::GscPageDaily,
        };
        let reasons = build_skip_reasons(&load, &analysis, 0, 0);
        assert!(
            reasons.iter().any(|r| r.contains("thin") || r.contains("impressions below")),
            "expected thin/skip reason, got {:?}",
            reasons
        );
    }

    // ── Load-path fixture (gsc_page_daily SoT, empty article_metadata) ──────

    fn load_fixture_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', '/tmp/territory-test', 1, 'workspace')",
            rusqlite::params![id],
        )
        .unwrap();
    }

    fn insert_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        slug: &str,
        keyword: &str,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', ?6, '[]', 0, 100, 0, 'hash')",
            rusqlite::params![
                id,
                project_id,
                format!("Title {id}"),
                slug,
                format!("content/{slug}.mdx"),
                keyword
            ],
        )
        .unwrap();
    }

    fn daily_row(
        page: &str,
        date: &str,
        impressions: f64,
    ) -> crate::models::gsc::PageDailyMetrics {
        crate::models::gsc::PageDailyMetrics {
            page: page.to_string(),
            date: date.to_string(),
            clicks: 1.0,
            impressions,
            ctr: if impressions > 0.0 {
                1.0 / impressions
            } else {
                0.0
            },
            position: 8.0,
        }
    }

    #[test]
    fn load_articles_with_gsc_uses_daily_tape_when_metadata_empty() {
        use chrono::{Duration, Utc};

        let conn = load_fixture_db();
        insert_project(&conn, "proj1");
        // Open: 1 article, high impressions via tape only.
        insert_article(&conn, "proj1", 1, "open-post", "open territory kw");
        // Mid: 3 articles, modest impressions.
        insert_article(&conn, "proj1", 2, "mid-a", "mid theme kw");
        insert_article(&conn, "proj1", 3, "mid-b", "mid theme kw");
        insert_article(&conn, "proj1", 4, "mid-c", "mid theme kw");

        let end = Utc::now().date_naive() - Duration::days(1);
        let d1 = (end - Duration::days(3)).format("%Y-%m-%d").to_string();
        let d2 = end.format("%Y-%m-%d").to_string();
        let rows = vec![
            // Two URL variants for open-post should sum.
            daily_row("https://example.com/blog/open-post", &d1, 3000.0),
            daily_row("https://example.com/blog/open-post/", &d2, 2500.0),
            daily_row("https://example.com/blog/mid-a", &d1, 100.0),
            daily_row("https://example.com/blog/mid-b", &d1, 150.0),
            daily_row("https://example.com/blog/mid-c", &d2, 200.0),
        ];
        crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

        // No article_metadata gsc rows — tape must be the sole source.
        let load = load_articles_with_gsc(&conn, "proj1").unwrap();
        assert_eq!(load.gsc_source, GscSource::GscPageDaily);
        assert_eq!(load.articles.len(), 4);

        let open = load
            .articles
            .iter()
            .find(|a| a.url_slug == "open-post")
            .expect("open-post present");
        assert!(
            open.gsc_impressions > 0.0,
            "tape join must yield non-zero impressions, got {}",
            open.gsc_impressions
        );
        assert_eq!(
            open.gsc_impressions, 5500.0,
            "URL variants must sum (3000+2500)"
        );

        let analysis = analyze_territories(&load.articles);
        assert_eq!(
            analysis.open_territories.len(),
            1,
            "1 article + ≥5k impressions → open; got {:?}",
            analysis.open_territories
        );
        assert_eq!(analysis.open_territories[0].theme, "open territory kw");
        assert_eq!(
            analysis.mid_coverage_themes.len(),
            1,
            "3 articles → mid; got {:?}",
            analysis.mid_coverage_themes
        );
        assert_eq!(analysis.mid_coverage_themes[0].theme, "mid theme kw");
        assert!(analysis.saturated_themes.is_empty());
    }

    // ── Strategy cluster annotation (issue #255) ────────────────────────────

    fn insert_project_with_path(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', ?2, 1, 'workspace')",
            rusqlite::params![id, path],
        )
        .unwrap();
    }

    fn strategy_project_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-territory-strategy-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        std::fs::write(
            automation.join("project.md"),
            r#"# Test Project

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo

### Cluster 2: Old Services (LEGACY)
- web design packages
"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn run_territory_analysis_annotates_shortlist_with_strategy_cluster() {
        let conn = load_fixture_db();
        let project_dir = strategy_project_dir();
        insert_project_with_path(&conn, "proj1", &project_dir.to_string_lossy());

        // Mid-coverage themes (2 articles each): one ACTIVE, one LEGACY, one unmatched.
        insert_article(&conn, "proj1", 1, "tech-seo-a", "technical seo");
        insert_article(&conn, "proj1", 2, "tech-seo-b", "technical seo");
        insert_article(&conn, "proj1", 3, "webdesign-a", "web design packages");
        insert_article(&conn, "proj1", 4, "webdesign-b", "web design packages");
        insert_article(&conn, "proj1", 5, "random-a", "random unmatched topic");
        insert_article(&conn, "proj1", 6, "random-b", "random unmatched topic");

        let diag = run_territory_analysis(&conn, "proj1").unwrap();
        // LEGACY mid-coverage is skipped as pending research fuel (issue #258).
        assert_eq!(diag.synced_to_shortlist, 2);

        let entries = crate::db::research_shortlist::list_entries(&conn, "proj1", None).unwrap();
        let by_theme = |t: &str| entries.iter().find(|e| e.theme == t);

        let active = by_theme("technical seo").expect("ACTIVE theme must sync");
        assert_eq!(active.strategy_cluster.as_deref(), Some("SEO Fundamentals"));
        assert_eq!(active.strategy_status.as_deref(), Some("active"));
        assert_eq!(active.status, "pending");

        // Hard-blocked LEGACY must not become pending research fuel.
        assert!(
            by_theme("web design packages").is_none(),
            "LEGACY theme must not sync as pending shortlist fuel"
        );

        let unmatched = by_theme("random unmatched topic").expect("unmatched still syncs");
        assert_eq!(unmatched.strategy_cluster, None);
        assert_eq!(unmatched.strategy_status, None);

        let _ = std::fs::remove_dir_all(&project_dir);
    }

    #[test]
    fn run_territory_analysis_skips_do_not_expand_pending_fuel() {
        let conn = load_fixture_db();
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-territory-dne-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        std::fs::write(
            automation.join("project.md"),
            r#"# Test Project

## Search Keywords

### Primary Keywords
- technical seo

### Legacy Service Keywords (do not expand)
- custom web design

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo
"#,
        )
        .unwrap();
        insert_project_with_path(&conn, "proj1", &dir.to_string_lossy());

        insert_article(&conn, "proj1", 1, "tech-a", "technical seo");
        insert_article(&conn, "proj1", 2, "tech-b", "technical seo");
        insert_article(&conn, "proj1", 3, "cwd-a", "custom web design");
        insert_article(&conn, "proj1", 4, "cwd-b", "custom web design");

        let diag = run_territory_analysis(&conn, "proj1").unwrap();
        assert_eq!(diag.synced_to_shortlist, 1);

        let entries = crate::db::research_shortlist::list_entries(&conn, "proj1", None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].theme, "technical seo");
        assert_eq!(entries[0].status, "pending");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_territory_analysis_without_strategy_leaves_annotation_null() {
        let conn = load_fixture_db();
        // '/tmp/territory-test' has no project.md → empty strategy → NULLs.
        insert_project(&conn, "proj1");
        insert_article(&conn, "proj1", 1, "tech-seo-a", "technical seo");
        insert_article(&conn, "proj1", 2, "tech-seo-b", "technical seo");

        let diag = run_territory_analysis(&conn, "proj1").unwrap();
        assert_eq!(diag.synced_to_shortlist, 1);

        let entries = crate::db::research_shortlist::list_entries(&conn, "proj1", None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].strategy_cluster, None);
        assert_eq!(entries[0].strategy_status, None);
    }
}
