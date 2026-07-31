/// Autonomous batch processing — executes all ready automatic/batchable tasks.
///
/// Mirrors Python `dashboard_ptk/dashboard/batch.py`.
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::engine::{executor, task_store};
use crate::models::task::{Priority, Task, TaskRunPolicy, TaskStatus};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Maximum number of tasks to process in one batch run.
    pub max_tasks: usize,
    /// Stop the batch on the first task error.
    pub pause_on_error: bool,
    /// Rate-limit delay between tasks (seconds).
    pub delay_secs: f64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_tasks: 20,
            pause_on_error: true,
            delay_secs: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTaskResult {
    pub task_id: String,
    pub task_type: String,
    pub title: String,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub status: String, // "complete" | "error" | "paused"
    pub processed: usize,
    pub errors: Vec<BatchTaskResult>,
    pub results: Vec<BatchTaskResult>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total_ready: usize,
    pub auto_enqueue: usize,
    pub user_enqueue: usize,
}

// ─── Autonomy mode helpers ────────────────────────────────────────────────────

fn is_autonomous(task: &Task) -> bool {
    matches!(task.run_policy, TaskRunPolicy::AutoEnqueue)
}

// ─── Ready task selection ─────────────────────────────────────────────────────

/// Returns tasks that are todo, autonomous, due (`not_before`), and have all
/// dependencies done. Scheduler/batch never picks a not-due task.
pub fn get_ready_tasks(conn: &Connection, project_id: &str) -> Result<Vec<Task>, String> {
    // Use list_tasks_light to avoid loading large artifact blobs into memory
    // when we only need status, type, and dependency info.
    let all_tasks = task_store::list_tasks_light(conn, project_id).map_err(|e| e.to_string())?;
    let done_ids: std::collections::HashSet<String> = all_tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .map(|t| t.id.clone())
        .collect();

    let now = Utc::now();
    let mut ready: Vec<Task> = all_tasks
        .into_iter()
        .filter(|t| {
            t.status == TaskStatus::Todo
                && is_autonomous(t)
                && t.depends_on.iter().all(|dep| done_ids.contains(dep))
                && executor::task_is_due(t, now)
        })
        .collect();

    ready.sort_by_key(|t| match t.priority {
        Priority::High => 0u8,
        Priority::Medium => 1,
        Priority::Low => 2,
    });
    Ok(ready)
}

pub fn get_batch_summary(conn: &Connection, project_id: &str) -> Result<BatchSummary, String> {
    let ready = get_ready_tasks(conn, project_id)?;
    Ok(BatchSummary {
        total_ready: ready.len(),
        auto_enqueue: ready
            .iter()
            .filter(|t| t.run_policy == TaskRunPolicy::AutoEnqueue)
            .count(),
        user_enqueue: ready
            .iter()
            .filter(|t| t.run_policy == TaskRunPolicy::UserEnqueue)
            .count(),
    })
}

// ─── Batch runner ─────────────────────────────────────────────────────────────

pub async fn run_batch(
    conn: &Connection,
    project_id: &str,
    config: &BatchConfig,
) -> Result<BatchResult, String> {
    run_batch_with_token(conn, project_id, config, None).await
}

pub async fn run_batch_with_token(
    conn: &Connection,
    project_id: &str,
    config: &BatchConfig,
    gsc_token: Option<&str>,
) -> Result<BatchResult, String> {
    let started = std::time::Instant::now();
    let mut processed = 0usize;
    let mut errors: Vec<BatchTaskResult> = Vec::new();
    let mut results: Vec<BatchTaskResult> = Vec::new();

    while processed < config.max_tasks {
        let ready = get_ready_tasks(conn, project_id)?;
        if ready.is_empty() {
            break;
        }

        let task = &ready[0];
        let task_id = task.id.clone();
        let task_type = task.task_type.clone();
        let title = task.title.clone().unwrap_or_default();

        log::info!("[batch] executing task {task_id} ({task_type})");

        match executor::execute_task_with_token(
            conn,
            &task_id,
            gsc_token,
            &executor::ExecuteOpts::default(),
        )
        .await
        {
            Ok(exec_result) => {
                let batch_task_result = BatchTaskResult {
                    task_id: task_id.clone(),
                    task_type: task_type.clone(),
                    title: title.clone(),
                    success: exec_result.success,
                    message: exec_result.message.clone(),
                };

                if exec_result.success {
                    processed += 1;
                    results.push(batch_task_result);
                } else {
                    errors.push(batch_task_result);
                    if config.pause_on_error {
                        return Ok(BatchResult {
                            status: "error".to_string(),
                            processed,
                            errors,
                            results,
                            duration_ms: started.elapsed().as_millis() as u64,
                        });
                    }
                    processed += 1; // count failures so we don't loop forever
                }
            }
            Err(e) => {
                errors.push(BatchTaskResult {
                    task_id: task_id.clone(),
                    task_type,
                    title,
                    success: false,
                    message: e.clone(),
                });
                if config.pause_on_error {
                    return Ok(BatchResult {
                        status: "error".to_string(),
                        processed,
                        errors,
                        results,
                        duration_ms: started.elapsed().as_millis() as u64,
                    });
                }
                processed += 1;
            }
        }

        if config.delay_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(config.delay_secs));
        }
    }

    Ok(BatchResult {
        status: "complete".to_string(),
        processed,
        errors,
        results,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp', 1)",
            [],
        )
        .unwrap();
        conn
    }

    fn make_auto_task(id: &str, not_before: Option<String>) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: id.to_string(),
            task_type: "collect_gsc".to_string(),
            phase: "collection".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: Some(id.to_string()),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
            not_before,
        }
    }

    #[test]
    fn get_ready_tasks_excludes_future_not_before() {
        let conn = in_memory_db();
        let future = (chrono::Utc::now() + chrono::Duration::days(14)).to_rfc3339();
        let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();

        task_store::create_task(&conn, &make_auto_task("due-null", None)).unwrap();
        task_store::create_task(&conn, &make_auto_task("due-past", Some(past))).unwrap();
        task_store::create_task(&conn, &make_auto_task("not-due", Some(future))).unwrap();

        let ready = get_ready_tasks(&conn, "proj1").unwrap();
        let ids: Vec<&str> = ready.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"due-null"), "null not_before should be ready: {ids:?}");
        assert!(ids.contains(&"due-past"), "past not_before should be ready: {ids:?}");
        assert!(
            !ids.contains(&"not-due"),
            "future not_before must be excluded: {ids:?}"
        );
    }
}
