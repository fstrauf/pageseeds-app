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
