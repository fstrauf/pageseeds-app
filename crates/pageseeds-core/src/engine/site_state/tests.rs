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
        position: 8.0,
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
