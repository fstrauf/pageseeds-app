    use super::*;
    use crate::models::ctr::CtrFixVerificationReport;
    use crate::models::task::{FollowUpPolicy, TaskReviewSurface};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn test_dir() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("ctr_audit_test_{}_{}", std::process::id(), n))
            .to_string_lossy()
            .to_string()
    }

    pub fn setup_project(path: &str) {
        let _ = std::fs::remove_dir_all(path);
        let auto_dir = std::path::Path::new(path)
            .join(".github")
            .join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = std::path::Path::new(path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "test-article",
                    "title": "Test Article | Brand | Brand -- Tagline",
                    "target_keyword": "test article",
                    "file": "content/001_test_article.mdx",
                    "gsc": { "impressions": 10000.0, "clicks": 10.0, "ctr": 0.001, "avg_position": 8.5 }
                },
                {
                    "id": 2,
                    "url_slug": "another-article",
                    "title": "Another Article",
                    "target_keyword": "another article",
                    "file": "content/002_another_article.mdx",
                    "gsc": { "impressions": 5000.0, "clicks": 5.0, "ctr": 0.001, "avg_position": 12.0 }
                }
            ]
        });
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let mdx1 = r#"---
title: "Test Article | Brand | Brand -- Tagline"
description: "A short desc"
date: "2024-01-01"
---

# Test Article | Brand | Brand -- Tagline

This is the first paragraph of the test article. It contains some content.

## Section 1

More content here.
"#;
        std::fs::write(content_dir.join("001_test_article.mdx"), mdx1).unwrap();

        let mdx2 = r#"---
title: "Another Article"
description: ""
date: "2024-01-02"
---

# Another Article

This is another article with different content.
"#;
        std::fs::write(content_dir.join("002_another_article.mdx"), mdx2).unwrap();
    }

    pub fn cleanup(path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    pub fn test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    pub fn insert_test_project(conn: &rusqlite::Connection, path: &str) {
        let project = crate::models::project::Project {
            id: "proj-test".to_string(),
            name: "Test Project".to_string(),
            path: path.to_string(),
            content_dir: None,
            site_url: None,
            site_id: None,
            sitemap_url: None,
            project_mode: crate::models::project::ProjectMode::Workspace,
            active: true,
            agent_provider: None,
            seo_provider: Some("ahrefs".to_string()),
            clarity_project_id: None,
        };
        crate::engine::task_store::create_project(conn, &project).unwrap();
    }

    pub fn ctr_parent_task(id: &str, content: serde_json::Value) -> crate::models::task::Task {
        crate::models::task::Task {
            id: id.to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Parent Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_build_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(content.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    pub fn ctr_context_for_article(content_hash: &str) -> serde_json::Value {
        serde_json::json!({
            "total_articles": 1,
            "articles": [
                {
                    "id": 1,
                    "url_slug": "test-article",
                    "file": "content/001_test_article.mdx",
                    "content_hash": content_hash,
                    "target_keyword": "test article",
                    "issues_detected": {
                        "file_not_found": false,
                        "title_too_long": true,
                        "meta_too_short": false,
                        "snippet_suboptimal": false,
                        "missing_faq_schema": false
                    }
                }
            ]
        })
    }

mod context;
mod analyze;
mod apply;
mod verify;
mod task_spawner;
mod patch;
