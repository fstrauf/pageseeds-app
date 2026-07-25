use super::*;
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::TaskArtifact;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempProjectDir {
    path: PathBuf,
}

impl TempProjectDir {
    fn new() -> Self {
        let n = TMP_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "pageseeds-merge-pkg-{}-{}",
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("content").join("blog")).unwrap();
        fs::create_dir_all(path.join(".github").join("automation")).unwrap();
        fs::write(
            path.join(".github")
                .join("automation")
                .join("seo_workspace.json"),
            r#"{"content_dir":"content/blog"}"#,
        )
        .unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempProjectDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn in_memory_db(project_path: &str) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            content_dir TEXT,
            site_url TEXT,
            site_id TEXT,
            sitemap_url TEXT,
            project_mode TEXT NOT NULL DEFAULT 'workspace',
            active INTEGER DEFAULT 1,
            agent_provider TEXT,
            seo_provider TEXT,
            clarity_project_id TEXT
        );
        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            phase TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            priority TEXT NOT NULL DEFAULT 'medium',
            run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
            review_surface TEXT NOT NULL DEFAULT 'none',
            follow_up_policy TEXT NOT NULL DEFAULT 'none',
            agent_policy TEXT NOT NULL DEFAULT 'none',
            title TEXT,
            description TEXT,
            project_id TEXT NOT NULL,
            depends_on TEXT NOT NULL DEFAULT '[]',
            artifacts TEXT NOT NULL DEFAULT '[]',
            run_attempts INTEGER DEFAULT 0,
            run_last_error TEXT,
            run_provider TEXT,
            not_before TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE task_idempotency_keys (
            key TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT
        );
        CREATE TABLE task_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            provider TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            success INTEGER,
            error TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER
        );
        CREATE TABLE articles (
            id INTEGER NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            url_slug TEXT NOT NULL DEFAULT '',
            file TEXT NOT NULL DEFAULT '',
            target_keyword TEXT,
            keyword_difficulty TEXT,
            target_volume INTEGER DEFAULT 0,
            published_date TEXT,
            word_count INTEGER DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'draft',
            review_status TEXT,
            review_started_at TEXT,
            last_reviewed_at TEXT,
            review_count INTEGER NOT NULL DEFAULT 0,
            content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
            estimated_traffic_monthly TEXT,
            page_type TEXT,
            content_hash TEXT,
            last_edited_at TEXT,
            project_id TEXT NOT NULL,
            PRIMARY KEY (id, project_id)
        );
        CREATE TABLE articles_meta (
            project_id TEXT PRIMARY KEY,
            next_article_id INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE article_metadata (
            project_id TEXT NOT NULL,
            article_id INTEGER NOT NULL,
            namespace TEXT NOT NULL,
            payload TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL,
            PRIMARY KEY (project_id, article_id, namespace)
        );
        CREATE TABLE gsc_page_daily (
            project_id TEXT NOT NULL,
            page TEXT NOT NULL,
            date TEXT NOT NULL,
            clicks REAL NOT NULL DEFAULT 0,
            impressions REAL NOT NULL DEFAULT 0,
            position REAL NOT NULL DEFAULT 0,
            PRIMARY KEY (project_id, page, date)
        );
        CREATE TABLE ctr_query_metrics (
            project_id TEXT NOT NULL,
            article_id INTEGER NOT NULL,
            page_url TEXT,
            query TEXT NOT NULL,
            impressions REAL NOT NULL DEFAULT 0,
            clicks REAL NOT NULL DEFAULT 0,
            ctr REAL NOT NULL DEFAULT 0,
            avg_position REAL NOT NULL DEFAULT 0,
            period_start TEXT,
            period_end TEXT,
            intent TEXT,
            fetched_at TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (id, name, path, content_dir, active)
         VALUES ('proj1', 'Test', ?1, 'content/blog', 1)",
        [project_path],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO articles_meta (project_id, next_article_id) VALUES ('proj1', 10)",
        [],
    )
    .unwrap();
    conn
}

/// Write MDX so `find_file_by_slug` resolves via stem normalize + url_slug.
fn write_mdx(dir: &Path, slug: &str, title: &str, body: &str) -> PathBuf {
    // Filename stem normalizes to the same slug (underscores → dashes).
    let stem = slug.replace('-', "_");
    let content = format!(
        "---\ntitle: {title}\ndescription: Test article about {title}\nurl_slug: {slug}\nslug: {slug}\ndate: \"2024-06-01\"\nstatus: published\n---\n\n# {title}\n\n{body}\n"
    );
    let path = dir
        .join("content")
        .join("blog")
        .join(format!("{stem}.mdx"));
    fs::write(&path, content).unwrap();
    path
}

