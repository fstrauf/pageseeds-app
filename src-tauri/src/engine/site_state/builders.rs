//! Domain builders for Site State desk tools (issue #120).
//!
//! Single source of truth for `site_overview`, `articles`, and `article`.
//! CLI and investigate Rig tools call these functions — no business logic
//! in adapters.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{Duration, Utc};
use rusqlite::Connection;

use crate::content::ops::count_words;
use crate::content::redirects::load_redirect_source_slugs;
use crate::content::slug::normalize_url_slug;
use crate::db::{self, GscDailyWindowMetrics};
use crate::engine::exec::outcome_review::page_matches_slug;
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::article::Article;

use super::types::*;

// ── Public builders ──────────────────────────────────────────────────────────

/// Compact site-wide SEO desk: totals, top pages, movers, indexing sample.
pub fn build_site_overview(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    period_days: Option<i64>,
) -> Result<SiteOverview> {
    let period_days = period_days.unwrap_or(DEFAULT_PERIOD_DAYS).max(1);
    let generated_at = Utc::now().to_rfc3339();
    let articles = task_store::list_articles(conn, project_id)?;
    let redirected = load_redirect_source_slugs(project_path);
    let live: Vec<&Article> = articles
        .iter()
        .filter(|a| !is_redirected(&a.url_slug, &redirected))
        .collect();

    let page_index = build_page_index(conn, project_id)?;
    let (recent_start, recent_end) = recent_window(period_days);
    let (prev_start, prev_end) = previous_window(period_days);

    // Two bulk aggregations + pure Rust ranking (not O(articles) SQL).
    let recent_by_page =
        db::gsc_page_daily_window_metrics_bulk(conn, project_id, &recent_start, &recent_end)?;
    let prev_by_page =
        db::gsc_page_daily_window_metrics_bulk(conn, project_id, &prev_start, &prev_end)?;

    let mut total_impressions = 0.0_f64;
    let mut total_clicks = 0.0_f64;
    let mut top_candidates: Vec<TopPage> = Vec::new();
    let mut movers: Vec<TopMover> = Vec::new();
    let mut has_any_gsc = false;

    for article in &live {
        let slug = normalize_url_slug(&article.url_slug);
        let page = resolve_page_url(&page_index, &slug);
        let recent = page
            .as_ref()
            .and_then(|p| recent_by_page.get(p).copied());
        if let Some(m) = recent {
            has_any_gsc = true;
            total_impressions += m.impressions;
            total_clicks += m.clicks;
            let ctr = safe_ctr(m.clicks, m.impressions);
            top_candidates.push(TopPage {
                article_id: article.id,
                slug: slug.clone(),
                title: article.title.clone(),
                impressions: m.impressions,
                clicks: m.clicks,
                ctr,
                avg_position: m.position,
                target_keyword: article.target_keyword.clone(),
            });
        }

        if let Some(p) = page.as_ref() {
            let prev = prev_by_page.get(p).copied();
            if let (Some(r), Some(b)) = (recent, prev) {
                has_any_gsc = true;
                let clicks_delta = r.clicks - b.clicks;
                let impressions_delta = r.impressions - b.impressions;
                // Require some signal in either window so noise zeros stay out.
                if r.impressions + b.impressions > 0.0 {
                    movers.push(TopMover {
                        slug: slug.clone(),
                        clicks_delta,
                        impressions_delta,
                        direction: mover_direction(clicks_delta),
                    });
                }
            }
        }
    }

    top_candidates.sort_by(|a, b| {
        b.impressions
            .partial_cmp(&a.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_candidates.truncate(10);

    movers.sort_by(|a, b| {
        b.clicks_delta
            .abs()
            .partial_cmp(&a.clicks_delta.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    movers.truncate(10);

    let indexing = crate::gsc::db::list_by_project(conn, project_id).unwrap_or_default();
    let not_indexed_rows: Vec<_> = indexing
        .iter()
        .filter(|s| s.last_reason_code.as_deref() != Some("indexed_pass"))
        .collect();
    let not_indexed_count = not_indexed_rows.len();
    let not_indexed_sample: Vec<NotIndexedSample> = not_indexed_rows
        .into_iter()
        .take(10)
        .map(|s| NotIndexedSample {
            slug: crate::content::slug::extract_slug_from_url(&s.url),
            reason: s
                .last_reason_code
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        })
        .collect();

    let avg_ctr = safe_ctr(total_clicks, total_impressions);
    let hints = build_hints(has_any_gsc, &top_candidates);

    Ok(SiteOverview {
        project_id: project_id.to_string(),
        generated_at,
        freshness: Freshness {
            gsc_at: gsc_freshness_at(conn, project_id),
            evidence_index_at: None,
            evidence_coverage: 0.0,
        },
        totals: SiteTotals {
            articles_live: live.len(),
            articles_redirected: articles
                .iter()
                .filter(|a| is_redirected(&a.url_slug, &redirected))
                .count(),
            impressions: total_impressions,
            clicks: total_clicks,
            avg_ctr,
            not_indexed: not_indexed_count,
            // Delta vs #117: orphans left at 0 — full link scan is expensive for overview.
            orphans: 0,
            // Delta vs #117: validation_failures stubbed at 0 until audit wiring.
            validation_failures: 0,
        },
        top_pages: top_candidates,
        top_movers: movers,
        not_indexed_sample,
        hints,
    })
}

/// Article catalog with GSC rollup; redirected excluded by default.
///
/// List path is deliberately cheap: DB fields + bulk GSC join. File/MDX
/// enrichment is reserved for [`get_article_package`].
pub fn list_articles_catalog(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    filter: ArticlesFilter,
) -> Result<ArticlesCatalog> {
    let period_days = filter.period_days.unwrap_or(DEFAULT_PERIOD_DAYS).max(1);
    let generated_at = Utc::now().to_rfc3339();
    let articles = task_store::list_articles(conn, project_id)?;
    let redirected = load_redirect_source_slugs(project_path);
    let page_index = build_page_index(conn, project_id)?;
    let (start, end) = recent_window(period_days);
    let indexing_by_slug = indexing_status_map(conn, project_id);
    let metrics_by_page =
        db::gsc_page_daily_window_metrics_bulk(conn, project_id, &start, &end)?;

    let mut rows: Vec<ArticleCatalogRow> = Vec::new();
    for article in &articles {
        if !filter.include_redirected && is_redirected(&article.url_slug, &redirected) {
            continue;
        }
        if let Some(ref status) = filter.status {
            if !article.status.eq_ignore_ascii_case(status) {
                continue;
            }
        }

        let slug = normalize_url_slug(&article.url_slug);
        let page = resolve_page_url(&page_index, &slug);
        let metrics = page
            .as_ref()
            .and_then(|p| metrics_by_page.get(p).copied());
        let top_queries = load_top_queries(conn, project_id, article.id);
        let indexing_status = indexing_by_slug.get(&slug).cloned();

        let row = build_catalog_row(
            article,
            period_days,
            metrics.as_ref(),
            indexing_status,
            top_queries,
            None, // list path: DB fields only — no MDX re-parse
        );

        if row.gsc.impressions < filter.min_impressions {
            continue;
        }
        rows.push(row);
    }

    // Stable order: impressions desc, then slug.
    rows.sort_by(|a, b| {
        b.gsc
            .impressions
            .partial_cmp(&a.gsc.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.slug.cmp(&b.slug))
    });

    if let Some(limit) = filter.limit {
        rows.truncate(limit);
    }

    let count = rows.len();
    Ok(ArticlesCatalog {
        project_id: project_id.to_string(),
        generated_at,
        filter: ArticlesFilterEcho {
            status: filter.status,
            min_impressions: filter.min_impressions,
            include_redirected: filter.include_redirected,
        },
        count,
        articles: rows,
    })
}

/// Full package for one article: catalog + body/outline + queries + neighbors.
///
/// MDX source is read/parsed **once**; SERP enrichment and [`ArticleContent`]
/// are derived from that single materialization.
pub fn get_article_package(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    slug: &str,
    period_days: Option<i64>,
) -> Result<ArticlePackage> {
    let period_days = period_days.unwrap_or(DEFAULT_PERIOD_DAYS).max(1);
    let want = normalize_url_slug(slug);
    if want.is_empty() {
        return Err(Error::Validation("slug is required".into()));
    }

    let articles = task_store::list_articles(conn, project_id)?;
    let article = articles
        .iter()
        .find(|a| {
            let s = normalize_url_slug(&a.url_slug);
            s == want || a.url_slug == slug
        })
        .ok_or_else(|| Error::Other(format!("Article not found for slug '{slug}'")))?;

    let page_index = build_page_index(conn, project_id)?;
    let (start, end) = recent_window(period_days);
    let indexing_by_slug = indexing_status_map(conn, project_id);

    let normalized_slug = normalize_url_slug(&article.url_slug);
    let page = resolve_page_url(&page_index, &normalized_slug);
    let metrics = page.as_ref().and_then(|p| {
        db::gsc_page_daily_window_metrics(conn, project_id, p, &start, &end)
            .ok()
            .flatten()
    });
    let top_queries = load_top_queries(conn, project_id, article.id);
    let indexing_status = indexing_by_slug.get(&normalized_slug).cloned();

    // Single MDX materialization → SERP enrichment + content package.
    let materialized = materialize_article(project_path, article);
    let catalog = build_catalog_row(
        article,
        period_days,
        metrics.as_ref(),
        indexing_status,
        top_queries.clone(),
        Some(&materialized.enrichment),
    );
    let content = materialized.content;
    let query_cannibalization =
        build_query_cannibalization(conn, project_id, article.id, &articles, &top_queries)?;

    Ok(ArticlePackage {
        article_id: article.id,
        slug: catalog.slug.clone(),
        catalog,
        content,
        queries: top_queries,
        query_cannibalization,
        neighbors: vec![],
        validation: ValidationStub {
            ok: true,
            checks: vec![],
        },
    })
}

// ── Catalog row builder ──────────────────────────────────────────────────────

/// File-derived SERP/body fields (package path only).
struct FileEnrichment {
    serp_title: Option<String>,
    meta_description: Option<String>,
    h1: Option<String>,
    has_faq: bool,
    body_word_count: i64,
}

/// One MDX materialization shared by package catalog enrichment + content.
struct MaterializedArticle {
    enrichment: FileEnrichment,
    content: ArticleContent,
}

fn build_catalog_row(
    article: &Article,
    period_days: i64,
    metrics: Option<&GscDailyWindowMetrics>,
    indexing_status: Option<String>,
    top_queries: Vec<QueryMetric>,
    file_enrichment: Option<&FileEnrichment>,
) -> ArticleCatalogRow {
    let slug = normalize_url_slug(&article.url_slug);
    let (impressions, clicks, position) = match metrics {
        Some(m) => (m.impressions, m.clicks, m.position),
        None => (0.0, 0.0, 0.0),
    };
    let ctr = safe_ctr(clicks, impressions);

    let mut h1 = None;
    let mut meta_description = None;
    let mut has_faq = false;
    let mut word_count = article.word_count;
    let mut serp_title = article.title.clone();

    if let Some(enr) = file_enrichment {
        if let Some(ref t) = enr.serp_title {
            serp_title = t.clone();
        }
        meta_description = enr.meta_description.clone();
        h1 = enr.h1.clone();
        has_faq = enr.has_faq;
        if enr.body_word_count > 0 {
            word_count = enr.body_word_count;
        }
    }

    ArticleCatalogRow {
        article_id: article.id,
        slug: slug.clone(),
        url: format!("/blog/{slug}"),
        title: article.title.clone(),
        h1,
        target_keyword: article.target_keyword.clone(),
        intent_card: None,
        status: article.status.clone(),
        published_at: article.published_date.clone(),
        last_edited_at: article.last_edited_at.clone(),
        word_count,
        serp: SerpFields {
            title: serp_title.clone(),
            title_len: serp_title.chars().count(),
            meta_description: meta_description.clone(),
            meta_len: meta_description
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0),
            has_faq,
        },
        gsc: GscRollup {
            impressions,
            clicks,
            ctr,
            avg_position: position,
            period_days,
        },
        top_queries,
        // Delta vs #117: link counts left at zero — full scan is expensive per row.
        links: LinkCounts::default(),
        indexing_status,
        neighbors: vec![],
        evidence: EvidenceStub {
            content_hash: article.content_hash.clone(),
            indexed_at: None,
            embedding_model: None,
            has_embedding: false,
        },
    }
}

/// Read/parse MDX once; derive SERP enrichment + body/outline content.
fn materialize_article(project_path: &str, article: &Article) -> MaterializedArticle {
    let source = read_article_source(project_path, article).unwrap_or_default();
    let (frontmatter, body) = split_content_parts(&source);
    let outline = extract_outline(&body);
    let body_markdown = cap_body(&body);

    let serp_title = frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let meta_description = frontmatter
        .get("description")
        .or_else(|| frontmatter.get("metaDescription"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let h1 = extract_h1(&body);
    let has_faq = crate::engine::exec::audit_health::has_faq_schema(&source);
    let body_word_count = count_words(&body) as i64;

    MaterializedArticle {
        enrichment: FileEnrichment {
            serp_title,
            meta_description,
            h1,
            has_faq,
            body_word_count,
        },
        content: ArticleContent {
            file: article.file.clone(),
            frontmatter,
            body_markdown,
            outline,
        },
    }
}

fn load_top_queries(conn: &Connection, project_id: &str, article_id: i64) -> Vec<QueryMetric> {
    db::get_ctr_query_metrics(conn, project_id, article_id)
        .unwrap_or_default()
        .into_iter()
        .take(10)
        .map(|q| QueryMetric {
            query: q.query,
            impressions: q.impressions,
            clicks: q.clicks,
            avg_position: q.avg_position,
            ctr: q.ctr,
        })
        .collect()
}

fn split_content_parts(source: &str) -> (serde_json::Value, String) {
    if let Some((fm_raw, body)) = crate::content::frontmatter::split_mdx(source) {
        let fm_json = match crate::content::frontmatter::parse(fm_raw) {
            Ok(fm) => yaml_to_json(&fm.parsed),
            Err(_) => serde_json::json!({}),
        };
        (fm_json, body.to_string())
    } else {
        (serde_json::json!({}), source.to_string())
    }
}

fn yaml_to_json(v: &serde_yaml::Value) -> serde_json::Value {
    // Round-trip via serde_json string for nested YAML (lists/maps).
    match serde_json::to_value(v) {
        Ok(j) => j,
        Err(_) => serde_json::json!({}),
    }
}

fn cap_body(body: &str) -> String {
    if body.chars().count() <= BODY_SIZE_CAP {
        return body.to_string();
    }
    let truncated: String = body.chars().take(BODY_SIZE_CAP).collect();
    format!("{truncated}{BODY_TRUNCATION_NOTE}")
}

fn extract_outline(body: &str) -> Vec<OutlineHeading> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&level) && trimmed[level..].starts_with(' ') {
                Some(OutlineHeading {
                    level,
                    text: trimmed[level..].trim().to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

fn extract_h1(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let text = rest.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn build_query_cannibalization(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    articles: &[Article],
    queries: &[QueryMetric],
) -> Result<Vec<QueryCannibalization>> {
    if queries.is_empty() {
        return Ok(vec![]);
    }

    let slug_by_id: HashMap<i64, String> = articles
        .iter()
        .map(|a| (a.id, normalize_url_slug(&a.url_slug)))
        .collect();

    // Load all project query metrics once for best-effort cannibalization.
    let mut stmt = conn.prepare(
        "SELECT article_id, query, impressions, clicks
         FROM ctr_query_metrics
         WHERE project_id = ?1",
    )?;
    let all_rows: Vec<(i64, String, f64, f64)> = stmt
        .query_map(rusqlite::params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut out = Vec::new();
    for q in queries.iter().take(20) {
        let q_lower = q.query.to_lowercase();
        let mut others: Vec<CannibalSlugMetric> = all_rows
            .iter()
            .filter(|(aid, query, _, _)| {
                *aid != article_id && query.to_lowercase() == q_lower
            })
            .filter_map(|(aid, _, imps, clicks)| {
                let slug = slug_by_id.get(aid)?.clone();
                Some(CannibalSlugMetric {
                    slug,
                    impressions: *imps,
                    clicks: *clicks,
                })
            })
            .collect();
        if others.is_empty() {
            continue;
        }
        others.sort_by(|a, b| {
            b.impressions
                .partial_cmp(&a.impressions)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.push(QueryCannibalization {
            query: q.query.clone(),
            other_slugs: others,
        });
    }
    Ok(out)
}

// ── GSC / page helpers ───────────────────────────────────────────────────────

/// (normalized_slug, page_url) pairs for all gsc_page_daily pages.
fn build_page_index(conn: &Connection, project_id: &str) -> Result<Vec<(String, String)>> {
    let pages = db::list_gsc_page_daily_pages(conn, project_id)?;
    Ok(pages
        .into_iter()
        .map(|page| {
            let slug = crate::content::slug::extract_slug_from_url(&page);
            (slug, page)
        })
        .filter(|(slug, _)| !slug.is_empty())
        .collect())
}

fn resolve_page_url(page_index: &[(String, String)], slug: &str) -> Option<String> {
    page_index
        .iter()
        .find(|(s, page)| s == slug || page_matches_slug(page, slug))
        .map(|(_, page)| page.clone())
}

fn recent_window(period_days: i64) -> (String, String) {
    let end = Utc::now().date_naive() - Duration::days(1);
    let start = end - Duration::days(period_days - 1);
    (start.format("%Y-%m-%d").to_string(), end.format("%Y-%m-%d").to_string())
}

fn previous_window(period_days: i64) -> (String, String) {
    let recent_end = Utc::now().date_naive() - Duration::days(1);
    let recent_start = recent_end - Duration::days(period_days - 1);
    let prev_end = recent_start - Duration::days(1);
    let prev_start = prev_end - Duration::days(period_days - 1);
    (
        prev_start.format("%Y-%m-%d").to_string(),
        prev_end.format("%Y-%m-%d").to_string(),
    )
}

fn gsc_freshness_at(conn: &Connection, project_id: &str) -> Option<String> {
    let query_max = db::ctr_query_metrics_max_fetched_at(conn, project_id)
        .ok()
        .flatten();
    let page_max: Option<String> = conn
        .query_row(
            "SELECT MAX(fetched_at) FROM gsc_page_daily WHERE project_id = ?1",
            rusqlite::params![project_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    match (query_max, page_max) {
        (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn indexing_status_map(conn: &Connection, project_id: &str) -> HashMap<String, String> {
    let rows = crate::gsc::db::list_by_project(conn, project_id).unwrap_or_default();
    let mut map = HashMap::new();
    for row in rows {
        let slug = crate::content::slug::extract_slug_from_url(&row.url);
        if slug.is_empty() {
            continue;
        }
        let status = row
            .last_reason_code
            .clone()
            .or(row.last_verdict)
            .unwrap_or_else(|| "unknown".into());
        map.insert(slug, status);
    }
    map
}

fn is_redirected(url_slug: &str, redirected: &HashSet<String>) -> bool {
    redirected.contains(&normalize_url_slug(url_slug))
}

fn safe_ctr(clicks: f64, impressions: f64) -> f64 {
    if impressions > 0.0 {
        clicks / impressions
    } else {
        0.0
    }
}

fn mover_direction(clicks_delta: f64) -> String {
    if clicks_delta > 0.5 {
        "up".into()
    } else if clicks_delta < -0.5 {
        "down".into()
    } else {
        "flat".into()
    }
}

fn build_hints(has_any_gsc: bool, top_pages: &[TopPage]) -> Vec<String> {
    let mut hints = Vec::new();
    if !has_any_gsc {
        hints.push("GSC snapshots missing".into());
    }
    if top_pages
        .iter()
        .any(|p| p.impressions >= 1000.0 && p.ctr < 0.01)
    {
        hints.push("High-impression low-CTR pages present".into());
    }
    // Always until #119
    hints.push("Evidence index not available".into());
    hints
}

fn read_article_source(project_path: &str, article: &Article) -> Option<String> {
    let repo = Path::new(project_path);
    crate::engine::exec::utils::read_source_file(repo, &article.file).or_else(|| {
        crate::engine::exec::audit_health::resolve_content_file(repo, &article.file)
            .and_then(|p| std::fs::read_to_string(p).ok())
    })
}
