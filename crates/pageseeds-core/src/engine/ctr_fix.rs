//! Shared slug → `fix_ctr_article` spawn path.
//!
//! CLI `create-task -t fix_ctr_article -S <slug>` routes here so every bare
//! CTR recovery create attaches a non-empty `ctr_context` artifact (single
//! article shape matching audit spawn). Never spawn `fix_ctr_article` without
//! that artifact.

use rusqlite::Connection;

use crate::content::slug::normalize_url_slug;
use crate::engine::exec::ctr_audit::{
    build_standalone_ctr_article_record, ctr_context_artifact_from_article,
};
use crate::engine::spawner::{DeduplicationPolicy, TaskSpec, TaskSpawner};
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::task::{AgentPolicy, Priority, Task, TaskRunPolicy};

/// Options for [`spawn_fix_ctr_article_for_slug`].
#[derive(Debug, Clone)]
pub struct SpawnFixCtrForSlugOpts {
    /// Override task title. Default: `CTR fix: {url_slug}`.
    pub title: Option<String>,
    pub priority: Priority,
    /// When true, set `run_policy = AutoEnqueue`.
    pub auto_enqueue: bool,
    /// Artifact `source` field (e.g. `"pageseeds-cli"`).
    pub source: String,
    /// Optional task description / operator reason.
    pub reason: Option<String>,
}

impl Default for SpawnFixCtrForSlugOpts {
    fn default() -> Self {
        Self {
            title: None,
            priority: Priority::Medium,
            auto_enqueue: false,
            source: String::new(),
            reason: None,
        }
    }
}

/// Resolve `slug` to a project article and spawn a `fix_ctr_article` task
/// with a full single-article `ctr_context` artifact.
///
/// Idempotency key is `fix_ctr_article:{project_id}:{article_id}`.
/// Dedup is `SkipIfActive` so re-runs after completion can create a new task.
pub fn spawn_fix_ctr_article_for_slug(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    slug: &str,
    opts: SpawnFixCtrForSlugOpts,
) -> Result<Task> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(Error::Validation(
            "fix_ctr_article requires a non-empty slug (article url_slug to fix)".to_string(),
        ));
    }

    let article = resolve_article_by_slug(conn, project_id, slug)?;

    // Bare article record → wrap once at the artifact boundary.
    let article_record =
        build_standalone_ctr_article_record(conn, project_id, project_path, &article)
            .map_err(Error::Validation)?;

    let source = if opts.source.is_empty() {
        "slug_recovery"
    } else {
        opts.source.as_str()
    };
    let artifact = ctr_context_artifact_from_article(article_record, source)
        .map_err(Error::Validation)?;

    let title = opts
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| format!("CTR fix: {}", article.url_slug));

    let description = opts
        .reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "Apply CTR fixes to article {} ({})",
                article.id, article.url_slug
            )
        });

    let spec = TaskSpec {
        project_id: project_id.to_string(),
        task_type: "fix_ctr_article".to_string(),
        title: Some(title),
        description: Some(description),
        priority: opts.priority,
        run_policy: if opts.auto_enqueue {
            Some(TaskRunPolicy::AutoEnqueue)
        } else {
            None
        },
        // Match audit spawn: agent is optional at the task level; analyze step
        // still runs the agent when context is present.
        agent_policy: AgentPolicy::Optional,
        artifacts: vec![artifact],
        idempotency_key: Some(format!(
            "fix_ctr_article:{}:{}",
            project_id, article.id
        )),
        dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
        ..Default::default()
    };

    TaskSpawner::spawn(conn, spec)
}

/// Match article by exact `url_slug` or normalized slug (same rule as content_fix).
fn resolve_article_by_slug(
    conn: &Connection,
    project_id: &str,
    slug: &str,
) -> Result<crate::models::article::Article> {
    let slug_norm = normalize_url_slug(slug);
    task_store::list_articles(conn, project_id)?
        .into_iter()
        .find(|a| a.url_slug == slug || normalize_url_slug(&a.url_slug) == slug_norm)
        .ok_or_else(|| Error::Validation(format!("No article found for slug '{slug}'")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::task::TaskStatus;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', ?2, 1, 'workspace')",
            rusqlite::params![id, path],
        )
        .unwrap();
    }

    fn insert_article(conn: &Connection, project_id: &str, id: i64, slug: &str, title: &str, file: &str) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status,
                content_gaps_addressed, target_volume, word_count, review_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', '[]', 0, 500, 0)",
            rusqlite::params![id, project_id, title, slug, file],
        )
        .unwrap();
    }

    fn setup_mdx_project() -> (String, Connection) {
        let n = std::sync::atomic::AtomicUsize::new(0);
        // unique temp dir per call
        let path = std::env::temp_dir()
            .join(format!(
                "ctr_fix_slug_test_{}_{}",
                std::process::id(),
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as usize
                        % 1_000_000)
            ))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        let content = std::path::Path::new(&path).join("content");
        let auto = std::path::Path::new(&path)
            .join(".github")
            .join("automation");
        std::fs::create_dir_all(&content).unwrap();
        std::fs::create_dir_all(&auto).unwrap();

        let mdx = r#"---
