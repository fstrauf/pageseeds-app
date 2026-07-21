mod cluster_link;
pub mod hub_page;
mod indexing_link;
/// Content review and sync execution module.
///
/// Covers:
///   - exec_content_sync           (sync articles.json ↔ MDX files)
///   - exec_content_review_recommend (select priority articles + run agent)
///   - exec_cluster_link_scan      (native Rust internal-link scan for cluster_and_link step 1)
///   - exec_cluster_link_strategy  (agentic: interpret scan, recommend links to add, write links_to_add.json)
///   - exec_cluster_link_apply     (deterministic: write "Related Articles" sections to MDX files)
///   - select_priority_articles    (scoring formula)
///   - build_review_context        (structured context for LLM)
///   - build_review_prompt         (prompt assembly)
///   - create_fix_content_article_tasks (auto-spawn follow-up task after content_review)
///   - create_cluster_and_link_task    (auto-spawn follow-up task after write_article)
///   - exec_fix_content_article_context   (deterministic: load recs + file for per-article fix)
///   - exec_fix_content_article_generate  (agentic: structured extraction of ContentFixPatch)
///   - exec_fix_content_article_apply     (deterministic: apply patch to MDX)
///   - exec_fix_content_article_verify    (deterministic: verify fixes meet thresholds)
///   - exec_link_integrity_verify         (deterministic: verify/repair /blog/ links after agentic writes)
///   - exec_content_write_verify          (deterministic: fail the task when a new-article write left no registered file)
mod fix_apply;
mod fix_context;
mod fix_generate;
mod fix_verify;
mod link_verify;
mod quality_review;
mod review;
mod sync;
mod task_spawner;
mod write_verify;

pub(crate) use cluster_link::*;
pub(crate) use fix_apply::*;
pub(crate) use fix_context::*;
pub(crate) use fix_generate::*;
pub(crate) use fix_verify::*;
pub(crate) use indexing_link::*;
pub(crate) use link_verify::*;
pub(crate) use quality_review::*;
pub(crate) use review::*;
#[cfg(test)]
use rusqlite::Connection;
pub(crate) use sync::*;
pub(crate) use task_spawner::*;
pub(crate) use write_verify::*;

