//! Fixture tests for Site State builders (issue #120).

use super::*;
use chrono::{Duration, Utc};
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

fn in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::db::init_with_conn(&conn).unwrap();
    conn
}

fn temp_project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pageseeds-site-state-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join(".github/automation")).unwrap();
    fs::create_dir_all(dir.join("content")).unwrap();
    dir
}

fn insert_project(conn: &Connection, id: &str, path: &str) {
    conn.execute(
        "INSERT INTO projects (id, name, path, active, project_mode)
         VALUES (?1, 'Test', ?2, 1, 'workspace')",
        rusqlite::params![id, path],
    )
    .unwrap();
}

fn insert_article(
    conn: &Connection,
    project_id: &str,
    id: i64,
    slug: &str,
    title: &str,
    file: &str,
    status: &str,
    word_count: i64,
) {
    conn.execute(
        "INSERT INTO articles (
            id, project_id, title, url_slug, file, status, target_keyword,
            content_gaps_addressed, target_volume, word_count, review_count, content_hash
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'keyword', '[]', 0, ?7, 0, 'hash-abc')",
        rusqlite::params![id, project_id, title, slug, file, status, word_count],
    )
    .unwrap();
}

fn write_mdx(project: &std::path::Path, rel: &str, body: &str) {
    let path = project.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn daily_row(page: &str, date: &str, clicks: f64, impressions: f64) -> crate::models::gsc::PageDailyMetrics {
    daily_row_at(page, date, clicks, impressions, 8.0)
}

/// Like [`daily_row`] but with an explicit position (for striking-distance band tests).
fn daily_row_at(
    page: &str,
    date: &str,
    clicks: f64,
    impressions: f64,
    position: f64,
) -> crate::models::gsc::PageDailyMetrics {
    crate::models::gsc::PageDailyMetrics {
        page: page.to_string(),
        date: date.to_string(),
        clicks,
        impressions,
        ctr: if impressions > 0.0 {
            clicks / impressions
        } else {
            0.0
        },
        position,
    }
}

/// Dates inside the most recent 28-day window (ending yesterday).
fn recent_dates() -> (String, String) {
    let end = Utc::now().date_naive() - Duration::days(1);
    let mid = end - Duration::days(5);
    (
        mid.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

/// Dates inside the previous 28-day window.
fn previous_dates() -> (String, String) {
    let recent_end = Utc::now().date_naive() - Duration::days(1);
    let recent_start = recent_end - Duration::days(27);
    let prev_end = recent_start - Duration::days(1);
    let prev_mid = prev_end - Duration::days(5);
    (
        prev_mid.format("%Y-%m-%d").to_string(),
        prev_end.format("%Y-%m-%d").to_string(),
    )
}

#[test]
fn redirected_excluded_from_default_articles_list() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(
        &conn, "proj1", 1, "live-post", "Live Post", "content/live.mdx", "published", 100,
    );
    insert_article(
        &conn,
        "proj1",
        2,
        "old-merged-post",
        "Old Merged",
        "content/old.mdx",
        "published",
        50,
    );

    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n/blog/old-merged-post,/blog/live-post,301\n",
    )
    .unwrap();

    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter {
            include_redirected: false,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(catalog.count, 1);
    assert_eq!(catalog.articles[0].slug, "live-post");
    assert!(!catalog.filter.include_redirected);

    let with_redir = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter {
            include_redirected: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(with_redir.count, 2);
    assert!(with_redir.articles.iter().any(|a| a.slug == "old-merged-post"));

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn site_overview_totals_articles_live() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(&conn, "proj1", 1, "alpha", "Alpha", "content/a.mdx", "published", 200);
    insert_article(&conn, "proj1", 2, "beta", "Beta", "content/b.mdx", "published", 150);
    insert_article(&conn, "proj1", 3, "gone", "Gone", "content/g.mdx", "published", 10);

    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n/blog/gone,/blog/alpha,301\n",
    )
    .unwrap();

    let (d1, d2) = recent_dates();
    let rows = vec![
        daily_row("https://example.com/blog/alpha", &d1, 5.0, 100.0),
        daily_row("https://example.com/blog/alpha", &d2, 10.0, 200.0),
        daily_row("https://example.com/blog/beta", &d1, 1.0, 50.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.totals.articles_live, 2);
    assert_eq!(overview.totals.articles_redirected, 1);
    assert!(overview.totals.impressions > 0.0);
    assert!(!overview.top_pages.is_empty());
    assert!(overview.hints.iter().any(|h| h.contains("Evidence index")));
    assert!(overview.freshness.evidence_index_at.is_none());
    assert_eq!(overview.freshness.evidence_coverage, 0.0);
    // Fresh insert → not stale (tape age ≤ GSC_METRICS_MAX_AGE_DAYS).
    assert!(!overview.freshness.stale);
    assert_eq!(overview.freshness.source, "gsc_page_daily");
    assert!(overview.freshness.gsc_at.is_some());
    assert!(overview.freshness.age_days.is_some());
    assert!(overview.freshness.hint.is_none());

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn article_package_has_outline_body_and_empty_neighbors() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    write_mdx(
        &project,
        "content/guide.mdx",
        r#"---
title: Complete Guide
description: A useful guide about widgets
faq:
  - question: "What is a widget?"
    answer: "A widget is a small mechanical device."
  - question: "How do I install a widget?"
    answer: "Follow the setup steps in this guide."
  - question: "When should I replace a widget?"
    answer: "Replace when wear indicators show."
---

# Complete Guide

Intro paragraph about widgets.

## Setup

Setup steps here.

## FAQ

### What is a widget?
"#,
    );
    insert_article(
        &conn,
        "proj1",
        10,
        "complete-guide",
        "Complete Guide",
        "content/guide.mdx",
        "published",
        0,
    );

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row(
            "https://example.com/blog/complete-guide",
            &d1,
            2.0,
            40.0,
        )],
    )
    .unwrap();

    crate::db::set_ctr_query_metrics(
        &conn,
        "proj1",
        10,
        "https://example.com/blog/complete-guide",
        &[(
            "widget guide".into(),
            40.0,
            2.0,
            0.05,
            7.0,
            None,
        )],
        Some("2026-01-01"),
        Some("2026-01-28"),
    )
    .unwrap();

    let pkg = get_article_package(&conn, "proj1", &project_path, "complete-guide", Some(28))
        .unwrap();

    assert_eq!(pkg.article_id, 10);
    assert_eq!(pkg.slug, "complete-guide");
    assert!(pkg.content.body_markdown.contains("Intro paragraph"));
    assert!(
        pkg.content
            .outline
            .iter()
            .any(|h| h.level == 2 && h.text == "Setup")
    );
    assert_eq!(pkg.neighbors.len(), 0);
    assert!(!pkg.catalog.evidence.has_embedding);
    assert!(pkg.catalog.neighbors.is_empty());
    assert!(pkg.validation.ok);
    assert!(pkg.validation.checks.is_empty());
    assert!(!pkg.queries.is_empty());
    assert_eq!(pkg.queries[0].query, "widget guide");
    assert!(pkg.catalog.word_count > 0);
    assert_eq!(pkg.catalog.h1.as_deref(), Some("Complete Guide"));
    assert!(pkg.catalog.serp.has_faq);
    assert_eq!(
        pkg.catalog.serp.meta_description.as_deref(),
        Some("A useful guide about widgets")
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn neighbors_always_array_never_null() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 1, "solo", "Solo", "content/s.mdx", "draft", 10);

    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter::default(),
    )
    .unwrap();
    let json = serde_json::to_value(&catalog).unwrap();
    assert!(json["articles"][0]["neighbors"].is_array());
    assert_eq!(json["articles"][0]["neighbors"].as_array().unwrap().len(), 0);
    assert_eq!(json["articles"][0]["evidence"]["has_embedding"], false);

    let pkg = get_article_package(&conn, "proj1", &project_path, "solo", None).unwrap();
    let pkg_json = serde_json::to_value(&pkg).unwrap();
    assert!(pkg_json["neighbors"].is_array());
    assert!(!pkg_json["neighbors"].is_null());

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn top_movers_empty_without_prior_window() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 1, "only-recent", "Only Recent", "content/o.mdx", "published", 20);

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row(
            "https://example.com/blog/only-recent",
            &d1,
            3.0,
            30.0,
        )],
    )
    .unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    // Only recent window has data → no movers pair.
    assert!(overview.top_movers.is_empty());

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn top_movers_computed_when_both_windows_exist() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 1, "mover", "Mover", "content/m.mdx", "published", 20);

    let (r1, r2) = recent_dates();
    let (p1, p2) = previous_dates();
    let rows = vec![
        daily_row("https://example.com/blog/mover", &r1, 20.0, 200.0),
        daily_row("https://example.com/blog/mover", &r2, 10.0, 100.0),
        daily_row("https://example.com/blog/mover", &p1, 2.0, 50.0),
        daily_row("https://example.com/blog/mover", &p2, 1.0, 50.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.top_movers.len(), 1);
    assert_eq!(overview.top_movers[0].slug, "mover");
    assert!(overview.top_movers[0].clicks_delta > 0.0);
    assert_eq!(overview.top_movers[0].direction, "up");

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn gsc_freshness_empty_tape_is_stale_with_hint() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(
        &conn, "proj1", 1, "lonely", "Lonely", "content/l.mdx", "published", 10,
    );

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert!(overview.freshness.stale);
    assert_eq!(overview.freshness.source, "none");
    assert!(overview.freshness.gsc_at.is_none());
    assert!(overview.freshness.age_days.is_none());
    let hint = overview.freshness.hint.as_deref().unwrap_or("");
    assert!(
        hint.contains("empty") || hint.contains("collect_gsc"),
        "expected recovery hint, got {hint:?}"
    );
    assert!(
        overview
            .hints
            .iter()
            .any(|h| h.contains("GSC snapshots missing") || h.to_lowercase().contains("stale")),
        "overview hints should signal missing/stale GSC: {:?}",
        overview.hints
    );

    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter::default(),
    )
    .unwrap();
    assert!(catalog.freshness.stale);
    assert_eq!(catalog.freshness.source, "none");
    assert!(catalog.freshness.hint.is_some());

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn gsc_freshness_old_tape_is_stale_with_age() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(
        &conn, "proj1", 1, "aged", "Aged", "content/a.mdx", "published", 10,
    );

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row(
            "https://example.com/blog/aged",
            &d1,
            1.0,
            20.0,
        )],
    )
    .unwrap();

    // insert_gsc_page_daily_snapshots stamps fetched_at = now; backdate past threshold.
    let old_fetched = (Utc::now() - Duration::days(10)).to_rfc3339();
    conn.execute(
        "UPDATE gsc_page_daily SET fetched_at = ?1 WHERE project_id = ?2",
        rusqlite::params![old_fetched, "proj1"],
    )
    .unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert!(overview.freshness.stale);
    assert_eq!(overview.freshness.source, "gsc_page_daily");
    assert_eq!(overview.freshness.gsc_at.as_deref(), Some(old_fetched.as_str()));
    assert!(
        overview.freshness.age_days.unwrap_or(0)
            > crate::engine::exec::common::GSC_METRICS_MAX_AGE_DAYS
    );
    let hint = overview.freshness.hint.as_deref().unwrap_or("");
    assert!(
        hint.contains("stale") || hint.contains("collect_gsc"),
        "expected recovery hint, got {hint:?}"
    );
    assert!(
        overview
            .hints
            .iter()
            .any(|h| h.contains("stale") || h.contains("GSC snapshots missing")),
        "overview hints should signal stale GSC tape: {:?}",
        overview.hints
    );

    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter::default(),
    )
    .unwrap();
    assert!(catalog.freshness.stale);
    assert_eq!(catalog.freshness.source, "gsc_page_daily");
    assert!(catalog.freshness.age_days.is_some());
    assert!(catalog.freshness.hint.is_some());

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn gsc_freshness_fresh_tape_not_stale() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(
        &conn, "proj1", 1, "fresh", "Fresh", "content/f.mdx", "published", 10,
    );

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row(
            "https://example.com/blog/fresh",
            &d1,
            2.0,
            40.0,
        )],
    )
    .unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert!(!overview.freshness.stale);
    assert_eq!(overview.freshness.source, "gsc_page_daily");
    assert!(overview.freshness.gsc_at.is_some());
    assert!(overview.freshness.age_days.unwrap_or(99) <= 7);
    assert!(overview.freshness.hint.is_none());
    assert!(
        !overview.hints.iter().any(|h| h.contains("stale")),
        "fresh tape must not emit stale hint: {:?}",
        overview.hints
    );
    assert!(
        !overview.hints.iter().any(|h| h == "GSC snapshots missing"),
        "fresh tape with metrics must not claim snapshots missing: {:?}",
        overview.hints
    );

    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter::default(),
    )
    .unwrap();
    assert!(!catalog.freshness.stale);
    assert_eq!(catalog.freshness.source, "gsc_page_daily");
    assert!(catalog.freshness.hint.is_none());

    let _ = fs::remove_dir_all(&project);
}

