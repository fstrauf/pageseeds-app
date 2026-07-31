//! Tests for the not_before due gate (issue #307).
//!
//! Kept separate from `tests.rs` so that module does not grow past ~1k lines
//! with pure-gate and force/block integration cases.

use super::*;
use crate::engine::task_store;
use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
    TaskStatus,
};
use rusqlite::Connection;

fn in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::db::init_with_conn(&conn).unwrap();
    conn
}

fn test_project_in(conn: &Connection) -> String {
    conn.execute(
        "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp', 1)",
        [],
    )
    .unwrap();
    "proj1".to_string()
}

fn make_task(task_type: &str, project_id: &str) -> Task {
    Task {
        id: format!("test-{task_type}"),
        task_type: task_type.to_string(),
        phase: "research".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some(format!("{task_type} test")),
        description: None,
        project_id: project_id.to_string(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun {
            attempts: 0,
            last_error: None,
            provider: None,
            ..Default::default()
        },
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        not_before: None,
    }
}

#[test]
fn task_is_due_null_not_before() {
    let task = make_task("collect_gsc", "proj1");
    assert!(task_is_due(&task, chrono::Utc::now()));
}

#[test]
fn task_is_due_past_not_before() {
    let mut task = make_task("collect_gsc", "proj1");
    task.not_before = Some((chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339());
    assert!(task_is_due(&task, chrono::Utc::now()));
}

#[test]
fn task_is_due_future_not_before() {
    let mut task = make_task("collect_gsc", "proj1");
    task.not_before = Some((chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339());
    assert!(!task_is_due(&task, chrono::Utc::now()));
}

#[test]
fn task_is_due_unparseable_fails_open() {
    let mut task = make_task("collect_gsc", "proj1");
    task.not_before = Some("not-a-timestamp".to_string());
    assert!(task_is_due(&task, chrono::Utc::now()));
}

#[tokio::test]
async fn execute_blocks_future_not_before_without_force() {
    let conn = in_memory_db();
    let proj = test_project_in(&conn);
    let mut task = make_task("collect_gsc", &proj);
    let not_before = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
    task.not_before = Some(not_before.clone());
    let task_id = task.id.clone();
    task_store::create_task(&conn, &task).unwrap();

    let err = execute_task_with_token(&conn, &task_id, None, &ExecuteOpts::default())
        .await
        .expect_err("should refuse not-due task");
    assert!(
        err.contains("not due") || err.contains("not_before"),
        "error should mention not due / not_before, got: {err}"
    );
    assert!(
        err.contains(&not_before) || err.contains("--force"),
        "error should include timestamp or --force hint, got: {err}"
    );

    // Task must still be todo (never transitioned to in_progress).
    let saved = task_store::get_task(&conn, &task_id).unwrap();
    assert_eq!(saved.status, TaskStatus::Todo);
}

#[tokio::test]
async fn execute_ignore_not_before_bypasses_gate() {
    let conn = in_memory_db();
    let proj = test_project_in(&conn);
    let mut task = make_task("collect_gsc", &proj);
    task.not_before = Some((chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339());
    let task_id = task.id.clone();
    task_store::create_task(&conn, &task).unwrap();

    // ignore_not_before must pass the gate; may fail later for missing secrets/
    // handler side effects — only assert the gate did not return the not_before Err.
    let opts = ExecuteOpts {
        dry_run: true,
        ignore_not_before: true,
    };
    let result = execute_task_with_token(&conn, &task_id, None, &opts).await;
    match result {
        Ok(_) => {} // dry_run with ignore proceeds
        Err(e) => {
            assert!(
                !e.contains("not due") && !e.contains("not_before"),
                "ignore_not_before should bypass gate, got: {e}"
            );
        }
    }
}

#[tokio::test]
async fn execute_null_not_before_proceeds() {
    let conn = in_memory_db();
    let proj = test_project_in(&conn);
    let task = make_task("collect_gsc", &proj);
    let task_id = task.id.clone();
    task_store::create_task(&conn, &task).unwrap();

    // dry_run so we don't need GSC credentials; gate still applies.
    let opts = ExecuteOpts {
        dry_run: true,
        ..Default::default()
    };
    let result = execute_task_with_token(&conn, &task_id, None, &opts)
        .await
        .expect("null not_before should proceed");
    assert!(result.success, "dry-run should succeed: {}", result.message);
}