#[cfg(test)]
use crate::engine::project_paths::ProjectPaths;
#[cfg(test)]
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::task_store;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TempProjectDir {
        path: PathBuf,
    }

    impl TempProjectDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("pageseeds-content-review-{}", Uuid::new_v4()));
            fs::create_dir_all(path.join(".github").join("automation")).unwrap();
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

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                active INTEGER DEFAULT 1
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
            );",
        )
        .unwrap();
        conn
    }

    fn create_test_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![id, "Test Project", path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES (?1, 200)",
            [id],
        )
        .unwrap();
    }

    fn insert_test_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        status: &str,
        review_status: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                review_status, review_started_at, last_reviewed_at, review_count,
                content_gaps_addressed, estimated_traffic_monthly, project_id
             ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, NULL, 0, ?5, ?6, NULL, NULL, 0, '[]', NULL, ?7)",
            rusqlite::params![
                id,
                format!("Article {id}"),
                format!("article-{id}"),
                format!("./content/{id}_article.mdx"),
                status,
                review_status,
                project_id,
            ],
        )
        .unwrap();
    }

    fn make_parent_task(project_id: &str) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: format!("task-{}", Uuid::new_v4()),
            project_id: project_id.to_string(),
            task_type: "content_review".to_string(),
            phase: "investigation".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Required,
            title: Some("Content Review".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
            not_before: None,
        }
    }

    fn write_recommendations(project_dir: &Path, recommendations: serde_json::Value) {
        let path = project_dir
            .join(".github")
            .join("automation")
            .join("recommendations.json");
        fs::write(
            path,
            serde_json::to_string_pretty(&recommendations).unwrap(),
        )
        .unwrap();
    }

    fn idempotency_keys(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT key FROM task_idempotency_keys ORDER BY key")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    }

    #[test]
    fn recommendation_article_id_accepts_strings_and_numbers() {
        assert_eq!(
            recommendation_article_id(&json!({ "article_id": "109" })),
            Some(109)
        );
        assert_eq!(
            recommendation_article_id(&json!({ "article_id": 111 })),
            Some(111)
        );
        assert_eq!(
            recommendation_article_id(&json!({ "article_id": "   " })),
            None
        );
        assert_eq!(recommendation_article_id(&json!({})), None);
    }

    fn reviewed_at(days_ago: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::days(days_ago)).to_rfc3339()
    }

    #[test]
    fn select_priority_articles_prioritizes_unreviewed_backlog_before_reviewed_revisits() {
        let raw_articles = vec![
            json!({
                "id": 1,
                "title": "Reviewed winner",
                "file": "./content/1_reviewed.mdx",
                "url_slug": "reviewed-winner",
                "status": "published",
                "review_status": "reviewed",
                "last_reviewed_at": reviewed_at(60),
                "gsc": { "avg_position": 8.0, "impressions": 800.0, "ctr": 0.0 }
            }),
            json!({
                "id": 2,
                "title": "Unreviewed backlog",
                "file": "./content/2_unreviewed.mdx",
                "url_slug": "unreviewed-backlog",
                "status": "published",
                "gsc": { "avg_position": 2.0, "impressions": 10.0, "ctr": 0.2 }
            }),
        ];

        let audit_articles = vec![
            json!({
                "file": "./content/1_reviewed.mdx",
                "health": "poor",
                "checks_failed": 6,
                "health_score": 40,
                "checks": {}
            }),
            json!({
                "file": "./content/2_unreviewed.mdx",
                "health": "good",
                "checks_failed": 0,
                "health_score": 100,
                "checks": {}
            }),
        ];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0]["id"], 2);
        assert_eq!(selected[0]["_review_bucket"], "backlog");
        assert_eq!(selected[1]["id"], 1);
        assert_eq!(selected[1]["_review_reason"], "stale");
    }

    #[test]
    fn select_priority_articles_backfills_with_stale_reviewed_articles() {
        let raw_articles = vec![json!({
            "id": 1,
            "title": "Stale reviewed article",
            "file": "./content/1_reviewed.mdx",
            "url_slug": "stale-reviewed",
            "status": "published",
            "review_status": "reviewed",
            "last_reviewed_at": reviewed_at(90),
            "gsc": { "avg_position": 2.0, "impressions": 50.0, "ctr": 0.10 }
        })];

        let audit_articles = vec![json!({
            "file": "./content/1_reviewed.mdx",
            "health": "good",
            "checks_failed": 0,
            "health_score": 100,
            "checks": {}
        })];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 5);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["id"], 1);
        assert_eq!(selected[0]["_review_reason"], "stale");
    }

    #[test]
    fn select_priority_articles_allows_regressed_reviewed_articles_after_cooldown() {
        let raw_articles = vec![json!({
            "id": 1,
            "title": "Regressed reviewed article",
            "file": "./content/1_reviewed.mdx",
            "url_slug": "regressed-reviewed",
            "status": "published",
            "review_status": "reviewed",
            "last_reviewed_at": reviewed_at(20),
            "gsc": { "avg_position": 8.0, "impressions": 900.0, "ctr": 0.01 }
        })];

        let audit_articles = vec![json!({
            "file": "./content/1_reviewed.mdx",
            "health": "good",
            "checks_failed": 0,
            "health_score": 100,
            "checks": {}
        })];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 5);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["id"], 1);
        assert_eq!(selected[0]["_review_reason"], "regressed");
    }

    #[test]
    fn select_priority_articles_keeps_recent_reviewed_regressions_on_cooldown() {
        let raw_articles = vec![json!({
            "id": 1,
            "title": "Recently reviewed article",
            "file": "./content/1_reviewed.mdx",
            "url_slug": "recent-reviewed",
            "status": "published",
            "review_status": "reviewed",
            "last_reviewed_at": reviewed_at(5),
            "gsc": { "avg_position": 9.0, "impressions": 1200.0, "ctr": 0.01 }
        })];

        let audit_articles = vec![json!({
            "file": "./content/1_reviewed.mdx",
            "health": "poor",
            "checks_failed": 5,
            "health_score": 45,
            "checks": {}
        })];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 5);
        assert!(selected.is_empty());
    }

    #[test]
    fn create_fix_content_article_tasks_uses_numeric_article_ids_in_idempotency_keys() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", None);
        insert_test_article(&conn, "proj1", 111, "published", None);

        write_recommendations(
            project_dir.path(),
            json!({
                "articles": [
                    {
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": [{ "category": "title" }]
                    },
                    {
                        "article_id": 111,
                        "article_title": "Beta",
                        "article_file": "./content/111_beta.mdx",
                        "suggestions": [{ "category": "meta_description" }, { "category": "cta" }]
                    }
                ]
            }),
        );

        let parent = make_parent_task("proj1");
        let created = create_fix_content_article_tasks(&conn, &parent, &project_path);

        assert_eq!(created.len(), 2);
        assert_eq!(
            idempotency_keys(&conn),
            vec![
                "fix_content_article:proj1:109".to_string(),
                "fix_content_article:proj1:111".to_string(),
            ]
        );

        let tasks = task_store::list_tasks(&conn, "proj1").unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(tasks
            .iter()
            .all(|task| task.task_type == "fix_content_article"));
        assert!(tasks.iter().all(|task| {
            task.artifacts.iter().any(|artifact| {
                artifact.key == "recommendations_109" || artifact.key == "recommendations_111"
            })
        }));

        let articles = task_store::list_articles(&conn, "proj1").unwrap();
        assert!(articles
            .iter()
            .all(|article| article.review_status.as_deref() == Some("in_review")));

        let exported: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                project_dir
                    .path()
                    .join(".github")
                    .join("automation")
                    .join("articles.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let exported_articles = exported["articles"].as_array().unwrap();
        assert!(exported_articles
            .iter()
            .all(|article| article["review_status"] == "in_review"));
    }

    #[test]
    fn create_fix_content_article_tasks_gates_fixes_behind_user_enqueue() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", None);

        write_recommendations(
            project_dir.path(),
            json!({
                "articles": [
                    {
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": [{ "category": "title" }]
                    }
                ]
            }),
        );

        let parent = make_parent_task("proj1");
        let created = create_fix_content_article_tasks(&conn, &parent, &project_path);
        assert_eq!(created.len(), 1);

        // A completed content_review must leave fix tasks pending explicit
        // user action: UserEnqueue policy, Todo status — never auto-run.
        let tasks = task_store::list_tasks(&conn, "proj1").unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks
            .iter()
            .all(|task| task.run_policy == TaskRunPolicy::UserEnqueue));
        assert!(tasks.iter().all(|task| task.status == TaskStatus::Todo));
    }

    #[test]
    fn create_fix_content_article_tasks_skips_invalid_and_duplicate_article_ids() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", None);

        write_recommendations(
            project_dir.path(),
            json!({
                "articles": [
                    {
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": [{ "category": "title" }]
                    },
                    {
                        "article_id": 109,
                        "article_title": "Alpha Duplicate",
                        "article_file": "./content/109_alpha_dup.mdx",
                        "suggestions": [{ "category": "cta" }]
                    },
                    {
                        "article_title": "Missing ID",
                        "article_file": "./content/missing_id.mdx",
                        "suggestions": [{ "category": "faq" }]
                    }
                ]
            }),
        );

        let parent = make_parent_task("proj1");
        let created = create_fix_content_article_tasks(&conn, &parent, &project_path);

        assert_eq!(created.len(), 1);
        assert_eq!(
            idempotency_keys(&conn),
            vec!["fix_content_article:proj1:109".to_string()]
        );
    }

    #[test]
    fn mark_fix_content_article_reviewed_updates_article_state_and_export() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", Some("in_review"));

        let task = Task {
            id: format!("task-{}", Uuid::new_v4()),
            project_id: "proj1".to_string(),
            task_type: "fix_content_article".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Required,
            title: Some("Fix: Alpha".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "recommendations_109".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("content_review".to_string()),
                content: Some(
                    serde_json::to_string(&json!({
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": []
                    }))
                    .unwrap(),
                ),
            }],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let article_id = mark_fix_content_article_reviewed(&conn, &task, &project_path).unwrap();
        assert_eq!(article_id, Some(109));

        let articles = task_store::list_articles(&conn, "proj1").unwrap();
        let article = articles.iter().find(|article| article.id == 109).unwrap();
        assert_eq!(article.review_status.as_deref(), Some("reviewed"));
        assert_eq!(article.review_count, 1);
        assert!(article.last_reviewed_at.is_some());
        assert!(article.review_started_at.is_none());

        let exported: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                project_dir
                    .path()
                    .join(".github")
                    .join("automation")
                    .join("articles.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let exported_article = exported["articles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|article| article["id"] == 109)
            .unwrap();
        assert_eq!(exported_article["review_status"], "reviewed");
        assert_eq!(exported_article["review_count"], 1);
        assert!(exported_article.get("last_reviewed_at").is_some());
    }
}