/// Issue #166: underscore + hyphen (and trailing-slash) GSC page URLs that
/// normalize to the same catalog slug must sum impressions/clicks; url_variants
/// exposes the multi-URL inventory.
#[test]
fn gsc_url_variants_summed_into_desk_rollups() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    // Catalog slug uses hyphens (normalized form).
    insert_article(
        &conn,
        "proj1",
        1,
        "digital-marketing-nz-guide",
        "Digital Marketing NZ Guide",
        "content/guide.mdx",
        "published",
        500,
    );
    // Single-page control article.
    insert_article(
        &conn,
        "proj1",
        2,
        "solo-page",
        "Solo Page",
        "content/solo.mdx",
        "published",
        100,
    );

    let (d1, d2) = recent_dates();
    // Two GSC page keys that normalize to digital-marketing-nz-guide:
    // underscore path + hyphen path with trailing slash.
    let rows = vec![
        daily_row(
            "https://example.com/blog/digital_marketing_nz_guide",
            &d1,
            4.0,
            100.0,
        ),
        daily_row(
            "https://example.com/blog/digital_marketing_nz_guide",
            &d2,
            6.0,
            150.0,
        ),
        daily_row(
            "https://example.com/blog/digital-marketing-nz-guide/",
            &d1,
            3.0,
            50.0,
        ),
        daily_row(
            "https://example.com/blog/digital-marketing-nz-guide/",
            &d2,
            2.0,
            50.0,
        ),
        // Single-page control: one URL only.
        daily_row("https://example.com/blog/solo-page", &d1, 1.0, 20.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    // Expected sums for multi-variant slug:
    // clicks: 4+6+3+2 = 15, impressions: 100+150+50+50 = 350
    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter::default(),
    )
    .unwrap();

    let multi = catalog
        .articles
        .iter()
        .find(|a| a.slug == "digital-marketing-nz-guide")
        .expect("multi-variant article in catalog");
    assert_eq!(multi.gsc.clicks, 15.0);
    assert_eq!(multi.gsc.impressions, 350.0);
    assert_eq!(multi.gsc.url_variants, 2);

    let solo = catalog
        .articles
        .iter()
        .find(|a| a.slug == "solo-page")
        .expect("solo article in catalog");
    assert_eq!(solo.gsc.clicks, 1.0);
    assert_eq!(solo.gsc.impressions, 20.0);
    assert_eq!(solo.gsc.url_variants, 1);

    // Package path also merges + reports url_variants.
    let pkg = get_article_package(
        &conn,
        "proj1",
        &project_path,
        "digital-marketing-nz-guide",
        Some(28),
    )
    .unwrap();
    assert_eq!(pkg.catalog.gsc.clicks, 15.0);
    assert_eq!(pkg.catalog.gsc.impressions, 350.0);
    assert_eq!(pkg.catalog.gsc.url_variants, 2);

    let pkg_solo =
        get_article_package(&conn, "proj1", &project_path, "solo-page", Some(28)).unwrap();
    assert_eq!(pkg_solo.catalog.gsc.url_variants, 1);
    assert_eq!(pkg_solo.catalog.gsc.impressions, 20.0);

    // Site overview totals include the summed multi-variant metrics.
    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    // multi 350 + solo 20 = 370 impressions; multi 15 + solo 1 = 16 clicks
    assert_eq!(overview.totals.impressions, 370.0);
    assert_eq!(overview.totals.clicks, 16.0);
    let top = overview
        .top_pages
        .iter()
        .find(|p| p.slug == "digital-marketing-nz-guide")
        .expect("multi-variant in top pages");
    assert_eq!(top.impressions, 350.0);
    assert_eq!(top.clicks, 15.0);
    assert!(
        overview
            .hints
            .iter()
            .any(|h| h.contains("GSC multi-URL inventory")),
        "overview should hint multi-URL inventory: {:?}",
        overview.hints
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn min_impressions_filter() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 1, "hot", "Hot", "content/h.mdx", "published", 10);
    insert_article(&conn, "proj1", 2, "cold", "Cold", "content/c.mdx", "published", 10);

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[
            daily_row("https://example.com/blog/hot", &d1, 5.0, 500.0),
            daily_row("https://example.com/blog/cold", &d1, 0.0, 5.0),
        ],
    )
    .unwrap();

    let catalog = list_articles_catalog(
        &conn,
        "proj1",
        &project_path,
        ArticlesFilter {
            min_impressions: 100.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(catalog.count, 1);
    assert_eq!(catalog.articles[0].slug, "hot");

    let _ = fs::remove_dir_all(&project);
}

/// Issue #179 residual D: `not_indexed_sample` only includes catalog-resolvable
/// slugs so operators can `create-task -S` every sample entry.
#[test]
fn not_indexed_sample_filters_to_catalog_slugs() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    // Catalog tracks one article; GSC has that plus a live-site-only path.
    insert_article(
        &conn,
        "proj1",
        1,
        "catalog-post",
        "Catalog Post",
        "content/catalog-post.mdx",
        "published",
        200,
    );

    let now = Utc::now().to_rfc3339();
    let in_catalog = crate::gsc::db::UrlIndexingStatus {
        url: "https://example.com/blog/catalog-post".into(),
        project_id: "proj1".into(),
        last_inspected_at: Some(now.clone()),
        last_reason_code: Some("crawled_currently_not_indexed".into()),
        last_verdict: Some("fail".into()),
        last_action: None,
        consecutive_passes: 0,
        last_task_created_at: None,
        last_task_type: None,
        last_task_id: None,
        last_fix_summary: None,
        fix_attempt_count: 0,
        last_task_resolved_at: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let live_only = crate::gsc::db::UrlIndexingStatus {
        url: "https://example.com/blog/live-site-only-path".into(),
        project_id: "proj1".into(),
        last_inspected_at: Some(now.clone()),
        last_reason_code: Some("discovered_currently_not_indexed".into()),
        last_verdict: Some("fail".into()),
        last_action: None,
        consecutive_passes: 0,
        last_task_created_at: None,
        last_task_type: None,
        last_task_id: None,
        last_fix_summary: None,
        fix_attempt_count: 0,
        last_task_resolved_at: None,
        created_at: now.clone(),
        updated_at: now,
    };
    // Insert live-only first so raw first-10 sampling would surface it if
    // filtering were absent.
    crate::gsc::db::upsert_status(&conn, &live_only).unwrap();
    crate::gsc::db::upsert_status(&conn, &in_catalog).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();

    // Global count still includes all GSC not-indexed rows.
    assert_eq!(overview.totals.not_indexed, 2);
    // Sample is catalog-only — never the live-site-only slug alone/unlabeled.
    assert_eq!(overview.not_indexed_sample.len(), 1);
    assert_eq!(overview.not_indexed_sample[0].slug, "catalog-post");
    assert!(
        !overview
            .not_indexed_sample
            .iter()
            .any(|s| s.slug.contains("live-site-only")),
        "non-catalog GSC paths must not appear in sample: {:?}",
        overview.not_indexed_sample
    );

    let _ = fs::remove_dir_all(&project);
}

// ── Issue #204: zero_impression / striking_distance / hard_cannibalization ───

#[test]
fn zero_impression_counts_published_with_zero_or_missing_gsc() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    // Live published with traffic — should NOT be zero-impression.
    insert_article(
        &conn, "proj1", 1, "has-traffic", "Has Traffic", "content/t.mdx", "published", 100,
    );
    // Live published missing GSC rows → treat as 0 impressions.
    insert_article(
        &conn, "proj1", 2, "no-gsc-rows", "No Gsc", "content/n.mdx", "published", 50,
    );
    // Live published with explicit 0 impressions.
    insert_article(
        &conn, "proj1", 3, "zero-impr", "Zero Impr", "content/z.mdx", "published", 40,
    );
    // Draft (not published) with 0 impressions — excluded.
    insert_article(
        &conn, "proj1", 4, "draft-zero", "Draft Zero", "content/d.mdx", "draft", 20,
    );
    // Redirected published with 0 impressions — excluded (not live).
    insert_article(
        &conn, "proj1", 5, "redirected-zero", "Redirected", "content/r.mdx", "published", 10,
    );
    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n/blog/redirected-zero,/blog/has-traffic,301\n",
    )
    .unwrap();

    let (d1, _) = recent_dates();
    let rows = vec![
        daily_row("https://example.com/blog/has-traffic", &d1, 5.0, 100.0),
        daily_row("https://example.com/blog/zero-impr", &d1, 0.0, 0.0),
        // draft-zero / no-gsc-rows / redirected-zero have no rows
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();

    assert!(overview.zero_impression.degraded_reason.is_none());
    assert_eq!(overview.zero_impression.count, 2);
    let slugs: Vec<&str> = overview
        .zero_impression
        .sample
        .iter()
        .map(|s| s.slug.as_str())
        .collect();
    // Sample sorted by slug asc.
    assert_eq!(slugs, vec!["no-gsc-rows", "zero-impr"]);
    assert!(overview
        .hints
        .iter()
        .any(|h| h == "Zero-impression published inventory present"));

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn zero_impression_degraded_when_gsc_tape_missing() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(
        &conn, "proj1", 1, "lonely", "Lonely", "content/l.mdx", "published", 10,
    );

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.freshness.source, "none");
    assert_eq!(overview.zero_impression.count, 0);
    assert!(overview.zero_impression.sample.is_empty());
    assert_eq!(
        overview.zero_impression.degraded_reason.as_deref(),
        Some("gsc_missing")
    );
    assert!(
        !overview
            .hints
            .iter()
            .any(|h| h == "Zero-impression published inventory present")
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn striking_distance_inclusion_and_exclusion_band() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    // pos 8, impr 250 → included (default daily_row position is 8.0).
    insert_article(
        &conn, "proj1", 1, "in-band", "In Band", "content/i.mdx", "published", 100,
    );
    // pos 6, high impr → excluded (below STRIKING_POS_MIN).
    insert_article(
        &conn, "proj1", 2, "too-high", "Too High", "content/h.mdx", "published", 100,
    );
    // pos 14, high impr → excluded (above STRIKING_POS_MAX).
    insert_article(
        &conn, "proj1", 3, "too-low", "Too Low", "content/l.mdx", "published", 100,
    );
    // pos 10, impr 50 → excluded (below STRIKING_MIN_IMPRESSIONS).
    insert_article(
        &conn, "proj1", 4, "low-impr", "Low Impr", "content/m.mdx", "published", 100,
    );
    // pos 7 boundary + high impr → included.
    insert_article(
        &conn, "proj1", 5, "boundary-min", "Boundary Min", "content/b.mdx", "published", 100,
    );
    // pos 13 boundary + higher impr → included; should sort first by impr desc.
    insert_article(
        &conn, "proj1", 6, "boundary-max", "Boundary Max", "content/x.mdx", "published", 100,
    );

    let (d1, _) = recent_dates();
    let rows = vec![
        daily_row_at("https://example.com/blog/in-band", &d1, 10.0, 250.0, 8.0),
        daily_row_at("https://example.com/blog/too-high", &d1, 20.0, 500.0, 6.0),
        daily_row_at("https://example.com/blog/too-low", &d1, 5.0, 400.0, 14.0),
        daily_row_at("https://example.com/blog/low-impr", &d1, 1.0, 50.0, 10.0),
        daily_row_at("https://example.com/blog/boundary-min", &d1, 8.0, 220.0, 7.0),
        daily_row_at("https://example.com/blog/boundary-max", &d1, 12.0, 300.0, 13.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.striking_distance.count, 3);
    let slugs: Vec<&str> = overview
        .striking_distance
        .sample
        .iter()
        .map(|s| s.slug.as_str())
        .collect();
    // Sorted by impressions desc: boundary-max (300), in-band (250), boundary-min (220).
    assert_eq!(slugs, vec!["boundary-max", "in-band", "boundary-min"]);
    assert!(overview
        .hints
        .iter()
        .any(|h| h == "Striking-distance pages present"));

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn hard_cannibalization_multi_url_same_query() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(
        &conn, "proj1", 1, "page-a", "Page A", "content/a.mdx", "published", 100,
    );
    insert_article(
        &conn, "proj1", 2, "page-b", "Page B", "content/b.mdx", "published", 100,
    );
    insert_article(
        &conn, "proj1", 3, "page-c", "Page C", "content/c.mdx", "published", 100,
    );

    // Need some GSC tape so overview has_any_gsc is true (unrelated to hard cannibal).
    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row(
            "https://example.com/blog/page-a",
            &d1,
            1.0,
            20.0,
        )],
    )
    .unwrap();

    // Shared query across A and B with impr >= 10 each → hard cannibal group.
    crate::db::set_ctr_query_metrics(
        &conn,
        "proj1",
        1,
        "https://example.com/blog/page-a",
        &[(
            "shared widget query".into(),
            50.0,
            3.0,
            0.06,
            8.0,
            None,
        )],
        Some("2026-01-01"),
        Some("2026-01-28"),
    )
    .unwrap();
    crate::db::set_ctr_query_metrics(
        &conn,
        "proj1",
        2,
        "https://example.com/blog/page-b",
        &[(
            "Shared Widget Query".into(), // case-insensitive match
            30.0,
            1.0,
            0.033,
            9.0,
            None,
        )],
        Some("2026-01-01"),
        Some("2026-01-28"),
    )
    .unwrap();
    // Solo query on C only — not multi-URL.
    crate::db::set_ctr_query_metrics(
        &conn,
        "proj1",
        3,
        "https://example.com/blog/page-c",
        &[(
            "unique query only".into(),
            100.0,
            5.0,
            0.05,
            4.0,
            None,
        )],
        Some("2026-01-01"),
        Some("2026-01-28"),
    )
    .unwrap();
    // Shared query with below-threshold impressions should not form a group.
    // (Would need a second article too; skip — already covered by floor filter.)

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert!(overview.hard_cannibalization.degraded_reason.is_none());
    assert_eq!(overview.hard_cannibalization.count, 1);
    assert_eq!(overview.hard_cannibalization.sample.len(), 1);
    let group = &overview.hard_cannibalization.sample[0];
    assert_eq!(group.query, "shared widget query");
    assert_eq!(group.slugs.len(), 2);
    // Slugs sorted by impressions desc.
    assert_eq!(group.slugs[0].slug, "page-a");
    assert_eq!(group.slugs[0].impressions, 50.0);
    assert_eq!(group.slugs[1].slug, "page-b");
    assert_eq!(group.slugs[1].impressions, 30.0);
    assert!(overview
        .hints
        .iter()
        .any(|h| h == "Hard same-query cannibal samples present"));

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn hard_cannibalization_empty_metrics_is_degraded() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(
        &conn, "proj1", 1, "solo", "Solo", "content/s.mdx", "published", 10,
    );

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row("https://example.com/blog/solo", &d1, 1.0, 10.0)],
    )
    .unwrap();

    // No ctr_query_metrics rows at all.
    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.hard_cannibalization.count, 0);
    assert!(overview.hard_cannibalization.sample.is_empty());
    assert_eq!(
        overview.hard_cannibalization.degraded_reason.as_deref(),
        Some("ctr_query_metrics_empty")
    );
    assert!(
        !overview
            .hints
            .iter()
            .any(|h| h == "Hard same-query cannibal samples present")
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn hard_cannibalization_below_impression_floor_not_grouped() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);
    insert_article(
        &conn, "proj1", 1, "a", "A", "content/a.mdx", "published", 10,
    );
    insert_article(
        &conn, "proj1", 2, "b", "B", "content/b.mdx", "published", 10,
    );

    let (d1, _) = recent_dates();
    crate::db::insert_gsc_page_daily_snapshots(
        &conn,
        "proj1",
        &[daily_row("https://example.com/blog/a", &d1, 1.0, 10.0)],
    )
    .unwrap();

    // Same query on two articles but both under SHARED_QUERY_MIN_IMPRESSIONS (10).
    crate::db::set_ctr_query_metrics(
        &conn,
        "proj1",
        1,
        "https://example.com/blog/a",
        &[("thin query".into(), 5.0, 0.0, 0.0, 12.0, None)],
        Some("2026-01-01"),
        Some("2026-01-28"),
    )
    .unwrap();
    crate::db::set_ctr_query_metrics(
        &conn,
        "proj1",
        2,
        "https://example.com/blog/b",
        &[("thin query".into(), 9.0, 0.0, 0.0, 11.0, None)],
        Some("2026-01-01"),
        Some("2026-01-28"),
    )
    .unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    // Metrics exist so not degraded, but no group meets ≥2 articles at floor.
    assert!(overview.hard_cannibalization.degraded_reason.is_none());
    assert_eq!(overview.hard_cannibalization.count, 0);
    assert!(overview.hard_cannibalization.sample.is_empty());

    let _ = fs::remove_dir_all(&project);
}

