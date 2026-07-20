//! Integration tests for Clarity follow-up task creation from selected findings.
//!
//! Covers the task-drawer "Create Tasks" flow:
//! (a) every finding issue type maps to `fix_content_article` with an article_id
//!     artifact (never `create_landing_page` / `write_article`),
//! (b) duplicate submissions are suppressed by the idempotency key,
//! (c) findings whose URL does not resolve to a project article are skipped
//!     with an explanation.

use std::time::{SystemTime, UNIX_EPOCH};

use pageseeds_lib::clarity::follow_up::spawn_tasks_from_selection;
use pageseeds_lib::db;
use pageseeds_lib::engine::task_store;
use pageseeds_lib::models::clarity::ClarityFindingPayload;
use pageseeds_lib::models::project::{Project, ProjectMode};
use pageseeds_lib::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
    TaskStatus,
};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
}

fn create_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("Failed to open in-memory DB");
    db::init_with_conn(&conn).expect("Failed to init DB");
    conn
}

fn create_test_project(conn: &rusqlite::Connection, path: &str) -> String {
    let project = Project {
        id: "test-proj-clarity".to_string(),
        name: "Test Project".to_string(),
        path: path.to_string(),
        content_dir: Some("content".to_string()),
        site_url: Some("https://example.com".to_string()),
        site_id: None,
        sitemap_url: None,
        project_mode: ProjectMode::Workspace,
        active: true,
        agent_provider: Some("copilot".to_string()),
        seo_provider: Some("ahrefs".to_string()),
        clarity_project_id: None,
    };
    task_store::create_project(conn, &project).expect("Failed to create project");
    project.id
}

fn insert_article(conn: &rusqlite::Connection, project_id: &str, id: i64, slug: &str) {
    conn.execute(
        "INSERT INTO articles (
            id, project_id, title, url_slug, file, status,
            content_gaps_addressed, target_volume, word_count, review_count
         ) VALUES (?1, ?2, ?3, ?4, 'article.mdx', 'published', '[]', 0, 0, 0)",
        rusqlite::params![id, project_id, format!("Article {}", id), slug],
    )
    .expect("Failed to insert article");
}

fn create_parent_task(conn: &rusqlite::Connection, project_id: &str, task_type: &str) -> Task {
    let now = chrono::Utc::now().to_rfc3339();
    let task = Task {
        id: format!("parent-{}", uuid::Uuid::new_v4()),
        task_type: task_type.to_string(),
        phase: "investigation".to_string(),
        status: TaskStatus::Review,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some("Clarity investigation".to_string()),
        description: None,
        project_id: project_id.to_string(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        not_before: None,
        created_at: now.clone(),
        updated_at: now,
    };
    task_store::create_task(conn, &task).expect("Failed to create parent task");
    task
}

fn finding(issue_type: &str, url: &str) -> ClarityFindingPayload {
    ClarityFindingPayload {
        issue_type: issue_type.to_string(),
        severity: "high".to_string(),
        url: url.to_string(),
        evidence: format!("evidence for {}", issue_type),
        recommendation: format!("recommendation for {}", issue_type),
        clarity_dashboard_url: "https://clarity.microsoft.com/dashboard".to_string(),
    }
}

fn fixture() -> (rusqlite::Connection, String, std::path::PathBuf) {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("clarity_followup_test");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());
    insert_article(&conn, &project_id, 1, "my-post");
    (conn, project_id, project_dir)
}

#[test]
fn every_issue_type_maps_to_fix_content_article_with_article_id() {
    let (conn, project_id, project_dir) = fixture();
    let parent = create_parent_task(&conn, &project_id, "investigate_clarity");

    let issue_types = [
        "Quickback bounces",
        "Low engagement",
        "Rage clicks",
        "Dead clicks",
        "Script errors",
        "Mobile UX",
    ];
    let findings: Vec<_> = issue_types
        .iter()
        .map(|t| finding(t, "https://example.com/blog/my-post"))
        .collect();

    let result = spawn_tasks_from_selection(&conn, &parent.id, &findings).unwrap();

    assert!(result.skipped.is_empty(), "no finding should be skipped");
    assert_eq!(result.created_tasks.len(), issue_types.len());
    for task in &result.created_tasks {
        assert_eq!(
            task.task_type, "fix_content_article",
            "every finding must map to fix_content_article, never create_landing_page/write_article"
        );
        // The artifact must resolve an article_id the way fix_context does:
        // a `recommendations_*` artifact whose content carries `article_id`.
        let artifact = task
            .artifacts
            .iter()
            .find(|a| a.key.starts_with("recommendations_"))
            .expect("fix task must carry a recommendations_* artifact");
        let content: serde_json::Value =
            serde_json::from_str(artifact.content.as_deref().unwrap()).unwrap();
        assert_eq!(content["article_id"].as_i64(), Some(1));
        assert_eq!(
            content["suggestions"].as_array().map(|s| s.len()),
            Some(1),
            "finding evidence must be carried as a suggestion"
        );
    }

    // Parent is marked done by the selection command.
    let parent_after = task_store::get_task(&conn, &parent.id).unwrap();
    assert_eq!(parent_after.status, TaskStatus::Done);

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn picker_also_works_for_clarity_analytics_parent() {
    let (conn, project_id, project_dir) = fixture();
    let parent = create_parent_task(&conn, &project_id, "clarity_analytics");

    let findings = vec![finding("Rage clicks", "https://example.com/blog/my-post")];
    let result = spawn_tasks_from_selection(&conn, &parent.id, &findings).unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    assert_eq!(result.created_tasks[0].task_type, "fix_content_article");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn duplicate_submission_is_suppressed_by_idempotency_key() {
    let (conn, project_id, project_dir) = fixture();
    let parent = create_parent_task(&conn, &project_id, "investigate_clarity");

    let findings = vec![finding("Rage clicks", "https://example.com/blog/my-post")];
    let first = spawn_tasks_from_selection(&conn, &parent.id, &findings).unwrap();
    assert_eq!(first.created_tasks.len(), 1);

    // Re-submitting the same finding must not create a second task.
    let second = spawn_tasks_from_selection(&conn, &parent.id, &findings).unwrap();
    assert_eq!(second.created_tasks.len(), 1);
    assert_eq!(first.created_tasks[0].id, second.created_tasks[0].id);

    let task_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE type = 'fix_content_article'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_count, 1, "re-clicking Create Tasks must not duplicate");

    let _ = std::fs::remove_dir_all(&project_dir);
}

#[test]
fn unresolvable_url_is_skipped_with_explanation() {
    let (conn, project_id, project_dir) = fixture();
    let parent = create_parent_task(&conn, &project_id, "investigate_clarity");

    let findings = vec![
        finding("Rage clicks", "https://example.com/blog/my-post"),
        finding("Dead clicks", "https://external.com/pricing"),
        finding("Script errors", "https://example.com/cart"),
    ];
    let result = spawn_tasks_from_selection(&conn, &parent.id, &findings).unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    assert_eq!(result.skipped.len(), 2);
    for skip in &result.skipped {
        assert!(
            !skip.reason.is_empty(),
            "every skipped finding must carry an explanation"
        );
    }
    assert!(result.skipped.iter().any(|s| s.url.contains("external.com")));
    assert!(result.skipped.iter().any(|s| s.url.contains("/cart")));

    let _ = std::fs::remove_dir_all(&project_dir);
}
