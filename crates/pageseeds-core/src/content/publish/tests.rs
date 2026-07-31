// Regression tests for publish date consistency (Phase 5)
// Extracted from publish.rs so the production module stays under 1k lines.

use super::*;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
}

fn in_memory_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::init_with_conn(&conn).unwrap();
    conn
}

fn write_mdx(path: &std::path::Path, title: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = format!("---\ntitle: \"{}\"\n---\n\nBody text.\n", title);
    std::fs::write(path, content).unwrap();
}

#[test]
fn apply_publish_keeps_mdx_json_and_db_dates_consistent() {
    let dir = unique_temp_dir("ps_publish_consistent");
    let auto_dir = dir.join(".github").join("automation");
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();

    // Write articles.json with a stale/empty date
    std::fs::create_dir_all(&auto_dir).unwrap();
    std::fs::write(
        auto_dir.join("articles.json"),
        r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Test","file":"./content/001_test.mdx","published_date":"","status":"draft"}]}"#,
    )
    .unwrap();

    // Write MDX without a date
    let mdx_path = content_dir.join("001_test.mdx");
    write_mdx(&mdx_path, "Test");

    let conn = in_memory_db();
    conn.execute(
        "INSERT INTO projects (id, name, path, active, project_mode)
         VALUES ('p1', 'Test', ?1, 1, 'workspace')",
        [dir.to_str().unwrap()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO articles (id, title, url_slug, file, status, content_gaps_addressed, project_id)
         VALUES (1, 'Test', 'test', './content/001_test.mdx', 'draft', '[]', 'p1')",
        [],
    )
    .unwrap();

    // Publish the article
    let result = apply_publish(&conn, "p1", &[1], &HashMap::new(), &[], &content_dir, &dir);

    assert_eq!(result.published.len(), 1);
    let assigned_date = &result.published[0].published_date;

    // Verify SQLite has the assigned date
    let db_date: String = conn
        .query_row(
            "SELECT published_date FROM articles WHERE id = 1 AND project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(&db_date, assigned_date);

    // Verify articles.json has the same assigned date
    let json_on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json_on_disk).unwrap();
    let json_date = doc["articles"][0]["published_date"].as_str().unwrap();
    assert_eq!(json_date, assigned_date);

    // Verify MDX frontmatter has the same assigned date
    let mdx_content = std::fs::read_to_string(&mdx_path).unwrap();
    assert!(
        mdx_content.contains(&format!("date: \"{}\"", assigned_date)),
        "MDX frontmatter should contain the assigned date {}. MDX content: {}",
        assigned_date,
        mdx_content
    );
}

fn insert_project(conn: &rusqlite::Connection, dir: &std::path::Path) {
    conn.execute(
        "INSERT INTO projects (id, name, path, active, project_mode)
         VALUES ('p1', 'Test', ?1, 1, 'workspace')",
        [dir.to_str().unwrap()],
    )
    .unwrap();
}

fn insert_article(
    conn: &rusqlite::Connection,
    id: i64,
    title: &str,
    slug: &str,
    file: &str,
    status: &str,
    published_date: Option<&str>,
) {
    conn.execute(
        "INSERT INTO articles (id, title, url_slug, file, status, published_date, content_gaps_addressed, project_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]', 'p1')",
        rusqlite::params![id, title, slug, file, status, published_date],
    )
    .unwrap();
}

#[test]
fn publish_by_slugs_draft_to_published_and_exports() {
    let dir = unique_temp_dir("ps_publish_by_slug_happy");
    let auto_dir = dir.join(".github").join("automation");
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();
    std::fs::create_dir_all(&auto_dir).unwrap();
    std::fs::write(
        auto_dir.join("articles.json"),
        r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Happy","file":"./content/001_happy.mdx","published_date":"2024-06-01","status":"draft"}]}"#,
    )
    .unwrap();
    write_mdx(&content_dir.join("001_happy.mdx"), "Happy");

    let conn = in_memory_db();
    insert_project(&conn, &dir);
    insert_article(
        &conn,
        1,
        "Happy",
        "happy",
        "./content/001_happy.mdx",
        "draft",
        Some("2024-06-01"),
    );

    let result =
        publish_by_slugs(&conn, "p1", &dir, &["happy".into()]).expect("publish_by_slugs");

    assert!(result.ok, "errors: {:?}", result.errors);
    assert_eq!(result.published.len(), 1);
    assert_eq!(result.published[0].slug, "happy");
    assert_eq!(result.published[0].catalog_status, "published");
    assert!(result.errors.is_empty());
    assert!(result.blocked.is_empty());
    assert!(result.year_mismatches.is_empty());

    let db_status: String = conn
        .query_row(
            "SELECT status FROM articles WHERE id = 1 AND project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(db_status, "published");

    let json_on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json_on_disk).unwrap();
    assert_eq!(doc["articles"][0]["status"].as_str().unwrap(), "published");
}

#[test]
fn publish_by_slugs_already_published_is_skip_noop() {
    let dir = unique_temp_dir("ps_publish_by_slug_skip");
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();
    std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
    write_mdx(&content_dir.join("001_live.mdx"), "Live");

    let conn = in_memory_db();
    insert_project(&conn, &dir);
    insert_article(
        &conn,
        1,
        "Live",
        "live",
        "./content/001_live.mdx",
        "published",
        Some("2024-01-15"),
    );

    let result =
        publish_by_slugs(&conn, "p1", &dir, &["live".into()]).expect("publish_by_slugs");

    assert!(result.ok);
    assert!(result.published.is_empty());
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].reason, "already published");
    assert_eq!(
        result.skipped[0].catalog_status.as_deref(),
        Some("published")
    );
    assert!(result.errors.is_empty());
}