// ── Issue #261: redirect_equity / non_catalog_gsc residual inventory ─────────

#[test]
fn redirect_equity_maps_residual_source_to_keeper_metrics() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    // Keeper B is live catalog; source A is redirected (may still have catalog row).
    insert_article(
        &conn, "proj1", 1, "keeper-b", "Keeper B", "content/b.mdx", "published", 200,
    );
    insert_article(
        &conn, "proj1", 2, "source-a", "Source A", "content/a.mdx", "published", 100,
    );
    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n/blog/source-a,/blog/keeper-b,301\n",
    )
    .unwrap();

    let (d1, _) = recent_dates();
    let rows = vec![
        // Residual demand still on redirected source A.
        daily_row("https://example.com/blog/source-a", &d1, 12.0, 400.0),
        // Keeper B also has traffic.
        daily_row("https://example.com/blog/keeper-b", &d1, 30.0, 800.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();

    assert_eq!(overview.redirect_equity.count, 1);
    assert_eq!(overview.redirect_equity.sample.len(), 1);
    let sample = &overview.redirect_equity.sample[0];
    assert_eq!(sample.source_slug, "source-a");
    assert_eq!(sample.destination_slug, "keeper-b");
    assert_eq!(sample.source_impressions, 400.0);
    assert_eq!(sample.source_clicks, 12.0);
    assert_eq!(sample.destination_impressions, 800.0);
    assert_eq!(sample.destination_clicks, 30.0);
    assert!(sample.destination_in_catalog);
    assert!(overview
        .hints
        .iter()
        .any(|h| h == "Redirect residual equity present"));

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn non_catalog_gsc_includes_never_catalog_excludes_mapped_redirect_sources() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(
        &conn, "proj1", 1, "live-post", "Live Post", "content/l.mdx", "published", 100,
    );
    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n/blog/old-landing,/blog/live-post,301\n",
    )
    .unwrap();

    let (d1, _) = recent_dates();
    let rows = vec![
        daily_row("https://example.com/blog/live-post", &d1, 5.0, 100.0),
        // Mapped redirect source with residual — belongs in redirect_equity only.
        daily_row("https://example.com/blog/old-landing", &d1, 8.0, 300.0),
        // Never-catalog high-impr page → non_catalog_gsc.
        daily_row("https://example.com/blog/copy-ai-alternative", &d1, 20.0, 773.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();

    // Redirect equity has the mapped source.
    assert_eq!(overview.redirect_equity.count, 1);
    assert_eq!(overview.redirect_equity.sample[0].source_slug, "old-landing");

    // Non-catalog has never-catalog only — not the mapped redirect source.
    assert_eq!(overview.non_catalog_gsc.count, 1);
    assert_eq!(overview.non_catalog_gsc.sample.len(), 1);
    assert_eq!(overview.non_catalog_gsc.sample[0].slug, "copy-ai-alternative");
    assert_eq!(overview.non_catalog_gsc.sample[0].impressions, 773.0);
    assert_eq!(overview.non_catalog_gsc.sample[0].clicks, 20.0);
    assert!(
        !overview
            .non_catalog_gsc
            .sample
            .iter()
            .any(|s| s.slug == "old-landing"),
        "mapped redirect source must not duplicate into non_catalog_gsc"
    );
    assert!(overview
        .hints
        .iter()
        .any(|h| h == "Non-catalog residual GSC present"));

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn top_pages_still_live_only_excludes_redirected_source_residual() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(
        &conn, "proj1", 1, "keeper", "Keeper", "content/k.mdx", "published", 100,
    );
    insert_article(
        &conn, "proj1", 2, "dead-source", "Dead Source", "content/d.mdx", "published", 50,
    );
    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n/blog/dead-source,/blog/keeper,301\n",
    )
    .unwrap();

    let (d1, _) = recent_dates();
    // Redirected source has higher residual than keeper.
    let rows = vec![
        daily_row("https://example.com/blog/dead-source", &d1, 50.0, 5000.0),
        daily_row("https://example.com/blog/keeper", &d1, 5.0, 100.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();

    // top_pages / totals stay live-catalog only.
    assert_eq!(overview.totals.articles_live, 1);
    assert_eq!(overview.totals.impressions, 100.0);
    assert_eq!(overview.totals.clicks, 5.0);
    assert!(
        !overview.top_pages.iter().any(|p| p.slug == "dead-source"),
        "redirected source must not appear in top_pages: {:?}",
        overview.top_pages
    );
    assert_eq!(overview.top_pages.len(), 1);
    assert_eq!(overview.top_pages[0].slug, "keeper");
    // Residual still visible via redirect_equity.
    assert_eq!(overview.redirect_equity.count, 1);
    assert_eq!(overview.redirect_equity.sample[0].source_impressions, 5000.0);

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn redirect_equity_skips_zero_residual_source() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(
        &conn, "proj1", 1, "keeper", "Keeper", "content/k.mdx", "published", 100,
    );
    fs::write(
        project.join(".github/automation/redirects.csv"),
        "source,destination,status\n\
         /blog/zero-source,/blog/keeper,301\n\
         /blog/hot-source,/blog/keeper,301\n",
    )
    .unwrap();

    let (d1, _) = recent_dates();
    let rows = vec![
        daily_row("https://example.com/blog/keeper", &d1, 2.0, 50.0),
        // Zero residual on source — skip.
        daily_row("https://example.com/blog/zero-source", &d1, 0.0, 0.0),
        // Positive residual — include.
        daily_row("https://example.com/blog/hot-source", &d1, 3.0, 120.0),
        // No GSC rows at all for a third source would also skip (not listed).
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.redirect_equity.count, 1);
    assert_eq!(overview.redirect_equity.sample[0].source_slug, "hot-source");
    assert!(
        !overview
            .redirect_equity
            .sample
            .iter()
            .any(|s| s.source_slug == "zero-source")
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn non_catalog_below_min_impressions_floor_excluded() {
    let conn = in_memory_db();
    let project = temp_project();
    let project_path = project.to_string_lossy().to_string();
    insert_project(&conn, "proj1", &project_path);

    insert_article(
        &conn, "proj1", 1, "live", "Live", "content/l.mdx", "published", 100,
    );

    let (d1, _) = recent_dates();
    let rows = vec![
        daily_row("https://example.com/blog/live", &d1, 1.0, 20.0),
        // Below NON_CATALOG_GSC_MIN_IMPRESSIONS (50).
        daily_row("https://example.com/blog/orphan-low", &d1, 1.0, 49.0),
        // At floor — included.
        daily_row("https://example.com/blog/orphan-ok", &d1, 2.0, 50.0),
    ];
    crate::db::insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

    let overview = build_site_overview(&conn, "proj1", &project_path, Some(28)).unwrap();
    assert_eq!(overview.non_catalog_gsc.count, 1);
    assert_eq!(overview.non_catalog_gsc.sample[0].slug, "orphan-ok");
    assert_eq!(overview.non_catalog_gsc.sample[0].impressions, 50.0);
    assert!(
        !overview
            .non_catalog_gsc
            .sample
            .iter()
            .any(|s| s.slug == "orphan-low")
    );

    let _ = fs::remove_dir_all(&project);
}