title: "My CTR Article Title That Is Quite Long For SERP"
description: "Short meta"
date: "2024-06-01"
---

# My CTR Article Title That Is Quite Long For SERP

Intro paragraph for the CTR article under recovery.
"#;
        std::fs::write(content.join("my_ctr_article.mdx"), mdx).unwrap();

        let articles = serde_json::json!({
            "articles": [{
                "id": 42,
                "url_slug": "my-ctr-article",
                "title": "My CTR Article Title That Is Quite Long For SERP",
                "target_keyword": "ctr article",
                "file": "content/my_ctr_article.mdx",
                "gsc": {
                    "impressions": 8000.0,
                    "clicks": 8.0,
                    "ctr": 0.001,
                    "avg_position": 9.0
                }
            }]
        });
        std::fs::write(
            auto.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let conn = in_memory_db();
        insert_project(&conn, "proj1", &path);
        insert_article(
            &conn,
            "proj1",
            42,
            "my-ctr-article",
            "My CTR Article Title That Is Quite Long For SERP",
            "content/my_ctr_article.mdx",
        );

        (path, conn)
    }

    #[test]
    fn spawn_attaches_ctr_context_with_total_articles_one() {
        let (path, conn) = setup_mdx_project();

        let task = spawn_fix_ctr_article_for_slug(
            &conn,
            "proj1",
            &path,
            "my-ctr-article",
            SpawnFixCtrForSlugOpts {
                source: "test".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(task.task_type, "fix_ctr_article");
        assert_eq!(task.title.as_deref(), Some("CTR fix: my-ctr-article"));

        let art = task
            .artifacts
            .iter()
            .find(|a| a.key == "ctr_context")
            .expect("ctr_context artifact");
        assert_eq!(art.source.as_deref(), Some("test"));
        assert_eq!(art.artifact_type.as_deref(), Some("json"));

        let doc: serde_json::Value =
            serde_json::from_str(art.content.as_deref().unwrap()).unwrap();
        assert_eq!(doc["total_articles"], 1);
        assert_eq!(doc["articles"].as_array().unwrap().len(), 1);

        let rec = &doc["articles"][0];
        assert_eq!(rec["id"], 42);
        assert_eq!(rec["url_slug"], "my-ctr-article");
        assert_eq!(rec["file"], "content/my_ctr_article.mdx");
        assert!(!rec["title"].as_str().unwrap_or("").is_empty());
        assert!(!rec["meta_description"].as_str().unwrap_or("").is_empty()
            || rec["meta_description"] == "Short meta");

        let reasons = rec["detection_reasons"].as_array().unwrap();
        assert!(
            reasons
                .iter()
                .any(|r| r.as_str() == Some("operator_requested")),
            "expected operator_requested in {reasons:?}"
        );

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_rejects_unknown_slug() {
        let (path, conn) = setup_mdx_project();
        let err = spawn_fix_ctr_article_for_slug(
            &conn,
            "proj1",
            &path,
            "missing-slug",
            SpawnFixCtrForSlugOpts::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("No article found"),
            "unexpected: {err}"
        );
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_rejects_empty_slug() {
        let (path, conn) = setup_mdx_project();
        let err = spawn_fix_ctr_article_for_slug(
            &conn,
            "proj1",
            &path,
            "  ",
            SpawnFixCtrForSlugOpts::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-empty slug"));
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_idempotency_returns_existing_active_task() {
        let (path, conn) = setup_mdx_project();
        let opts = SpawnFixCtrForSlugOpts {
            source: "test".to_string(),
            ..Default::default()
        };
        let first =
            spawn_fix_ctr_article_for_slug(&conn, "proj1", &path, "my-ctr-article", opts.clone())
                .unwrap();
        let second =
            spawn_fix_ctr_article_for_slug(&conn, "proj1", &path, "my-ctr-article", opts)
                .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.status, TaskStatus::Todo);

        let key: String = conn
            .query_row(
                "SELECT key FROM task_idempotency_keys WHERE task_id = ?1",
                rusqlite::params![first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(key, "fix_ctr_article:proj1:42");

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_matches_normalized_slug() {
        let (path, conn) = setup_mdx_project();
        let task = spawn_fix_ctr_article_for_slug(
            &conn,
            "proj1",
            &path,
            "my-ctr-article/",
            SpawnFixCtrForSlugOpts {
                source: "pageseeds-cli".to_string(),
                priority: Priority::High,
                auto_enqueue: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(task.task_type, "fix_ctr_article");
        assert_eq!(task.priority, Priority::High);
        assert_eq!(task.run_policy, TaskRunPolicy::AutoEnqueue);
        assert!(task.artifacts.iter().any(|a| a.key == "ctr_context"));
        let _ = std::fs::remove_dir_all(&path);
    }
}