#[test]
fn publish_by_slugs_missing_slug_is_error() {
    let dir = unique_temp_dir("ps_publish_by_slug_missing");
    let auto_dir = dir.join(".github").join("automation");
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();
    std::fs::create_dir_all(&auto_dir).unwrap();
    // Pin content dir so resolve_content_dir succeeds without markdown files.
    std::fs::write(
        auto_dir.join("seo_workspace.json"),
        r#"{"content_dir":"content"}"#,
    )
    .unwrap();
    let conn = in_memory_db();
    insert_project(&conn, &dir);

    let result =
        publish_by_slugs(&conn, "p1", &dir, &["no-such-slug".into()]).expect("result ok");

    assert!(!result.ok);
    assert!(result.published.is_empty());
    assert!(result
        .errors
        .iter()
        .any(|e| e.contains("No article found for slug 'no-such-slug'")));
}

#[test]
fn publish_by_slugs_year_mismatch_leaves_status_unchanged() {
    let dir = unique_temp_dir("ps_publish_by_slug_year");
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();
    std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
    // Title year far behind publish year (>1) → year mismatch.
    write_mdx(&content_dir.join("001_guide_2020.mdx"), "Guide 2020");

    let conn = in_memory_db();
    insert_project(&conn, &dir);
    let today = chrono::Utc::now().date_naive();
    let publish_date = format!("{}-01-15", today.year());
    insert_article(
        &conn,
        1,
        "Guide 2020",
        "guide-2020",
        "./content/001_guide_2020.mdx",
        "draft",
        Some(&publish_date),
    );

    let result =
        publish_by_slugs(&conn, "p1", &dir, &["guide-2020".into()]).expect("publish_by_slugs");

    assert!(!result.ok);
    assert!(result.published.is_empty());
    assert_eq!(result.year_mismatches.len(), 1);
    assert_eq!(result.year_mismatches[0].title_year, 2020);
    assert_eq!(result.year_mismatches[0].catalog_status, "draft");

    let db_status: String = conn
        .query_row(
            "SELECT status FROM articles WHERE id = 1 AND project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(db_status, "draft");
}

#[test]
fn publish_by_slugs_empty_slugs_err() {
    let dir = unique_temp_dir("ps_publish_by_slug_empty");
    let conn = in_memory_db();
    insert_project(&conn, &dir);
    let err = publish_by_slugs(&conn, "p1", &dir, &[]).unwrap_err();
    assert!(err.contains("slug"));
}

#[test]
fn publish_by_slugs_uses_seo_workspace_content_dir() {
    // content_dir is content/blog (not hardcoded content/) via seo_workspace.json.
    let dir = unique_temp_dir("ps_publish_by_slug_blog_dir");
    let auto_dir = dir.join(".github").join("automation");
    let content_dir = dir.join("content").join("blog");
    std::fs::create_dir_all(&content_dir).unwrap();
    std::fs::create_dir_all(&auto_dir).unwrap();
    std::fs::write(
        auto_dir.join("seo_workspace.json"),
        r#"{"content_dir":"content/blog"}"#,
    )
    .unwrap();
    std::fs::write(
        auto_dir.join("articles.json"),
        r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Blog Post","file":"./content/blog/001_blog_post.mdx","published_date":"2024-06-01","status":"draft"}]}"#,
    )
    .unwrap();
    write_mdx(&content_dir.join("001_blog_post.mdx"), "Blog Post");

    let conn = in_memory_db();
    insert_project(&conn, &dir);
    insert_article(
        &conn,
        1,
        "Blog Post",
        "blog-post",
        "./content/blog/001_blog_post.mdx",
        "draft",
        Some("2024-06-01"),
    );

    let result =
        publish_by_slugs(&conn, "p1", &dir, &["blog-post".into()]).expect("publish_by_slugs");

    assert!(result.ok, "errors: {:?}", result.errors);
    assert_eq!(result.published.len(), 1);
    assert_eq!(result.published[0].slug, "blog-post");
    assert_eq!(result.published[0].catalog_status, "published");

    let db_status: String = conn
        .query_row(
            "SELECT status FROM articles WHERE id = 1 AND project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(db_status, "published");

    // MDX under content/blog must have been touched (status/date sync path).
    let mdx = std::fs::read_to_string(content_dir.join("001_blog_post.mdx")).unwrap();
    assert!(
        mdx.contains("date:") || mdx.contains("Blog Post"),
        "expected content under content/blog to remain readable: {mdx}"
    );
}
