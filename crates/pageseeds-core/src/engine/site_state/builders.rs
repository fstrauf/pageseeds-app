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
use crate::content::redirects::{load_redirect_map, load_redirect_source_slugs};
use crate::content::slug::{extract_slug_from_url, normalize_url_slug, resolve_slug};
use crate::db::{
    self, build_page_index, pages_for_slug, previous_window, recent_window, rollup_for_slug,
    GscDailyWindowMetrics,
};
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
    let redirect_map = load_redirect_map(project_path);
    let redirected: HashSet<String> = redirect_map.keys().cloned().collect();
    let live: Vec<&Article> = articles
        .iter()
        .filter(|a| !is_redirected(&a.url_slug, &redirected))
        .collect();
    let live_catalog_slugs: HashSet<String> = live
        .iter()
        .map(|a| normalize_url_slug(&a.url_slug))
        .filter(|s| !s.is_empty())
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
    let mut multi_url_slug_count = 0_usize;
    // Collect zero-impression / striking candidates during the same live pass (#204).
    let mut zero_impression_candidates: Vec<ZeroImpressionSample> = Vec::new();
    let mut striking_candidates: Vec<StrikingDistanceSample> = Vec::new();

    for article in &live {
        let slug = normalize_url_slug(&article.url_slug);
        let pages = pages_for_slug(&page_index, &slug);
        if pages.len() > 1 {
            multi_url_slug_count += 1;
        }
        let (recent, _) = rollup_for_slug(&page_index, &recent_by_page, &slug);
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

            // Striking distance: only articles with recent metrics in band (#204).
            if m.impressions >= STRIKING_MIN_IMPRESSIONS
                && m.position >= STRIKING_POS_MIN
                && m.position <= STRIKING_POS_MAX
            {
                striking_candidates.push(StrikingDistanceSample {
                    slug: slug.clone(),
                    title: Some(article.title.clone()),
                    impressions: m.impressions,
                    clicks: m.clicks,
                    avg_position: m.position,
                    ctr: Some(ctr),
                });
            }
        }

        // Zero-impression published inventory: missing rollup ≡ 0 impressions.
        // Degraded path (no usable GSC tape) is applied after freshness is known.
        if article.status == "published" {
            let impressions = recent.map(|m| m.impressions).unwrap_or(0.0);
            if impressions == 0.0 {
                zero_impression_candidates.push(ZeroImpressionSample {
                    slug: slug.clone(),
                    title: Some(article.title.clone()),
                    status: Some(article.status.clone()),
                });
            }
        }

        if !pages.is_empty() {
            let (prev, _) = rollup_for_slug(&page_index, &prev_by_page, &slug);
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
        .filter(|s| {
            !crate::gsc::indexing::is_non_actionable_reason(
                s.last_reason_code.as_deref().unwrap_or(""),
            )
        })
        .collect();
    // Global GSC count may include live-site-only paths; sample is catalog-only
    // so create-task -S can act on every sample slug (issue #179 residual D).
    let not_indexed_count = not_indexed_rows.len();
    let catalog_slugs: HashSet<String> = articles
        .iter()
        .map(|a| normalize_url_slug(&a.url_slug))
        .filter(|s| !s.is_empty())
        .collect();
    let not_indexed_sample: Vec<NotIndexedSample> = not_indexed_rows
        .into_iter()
        .filter_map(|s| {
            let extracted = extract_slug_from_url(&s.url);
            if extracted.is_empty() {
                return None;
            }
            let slug = resolve_slug(&extracted, &catalog_slugs)?;
            Some(NotIndexedSample {
                slug,
                reason: s
                    .last_reason_code
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            })
        })
        .take(10)
        .collect();

    let avg_ctr = safe_ctr(total_clicks, total_impressions);
    let freshness = build_gsc_freshness(conn, project_id);

    // Zero-impression: never treat the whole catalog as dead weight when GSC
    // tape is missing (issue #204). Prefer has_any_gsc OR freshness.source.
    let zero_impression = if !has_any_gsc || freshness.source == "none" {
        ZeroImpressionInventory {
            count: 0,
            sample: vec![],
            degraded_reason: Some("gsc_missing".into()),
        }
    } else {
        zero_impression_candidates.sort_by(|a, b| a.slug.cmp(&b.slug));
        let count = zero_impression_candidates.len();
        zero_impression_candidates.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
        ZeroImpressionInventory {
            count,
            sample: zero_impression_candidates,
            degraded_reason: None,
        }
    };

    striking_candidates.sort_by(|a, b| {
        b.impressions
            .partial_cmp(&a.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let striking_count = striking_candidates.len();
    striking_candidates.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
    let striking_distance = StrikingDistanceInventory {
        count: striking_count,
        sample: striking_candidates,
    };

    let hard_cannibalization = build_hard_cannibalization_inventory(conn, project_id, &articles);

    // Residual equity on redirect sources + never-catalog high-impr GSC (#261).
    let redirect_equity = build_redirect_equity_inventory(
        &redirect_map,
        &page_index,
        &recent_by_page,
        &live_catalog_slugs,
    );
    let non_catalog_gsc = build_non_catalog_gsc_inventory(
        &page_index,
        &recent_by_page,
        &live_catalog_slugs,
        &redirected,
    );

    let hints = build_hints(
        has_any_gsc,
        &freshness,
        &top_candidates,
        multi_url_slug_count,
        &zero_impression,
        &striking_distance,
        &hard_cannibalization,
        &redirect_equity,
        &non_catalog_gsc,
    );

    Ok(SiteOverview {
        project_id: project_id.to_string(),
        generated_at,
        freshness,
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
        zero_impression,
        striking_distance,
        hard_cannibalization,
        redirect_equity,
        non_catalog_gsc,
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
        let (metrics, url_variants) = rollup_for_slug(&page_index, &metrics_by_page, &slug);
        let top_queries = load_top_queries(conn, project_id, article.id);
        let indexing_status = indexing_by_slug.get(&slug).cloned();

        let row = build_catalog_row(
            article,
            period_days,
            metrics.as_ref(),
            url_variants,
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
        freshness: build_gsc_freshness(conn, project_id),
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
    // Issue #166: sum all URL variants that normalize to the catalog slug.
    let metrics_by_page =
        db::gsc_page_daily_window_metrics_bulk(conn, project_id, &start, &end)?;
    let (metrics, url_variants) = rollup_for_slug(&page_index, &metrics_by_page, &normalized_slug);
    let top_queries = load_top_queries(conn, project_id, article.id);
    let indexing_status = indexing_by_slug.get(&normalized_slug).cloned();

    // Single MDX materialization → SERP enrichment + content package.
    let materialized = materialize_article(project_path, article);
    let catalog = build_catalog_row(
        article,
        period_days,
        metrics.as_ref(),
        url_variants,
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
    url_variants: usize,
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
            url_variants,
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
    // Machine-readable FAQ only (frontmatter Q/A and/or parseable FAQPage JSON-LD).
    // Markdown ## FAQ prose alone does not set has_faq — see assess_faq_source.
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

    // Shared project-wide load (db::list_ctr_query_metrics_for_project).
    let all_rows = db::list_ctr_query_metrics_for_project(conn, project_id)?;

    let mut out = Vec::new();
    for q in queries.iter().take(20) {
        let q_lower = q.query.to_lowercase();
        let mut others: Vec<CannibalSlugMetric> = all_rows
            .iter()
            .filter(|row| row.article_id != article_id && row.query.to_lowercase() == q_lower)
            .filter_map(|row| {
                let slug = slug_by_id.get(&row.article_id)?.clone();
                Some(CannibalSlugMetric {
                    slug,
                    impressions: row.impressions,
                    clicks: row.clicks,
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

/// Hard same-query multi-URL inventory for site-overview (issue #204).
///
/// Soft TF-IDF / clusters are intentionally out of scope. Never fails the
/// overview: empty or unreadable metrics degrade to `ctr_query_metrics_empty`.
///
/// Grouping floor/cap/sort live in [`db::group_shared_query_articles`] (shared
/// with cannibalization audit). Desk maps article_ids → slugs + clicks and
/// truncates sample groups with [`OVERVIEW_INVENTORY_SAMPLE_CAP`].
fn build_hard_cannibalization_inventory(
    conn: &Connection,
    project_id: &str,
    articles: &[Article],
) -> HardCannibalizationInventory {
    let slug_by_id: HashMap<i64, String> = articles
        .iter()
        .map(|a| (a.id, normalize_url_slug(&a.url_slug)))
        .collect();

    let all_rows = match db::list_ctr_query_metrics_for_project(conn, project_id) {
        Ok(rows) if !rows.is_empty() => rows,
        _ => {
            return HardCannibalizationInventory {
                count: 0,
                sample: vec![],
                degraded_reason: Some("ctr_query_metrics_empty".into()),
            };
        }
    };

    // Clicks for the highest-impressions row per (query_lower, article_id).
    let mut clicks_for_best: HashMap<(String, i64), (f64, f64)> = HashMap::new();
    for row in &all_rows {
        let q_lower = row.query.to_lowercase();
        let key = (q_lower, row.article_id);
        clicks_for_best
            .entry(key)
            .and_modify(|(imp, clk)| {
                if row.impressions > *imp {
                    *imp = row.impressions;
                    *clk = row.clicks;
                }
            })
            .or_insert((row.impressions, row.clicks));
    }

    let groups = db::group_shared_query_articles(
        all_rows
            .iter()
            .map(|r| (r.query.clone(), r.article_id, r.impressions)),
    );

    let mut sample_groups: Vec<(String, Vec<HardCannibalSlugMetric>)> = groups
        .into_iter()
        .filter_map(|(query, pages)| {
            let slugs: Vec<HardCannibalSlugMetric> = pages
                .into_iter()
                .filter_map(|(aid, imp)| {
                    let slug = slug_by_id.get(&aid)?.clone();
                    let clicks = clicks_for_best
                        .get(&(query.clone(), aid))
                        .map(|(_, c)| *c);
                    Some(HardCannibalSlugMetric {
                        slug,
                        impressions: imp,
                        clicks,
                    })
                })
                .collect();
            // Drop groups that lose cardinality after slug resolution.
            if slugs.len() < 2 {
                return None;
            }
            Some((query, slugs))
        })
        .collect();

    let count = sample_groups.len();
    sample_groups.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
    let sample = sample_groups
        .into_iter()
        .map(|(query, slugs)| HardCannibalizationSample { query, slugs })
        .collect();

    HardCannibalizationInventory {
        count,
        sample,
        degraded_reason: None,
    }
}

// GSC page-index / window / rollup helpers live in `db::gsc_join` (shared with
// territory analysis — single SoT, issue #167 / PR #176).

/// Desk GSC tape freshness from `gsc_page_daily` only (issue #164).
///
/// Does **not** consult `ctr_query_metrics` — live `gsc-*` tools are the
/// dual-path for API freshness; desk rollups expose tape usability here.
fn build_gsc_freshness(conn: &Connection, project_id: &str) -> Freshness {
    use crate::engine::exec::common::GSC_METRICS_MAX_AGE_DAYS;

    let page_max: Option<String> = conn
        .query_row(
            "SELECT MAX(fetched_at) FROM gsc_page_daily WHERE project_id = ?1",
            rusqlite::params![project_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    let Some(gsc_at) = page_max else {
        return Freshness {
            gsc_at: None,
            age_days: None,
            stale: true,
            source: "none".into(),
            hint: Some(
                "Desk GSC tape empty — use gsc-performance / gsc-queries or run collect_gsc then re-read desk"
                    .into(),
            ),
            evidence_index_at: None,
            evidence_coverage: 0.0,
        };
    };

    let (age_days, stale) = match chrono::DateTime::parse_from_rfc3339(&gsc_at) {
        Ok(synced_at) => {
            let age = Utc::now().signed_duration_since(synced_at);
            let days = age.num_days();
            let stale = age > Duration::days(GSC_METRICS_MAX_AGE_DAYS);
            (Some(days), stale)
        }
        // Unparseable timestamp: treat as unusable (stale) without inventing age.
        Err(_) => (None, true),
    };

    let hint = if stale {
        Some(
            "Desk GSC tape stale — use gsc-performance / gsc-queries or run collect_gsc then re-read desk"
                .into(),
        )
    } else {
        None
    };

    Freshness {
        gsc_at: Some(gsc_at),
        age_days,
        stale,
        source: "gsc_page_daily".into(),
        hint,
        evidence_index_at: None,
        evidence_coverage: 0.0,
    }
}

fn indexing_status_map(conn: &Connection, project_id: &str) -> HashMap<String, String> {
    let rows = crate::gsc::db::list_by_project(conn, project_id).unwrap_or_default();
    let mut map = HashMap::new();
    for row in rows {
        let slug = extract_slug_from_url(&row.url);
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

/// Residual GSC still attributed to redirect sources, mapped to destinations (#261).
fn build_redirect_equity_inventory(
    redirect_map: &HashMap<String, String>,
    page_index: &HashMap<String, Vec<String>>,
    recent_by_page: &HashMap<String, GscDailyWindowMetrics>,
    live_catalog_slugs: &HashSet<String>,
) -> RedirectEquityInventory {
    let mut candidates: Vec<RedirectEquitySample> = Vec::new();
    for (source_slug, dest_slug) in redirect_map {
        let (source_metrics, _) = rollup_for_slug(page_index, recent_by_page, source_slug);
        let Some(src) = source_metrics else {
            continue;
        };
        // Skip zero residual impressions (noise / no tape on source).
        if src.impressions <= 0.0 {
            continue;
        }
        let (dest_metrics, _) = rollup_for_slug(page_index, recent_by_page, dest_slug);
        let (dest_impr, dest_clicks) = match dest_metrics {
            Some(m) => (m.impressions, m.clicks),
            None => (0.0, 0.0),
        };
        candidates.push(RedirectEquitySample {
            source_slug: source_slug.clone(),
            destination_slug: dest_slug.clone(),
            source_impressions: src.impressions,
            source_clicks: src.clicks,
            destination_impressions: dest_impr,
            destination_clicks: dest_clicks,
            destination_in_catalog: live_catalog_slugs.contains(dest_slug),
        });
    }
    candidates.sort_by(|a, b| {
        b.source_impressions
            .partial_cmp(&a.source_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let count = candidates.len();
    candidates.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
    RedirectEquityInventory {
        count,
        sample: candidates,
    }
}

/// High-impression GSC pages outside live catalog and redirect map (#261).
fn build_non_catalog_gsc_inventory(
    page_index: &HashMap<String, Vec<String>>,
    recent_by_page: &HashMap<String, GscDailyWindowMetrics>,
    live_catalog_slugs: &HashSet<String>,
    redirect_sources: &HashSet<String>,
) -> NonCatalogGscInventory {
    let mut candidates: Vec<NonCatalogGscSample> = Vec::new();
    for slug in page_index.keys() {
        if live_catalog_slugs.contains(slug) {
            continue;
        }
        // Mapped redirect sources belong in redirect_equity only.
        if redirect_sources.contains(slug) {
            continue;
        }
        let (metrics, _) = rollup_for_slug(page_index, recent_by_page, slug);
        let Some(m) = metrics else {
            continue;
        };
        if m.impressions < NON_CATALOG_GSC_MIN_IMPRESSIONS {
            continue;
        }
        candidates.push(NonCatalogGscSample {
            slug: slug.clone(),
            impressions: m.impressions,
            clicks: m.clicks,
            kind: "unknown".into(),
        });
    }
    candidates.sort_by(|a, b| {
        b.impressions
            .partial_cmp(&a.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let count = candidates.len();
    candidates.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
    NonCatalogGscInventory {
        count,
        sample: candidates,
    }
}

fn build_hints(
    has_any_gsc: bool,
    freshness: &Freshness,
    top_pages: &[TopPage],
    multi_url_slug_count: usize,
    zero_impression: &ZeroImpressionInventory,
    striking_distance: &StrikingDistanceInventory,
    hard_cannibalization: &HardCannibalizationInventory,
    redirect_equity: &RedirectEquityInventory,
    non_catalog_gsc: &NonCatalogGscInventory,
) -> Vec<String> {
    let mut hints = Vec::new();
    // Always surface missing/stale tape for agents scanning hints only (#164).
    if freshness.source == "none" || !has_any_gsc {
        hints.push("GSC snapshots missing".into());
    } else if freshness.stale {
        hints.push("GSC page-daily tape stale".into());
    }
    if top_pages
        .iter()
        .any(|p| p.impressions >= 1000.0 && p.ctr < 0.01)
    {
        hints.push("High-impression low-CTR pages present".into());
    }
    if multi_url_slug_count > 0 {
        hints.push(format!(
            "GSC multi-URL inventory: {multi_url_slug_count} catalog slugs map to >1 page URL"
        ));
    }
    // Inventory flags: one short string per non-empty, non-degraded set (#204).
    if zero_impression.count > 0 && zero_impression.degraded_reason.is_none() {
        hints.push("Zero-impression published inventory present".into());
    }
    if striking_distance.count > 0 {
        hints.push("Striking-distance pages present".into());
    }
    if hard_cannibalization.count > 0 && hard_cannibalization.degraded_reason.is_none() {
        hints.push("Hard same-query cannibal samples present".into());
    }
    // Residual equity inventories (#261).
    if redirect_equity.count > 0 {
        hints.push("Redirect residual equity present".into());
    }
    if non_catalog_gsc.count > 0 {
        hints.push("Non-catalog residual GSC present".into());
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