fn pad_body(min_words: usize) -> String {
    format!(
        "## Overview\n\n{}\n\n## Details\n\nMore content for the article body.\n",
        "word ".repeat(min_words)
    )
}

fn insert_article(
    conn: &Connection,
    id: i64,
    slug: &str,
    title: &str,
    file: &str,
) {
    conn.execute(
        "INSERT INTO articles (id, title, url_slug, file, status, word_count, project_id)
         VALUES (?1, ?2, ?3, ?4, 'published', 500, 'proj1')",
        rusqlite::params![id, title, slug, file],
    )
    .unwrap();
}

#[test]
fn build_from_urls_includes_full_bodies_and_skill() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "hub-page", "Hub Page", &pad_body(100));
    write_mdx(tmp.path(), "old-page", "Old Page", &pad_body(80));
    insert_article(&conn, 1, "hub-page", "Hub Page", "content/blog/hub_page.mdx");
    insert_article(&conn, 2, "old-page", "Old Page", "content/blog/old_page.mdx");

    let pkg = build_merge_package(
        &conn,
        "proj1",
        tmp.path(),
        MergeContextSource::Urls {
            keep_url: "/blog/hub-page".into(),
            redirect_urls: vec!["/blog/old-page".into()],
        },
    )
    .expect("package builds without LLM");

    assert_eq!(pkg.project_id, "proj1");
    assert_eq!(pkg.plan.keep_url, "/blog/hub-page");
    assert_eq!(pkg.plan.redirect_urls, vec!["/blog/old-page".to_string()]);
    assert_eq!(pkg.keep.slug, "hub-page");
    assert!(pkg.keep.content.contains("title: Hub Page"));
    assert_eq!(pkg.redirects.len(), 1);
    assert!(pkg.redirects[0].content.contains("title: Old Page"));
    assert!(!pkg.keep.outline.is_empty());
    assert_eq!(pkg.skill_name, "merge-content");
    assert!(
        pkg.skill_content
            .as_ref()
            .map(|c| !c.is_empty())
            .unwrap_or(false),
        "skill_content should load merge-content body"
    );
    assert!(pkg.keeper_file.ends_with(".mdx"));
    assert!(pkg.instructions.contains("merge-submit"));
    assert!(!pkg.requires_human_confirm);
    assert_eq!(pkg.constraints.min_keeper_words, MIN_KEEPER_WORDS);
}

#[test]
fn build_from_article_ids() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "keep-slug", "Keep", &pad_body(50));
    write_mdx(tmp.path(), "src-slug", "Src", &pad_body(40));
    insert_article(&conn, 10, "keep-slug", "Keep", "content/blog/keep_slug.mdx");
    insert_article(&conn, 20, "src-slug", "Src", "content/blog/src_slug.mdx");

    let pkg = build_merge_package(
        &conn,
        "proj1",
        tmp.path(),
        MergeContextSource::ArticleIds {
            keep_id: 10,
            redirect_ids: vec![20],
        },
    )
    .unwrap();

    assert_eq!(pkg.keep.article_id, Some(10));
    assert_eq!(pkg.redirects[0].article_id, Some(20));
    assert_eq!(pkg.plan.keep_url, "/blog/keep-slug");
}

#[test]
fn build_fails_on_missing_redirect_file() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "keep-slug", "Keep", &pad_body(50));

    let err = build_merge_package(
        &conn,
        "proj1",
        tmp.path(),
        MergeContextSource::Urls {
            keep_url: "/blog/keep-slug".into(),
            redirect_urls: vec!["/blog/missing-page".into()],
        },
    )
    .unwrap_err();
    assert!(
        err.contains("not found") || err.contains("missing"),
        "err={err}"
    );
}

#[test]
fn build_fails_on_cycle() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "keep-slug", "Keep", &pad_body(50));

    let err = build_merge_package(
        &conn,
        "proj1",
        tmp.path(),
        MergeContextSource::Urls {
            keep_url: "/blog/keep-slug".into(),
            redirect_urls: vec!["/blog/keep-slug".into()],
        },
    )
    .unwrap_err();
    assert!(err.contains("cycle"), "err={err}");
}

