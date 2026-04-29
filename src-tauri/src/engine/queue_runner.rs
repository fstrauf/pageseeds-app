/// Background queue execution runtime.
///
/// Runs a sequence of tasks sequentially, emitting progress events via Tauri
/// so the UI can track execution state.
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

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
    #[serde(rename = "executionMode")]
    pub execution_mode: String,
}

/// Execute a queue of tasks in the background, emitting progress events.
pub async fn execute_queue_internal(
    items: Vec<QueueItem>,
    db_path: PathBuf,
    app_handle: AppHandle,
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

        // Emit started event AFTER database is updated
        let event = QueueProgressEvent {
            event_type: "started".to_string(),
            task_id: item.task_id.clone(),
            project_id: item.project_id.clone(),
            payload: serde_json::json!({
                "index": index,
                "total": items.len(),
                "title": item.title.clone(),
                "task_type": item.task_type.clone(),
            }),
        };

        log::info!(
            "[queue_runner] Emitting queue:task-started for task {}",
            item.task_id
        );
        match app_handle.emit("queue:task-started", &event) {
            Ok(_) => log::info!("[queue_runner] Successfully emitted started event"),
            Err(e) => log::error!("[queue_runner] Failed to emit started event: {}", e),
        }

        // Execute task in blocking thread with local runtime
        let task_id = item.task_id.clone();
        let db_path_clone = db_path.clone();
        let app_handle_clone = app_handle.clone();
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
                    Some(app_handle_clone),
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

                let event = QueueProgressEvent {
                    event_type: if exec_result.success {
                        "completed"
                    } else {
                        "failed"
                    }
                    .to_string(),
                    task_id: item.task_id.clone(),
                    project_id: item.project_id.clone(),
                    payload: serde_json::json!({
                        "message": exec_result.message,
                        "error": if exec_result.success { serde_json::Value::Null } else { serde_json::Value::String(exec_result.message.clone()) },
                        "success": exec_result.success,
                        "started_at": exec_result.started_at,
                        "finished_at": exec_result.finished_at,
                        "follow_up_tasks": exec_result.follow_up_tasks,
                    }),
                };

                let event_name = if exec_result.success {
                    "queue:task-completed"
                } else {
                    "queue:task-failed"
                };
                log::info!(
                    "[queue_runner] Emitting {} for task {}",
                    event_name,
                    item.task_id
                );
                match app_handle.emit(event_name, &event) {
                    Ok(_) => log::info!(
                        "[queue_runner] Successfully emitted {} event",
                        event.event_type
                    ),
                    Err(e) => log::error!(
                        "[queue_runner] Failed to emit {} event: {}",
                        event.event_type,
                        e
                    ),
                }

                // Emit follow-up created events for automatic/batchable follow-ups
                if exec_result.success {
                    for follow_up in &exec_result.follow_up_tasks {
                        if follow_up.execution_mode == "automatic"
                            || follow_up.execution_mode == "batchable"
                        {
                            let follow_up_event = FollowUpCreatedEvent {
                                task_id: follow_up.id.clone(),
                                project_id: item.project_id.clone(),
                                title: follow_up.title.clone(),
                                task_type: follow_up.task_type.clone(),
                                execution_mode: follow_up.execution_mode.clone(),
                            };

                            log::info!(
                                "[queue_runner] Emitting follow-up created: {} (mode: {})",
                                follow_up.id,
                                follow_up.execution_mode
                            );

                            if let Err(e) =
                                app_handle.emit("queue:follow-up-created", &follow_up_event)
                            {
                                log::error!("[queue_runner] Failed to emit follow-up event: {}", e);
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                log::warn!("[queue_runner] Task failed: {}", e);
                let event = QueueProgressEvent {
                    event_type: "failed".to_string(),
                    task_id: item.task_id.clone(),
                    project_id: item.project_id.clone(),
                    payload: serde_json::json!({
                        "error": e,
                        "message": e,
                    }),
                };

                if let Err(e) = app_handle.emit("queue:task-failed", &event) {
                    log::error!("[queue_runner] Failed to emit failed event: {}", e);
                }
            }
            Err(e) => {
                log::error!("[queue_runner] Task panicked: {:?}", e);
                let event = QueueProgressEvent {
                    event_type: "failed".to_string(),
                    task_id: item.task_id.clone(),
                    project_id: item.project_id.clone(),
                    payload: serde_json::json!({
                        "error": format!("Task panicked: {:?}", e),
                        "message": "Task execution failed",
                    }),
                };

                if let Err(e) = app_handle.emit("queue:task-failed", &event) {
                    log::error!("[queue_runner] Failed to emit failed event: {}", e);
                }
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
    log::info!("[queue_runner] Emitting queue:finished event");
    if let Err(e) = app_handle.emit("queue:finished", ()) {
        log::error!("[queue_runner] Failed to emit finished event: {}", e);
    }
    log::info!("[queue_runner] Queue execution finished");
}
