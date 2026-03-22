/// Autonomous batch processing — executes all ready automatic/batchable tasks.
///
/// Mirrors Python `dashboard_ptk/dashboard/batch.py`.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::engine::{executor, task_store};
use crate::models::task::Task;

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
    pub automatic: usize,
    pub batchable: usize,
}

// ─── Autonomy mode helpers ────────────────────────────────────────────────────

fn autonomy_mode(task: &Task) -> &'static str {
    match task.execution_mode.as_str() {
        "automatic" => "automatic",
        "batchable" => "batchable",
        _ => "manual",
    }
}

fn is_autonomous(task: &Task) -> bool {
    matches!(autonomy_mode(task), "automatic" | "batchable")
}

// ─── Ready task selection ─────────────────────────────────────────────────────

/// Returns tasks that are todo, autonomous, and have all dependencies done.
pub fn get_ready_tasks(conn: &Connection, project_id: &str) -> Result<Vec<Task>, String> {
    let all_tasks = task_store::list_tasks(conn, project_id).map_err(|e| e.to_string())?;
    let done_ids: std::collections::HashSet<String> = all_tasks
        .iter()
        .filter(|t| t.status == "done")
        .map(|t| t.id.clone())
        .collect();

    let mut ready: Vec<Task> = all_tasks
        .into_iter()
        .filter(|t| {
            t.status == "todo"
                && is_autonomous(t)
                && t.depends_on.iter().all(|dep| done_ids.contains(dep))
        })
        .collect();

    let priority_order = |p: &str| match p {
        "high" => 0u8,
        "medium" => 1,
        _ => 2,
    };
    ready.sort_by_key(|t| priority_order(&t.priority));
    Ok(ready)
}

pub fn get_batch_summary(conn: &Connection, project_id: &str) -> Result<BatchSummary, String> {
    let ready = get_ready_tasks(conn, project_id)?;
    Ok(BatchSummary {
        total_ready: ready.len(),
        automatic: ready.iter().filter(|t| autonomy_mode(t) == "automatic").count(),
        batchable: ready.iter().filter(|t| autonomy_mode(t) == "batchable").count(),
    })
}

// ─── Batch runner ─────────────────────────────────────────────────────────────

pub fn run_batch(
    conn: &Connection,
    project_id: &str,
    config: &BatchConfig,
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

        match executor::execute_task(conn, &task_id) {
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