#[test]
fn build_from_consolidate_task() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "hub-coffee", "Hub", &pad_body(60));
    write_mdx(tmp.path(), "old-post", "Old", &pad_body(40));

    let task = TaskSpawner::spawn(
        &conn,
        TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "consolidate_cluster".to_string(),
            title: Some("Merge cluster: cluster-1".into()),
            artifacts: vec![TaskArtifact {
                key: "cannibalization_strategy".into(),
                path: None,
                artifact_type: Some("json".into()),
                source: Some("cannibalization_audit".into()),
                content: Some(
                    serde_json::json!({
                        "merge_recommendations": [{
                            "cluster_id": "cluster-1",
                            "keep_url": "/blog/hub-coffee",
                            "redirect_urls": ["/blog/old-post"],
                            "reason": "exact keyword overlap"
                        }]
                    })
                    .to_string(),
                ),
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let pkg = build_merge_package(
        &conn,
        "proj1",
        tmp.path(),
        MergeContextSource::ConsolidateTask {
            task_id: task.id.clone(),
        },
    )
    .unwrap();

    assert_eq!(pkg.consolidate_task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(pkg.plan.cluster_id.as_deref(), Some("cluster-1"));
    assert_eq!(pkg.plan.keep_url, "/blog/hub-coffee");
    assert_eq!(pkg.plan.reason.as_deref(), Some("exact keyword overlap"));
}

#[test]
fn submit_fails_validation_on_broken_mdx() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    // Unclosed frontmatter fence → invalid MDX; stem still resolves hub-page.
    let keep = tmp.path().join("content/blog/hub_page.mdx");
    fs::write(
        &keep,
        "---\ntitle: Hub\nurl_slug: hub-page\n\n# Broken\n\nword ".repeat(50),
    )
    .unwrap();
    write_mdx(tmp.path(), "old-page", "Old", &pad_body(40));

    let result = submit_merge(
        &conn,
        "proj1",
        tmp.path(),
        MergeSubmitOpts {
            keep_url: Some("/blog/hub-page".into()),
            redirect_urls: Some(vec!["/blog/old-page".into()]),
            ..Default::default()
        },
    )
    .expect("structured failure not domain Err");

    assert!(!result.ok);
    assert!(!result.redirects_written);
    assert_eq!(result.sources_depublished, 0);
    let mdx = result.checks.iter().find(|c| c.name == "valid_mdx").unwrap();
    assert!(!mdx.ok);
    // redirects.csv must not exist
    assert!(!tmp
        .path()
        .join(".github/automation/redirects.csv")
        .exists());
}

#[test]
fn submit_fails_on_thin_body() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "hub-page", "Hub", "Too short.\n");
    write_mdx(tmp.path(), "old-page", "Old", &pad_body(40));

    let result = submit_merge(
        &conn,
        "proj1",
        tmp.path(),
        MergeSubmitOpts {
            keep_url: Some("/blog/hub-page".into()),
            redirect_urls: Some(vec!["/blog/old-page".into()]),
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!result.ok);
    assert!(!result.redirects_written);
    let words = result
        .checks
        .iter()
        .find(|c| c.name == "min_keeper_words")
        .unwrap();
    assert!(!words.ok);
}

#[test]
fn submit_success_writes_redirects_depublishes_rewrites() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());

    write_mdx(tmp.path(), "hub-page", "Hub Page", &pad_body(450));
    write_mdx(tmp.path(), "old-page", "Old Page", &pad_body(80));
    // Inbound link from a third page
    write_mdx(
        tmp.path(),
        "other-page",
        "Other",
        &format!(
            "See [old](/blog/old-page) for more.\n\n{}",
            pad_body(50)
        ),
    );
    insert_article(&conn, 1, "hub-page", "Hub Page", "content/blog/hub_page.mdx");
    insert_article(&conn, 2, "old-page", "Old Page", "content/blog/old_page.mdx");
    insert_article(
        &conn,
        3,
        "other-page",
        "Other",
        "content/blog/other_page.mdx",
    );

    let task = TaskSpawner::spawn(
        &conn,
        TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "consolidate_cluster".to_string(),
            title: Some("Merge cluster: c1".into()),
            artifacts: vec![TaskArtifact {
                key: "cannibalization_strategy".into(),
                path: None,
                artifact_type: Some("json".into()),
                source: None,
                content: Some(
                    serde_json::json!({
                        "merge_recommendations": [{
                            "cluster_id": "c1",
                            "keep_url": "/blog/hub-page",
                            "redirect_urls": ["/blog/old-page"]
                        }]
                    })
                    .to_string(),
                ),
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let result = submit_merge(
        &conn,
        "proj1",
        tmp.path(),
        MergeSubmitOpts {
            consolidate_task_id: Some(task.id.clone()),
            keep_url: Some("/blog/hub-page".into()),
            redirect_urls: Some(vec!["/blog/old-page".into()]),
            confirmed: false,
        },
    )
    .expect("submit succeeds");

    assert!(result.ok, "checks={:?}", result.checks);
    assert!(result.redirects_written);
    assert!(result.inbound_links_rewritten >= 1);
    assert_eq!(result.sources_depublished, 1);
    assert_eq!(
        result.consolidate_task_id.as_deref(),
        Some(task.id.as_str())
    );
    assert_eq!(result.consolidate_task_status.as_deref(), Some("done"));

    let csv = fs::read_to_string(tmp.path().join(".github/automation/redirects.csv")).unwrap();
    assert!(csv.contains("/blog/old-page"));
    assert!(csv.contains("/blog/hub-page"));
    assert!(csv.contains("301"));

    let old = fs::read_to_string(tmp.path().join("content/blog/old_page.mdx")).unwrap();
    assert!(old.contains("status: redirected") || old.contains("status: \"redirected\""));

    let other = fs::read_to_string(tmp.path().join("content/blog/other_page.mdx")).unwrap();
    assert!(other.contains("/blog/hub-page"));
    assert!(!other.contains("/blog/old-page"));

    let status: String = conn
        .query_row(
            "SELECT status FROM articles WHERE id = 2 AND project_id = 'proj1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "redirected");

    let done = crate::engine::task_store::get_task(&conn, &task.id).unwrap();
    assert_eq!(done.status, TaskStatus::Done);
}

#[test]
fn high_traffic_requires_confirm() {
    let tmp = TempProjectDir::new();
    let conn = in_memory_db(tmp.path().to_str().unwrap());
    write_mdx(tmp.path(), "hub-page", "Hub", &pad_body(450));
    write_mdx(tmp.path(), "old-page", "Old", &pad_body(40));
    insert_article(&conn, 1, "hub-page", "Hub", "content/blog/hub_page.mdx");
    insert_article(&conn, 2, "old-page", "Old", "content/blog/old_page.mdx");

    // Seed GSC above confirm threshold
    let (start, end) = gsc_window_dates(GSC_WINDOW_DAYS);
    conn.execute(
        "INSERT INTO gsc_page_daily (project_id, page, date, clicks, impressions, position)
         VALUES ('proj1', 'https://example.com/blog/hub-page', ?1, 80, 2000, 5.0)",
        [&start],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO gsc_page_daily (project_id, page, date, clicks, impressions, position)
         VALUES ('proj1', 'https://example.com/blog/hub-page', ?1, 20, 500, 5.0)",
        [&end],
    )
    .unwrap();

    let pkg = build_merge_package(
        &conn,
        "proj1",
        tmp.path(),
        MergeContextSource::Urls {
            keep_url: "/blog/hub-page".into(),
            redirect_urls: vec!["/blog/old-page".into()],
        },
    )
    .unwrap();
    assert!(
        pkg.requires_human_confirm,
        "clicks/impressions should flag confirm"
    );
    assert!(pkg.keep.clicks >= HUMAN_CONFIRM_CLICKS || pkg.keep.impressions >= HUMAN_CONFIRM_IMPRESSIONS);

    // Submit without confirm → fail closed
    let result = submit_merge(
        &conn,
        "proj1",
        tmp.path(),
        MergeSubmitOpts {
            keep_url: Some("/blog/hub-page".into()),
            redirect_urls: Some(vec!["/blog/old-page".into()]),
            confirmed: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!result.ok);
    assert!(!result.redirects_written);
    let confirm = result
        .checks
        .iter()
        .find(|c| c.name == "human_confirm")
        .unwrap();
    assert!(!confirm.ok);

    // With confirm → success
    let result = submit_merge(
        &conn,
        "proj1",
        tmp.path(),
        MergeSubmitOpts {
            keep_url: Some("/blog/hub-page".into()),
            redirect_urls: Some(vec!["/blog/old-page".into()]),
            confirmed: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.ok, "checks={:?}", result.checks);
    assert!(result.redirects_written);
}
