/// Background queue execution runtime.
///
/// Runs a sequence of tasks sequentially, emitting progress events via Tauri
/// so the UI can track execution state.
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::engine::executor;
use crate::engine::task_store;
use crate::models::task::TaskStatus;

/// A queue item for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "projectId")]
    pub project_id: String,
    pub title: String,
    #[serde(rename = "taskType")]
    pub task_type: String,
    #[serde(rename = "projectName")]
    pub project_name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Progress event emitted during queue execution
#[derive(Debug, Clone, Serialize)]
pub struct QueueProgressEvent {
    #[serde(rename = "eventType")]
    pub event_type: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "projectId")]
    pub project_id: String,
    pub payload: serde_json::Value,
}

/// Follow-up task notification
#[derive(Debug, Clone, Serialize)]
pub struct FollowUpCreatedEvent {
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "projectId")]
    pub project_id: String,
    pub title: String,
    #[serde(rename = "taskType")]
    pub task_type: String,
    #[serde(rename = "runPolicy")]
    pub run_policy: String,
}

/// Execute a queue of tasks in the background, emitting progress events.
pub async fn execute_queue_internal(
    items: Vec<QueueItem>,
    db_path: PathBuf,
    gsc_token: Option<String>,
) {
    log::info!("[queue_runner] ==========================================");
    log::info!("[queue_runner] STARTING EXECUTION OF {} TASKS", items.len());
    log::info!("[queue_runner] DB path: {:?}", db_path);

    for (index, item) in items.iter().enumerate() {
        log::info!("[queue_runner] ------------------------------------------");
        log::info!(
            "[queue_runner] TASK {}/{}: {}",
            index + 1,
            items.len(),
            item.title
        );
        log::info!("[queue_runner]   ID: {}", item.task_id);
        log::info!("[queue_runner]   Type: {}", item.task_type);
        log::info!("[queue_runner]   Project: {}", item.project_id);

        // Mark task as in_progress in the database BEFORE emitting event
        let db_path_clone = db_path.clone();
        let task_id = item.task_id.clone();
        let update_result: Result<(), String> = tokio::task::spawn_blocking(move || {
            let conn = Connection::open(&db_path_clone)
                .map_err(|e| format!("Failed to open DB: {}", e))?;

            task_store::update_task_status(&conn, &task_id, TaskStatus::InProgress)
                .map_err(|e| format!("Failed to update task status: {}", e))?;

            log::info!("[queue_runner] Task {} marked as in_progress", task_id);
            Ok(())
        })
        .await
        .map_err(|e| format!("Task panicked: {:?}", e))
        .and_then(|r| r);

        if let Err(e) = update_result {
            log::error!("[queue_runner] Failed to mark task as in_progress: {}", e);
        }

        // Log started event AFTER database is updated (desktop emit deferred to #184)
        log::info!(
            "[queue_runner] queue:task-started task={} index={}/{} type={}",
            item.task_id,
            index + 1,
            items.len(),
            item.task_type
        );

        // Execute task in blocking thread with local runtime
        let task_id = item.task_id.clone();
        let db_path_clone = db_path.clone();
        let gsc_token_clone = gsc_token.clone();

        log::info!("[queue_runner] Spawning blocking task for {}", item.task_id);
        let result = tokio::task::spawn_blocking(move || {
            log::info!("[queue_runner] Blocking task started for {}", task_id);
            let conn = match Connection::open(&db_path_clone) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[queue_runner] Failed to open DB: {}", e);
                    return Err(format!("DB error: {}", e));
                }
            };

            conn.busy_timeout(Duration::from_secs(10))
                .map_err(|e| format!("Failed to set busy timeout: {}", e))?;

            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => return Err(format!("Runtime error: {}", e)),
            };

            let result = rt.block_on(async {
                executor::execute_task_with_token(
                    &conn,
                    &task_id,
                    gsc_token_clone.as_deref(),
                    false,
                )
                .await
            });
            log::info!("[queue_runner] Blocking task finished for {}", task_id);
            result
        })
        .await;
        log::info!(
            "[queue_runner] Spawn blocking returned for {}",
            item.task_id
        );

        // Handle result and emit completion event
        log::info!("[queue_runner] Task execution completed, handling result");
        match result {
            Ok(Ok(exec_result)) => {
                if exec_result.success {
                    log::info!(
                        "[queue_runner] Task {} completed: {}",
                        item.task_id,
                        exec_result.message
                    );
                } else {
                    log::warn!(
                        "[queue_runner] Task {} finished with workflow failure: {}",
                        item.task_id,
                        exec_result.message
                    );
                }

                let event_name = if exec_result.success {
                    "queue:task-completed"
                } else {
                    "queue:task-failed"
                };
                log::info!(
                    "[queue_runner] {} for task {} message={}",
                    event_name,
                    item.task_id,
                    exec_result.message
                );

                // Emit follow-up created events for automatic/batchable follow-ups
                if exec_result.success {
                    for follow_up in &exec_result.follow_up_tasks {
                        if follow_up.run_policy == "auto_enqueue" {
                            log::info!(
                                "[queue_runner] queue:follow-up-created: {} (policy: {})",
                                follow_up.id,
                                follow_up.run_policy
                            );
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                log::warn!("[queue_runner] Task failed: {}", e);
                log::info!(
                    "[queue_runner] queue:task-failed task={} error={}",
                    item.task_id,
                    e
                );
            }
            Err(e) => {
                log::error!("[queue_runner] Task panicked: {:?}", e);
                log::info!(
                    "[queue_runner] queue:task-failed task={} panic={:?}",
                    item.task_id,
                    e
                );
            }
        }

        // Small delay between tasks to prevent database contention
        log::info!(
            "[queue_runner] Task {}/{} finished, sleeping before next...",
            index + 1,
            items.len()
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        log::info!("[queue_runner] Continuing to next task...");
    }

    // Emit finished event
    log::info!("[queue_runner] ==========================================");
    log::info!("[queue_runner] ALL {} TASKS COMPLETE", items.len());
    log::info!("[queue_runner] queue:finished");
    log::info!("[queue_runner] Queue execution finished");
}
