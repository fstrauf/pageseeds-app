/// Queue execution commands for PageSeeds
///
/// Handles background task queue execution with proper async handling
/// and event emission for UI updates.

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::commands::AppState;
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

/// Mark tasks as queued in the database when added to the queue
#[tauri::command]
pub async fn mark_tasks_queued(
    task_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[mark_tasks_queued] Marking {} tasks as queued", task_ids.len());
    
    if task_ids.is_empty() {
        return Ok(());
    }
    
    let db_path = state.db_path.clone();
    
    tokio::task::spawn_blocking(move || {
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open DB: {}", e))?;
        
        for task_id in &task_ids {
            if let Err(e) = task_store::update_task_status(&conn, task_id, TaskStatus::Queued) {
                log::warn!("[mark_tasks_queued] Failed to mark task {} as queued: {}", task_id, e);
            } else {
                log::info!("[mark_tasks_queued] Task {} marked as queued", task_id);
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Task panicked: {:?}", e))?
}

/// Reset queued tasks back to todo (called when removing from queue)
#[tauri::command]
pub async fn mark_tasks_todo(
    task_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[mark_tasks_todo] Resetting {} tasks to todo", task_ids.len());
    
    if task_ids.is_empty() {
        return Ok(());
    }
    
    let db_path = state.db_path.clone();
    
    tokio::task::spawn_blocking(move || {
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open DB: {}", e))?;
        
        for task_id in &task_ids {
            // Only reset if currently queued (not if already running)
            if let Ok(task) = task_store::get_task(&conn, task_id) {
                if task.status == TaskStatus::Queued {
                    if let Err(e) = task_store::update_task_status(&conn, task_id, TaskStatus::Todo) {
                        log::warn!("[mark_tasks_todo] Failed to reset task {} to todo: {}", task_id, e);
                    } else {
                        log::info!("[mark_tasks_todo] Task {} reset to todo", task_id);
                    }
                }
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Task panicked: {:?}", e))?
}

/// Execute a queue of tasks across projects
#[tauri::command]
pub async fn execute_queue(
    items: Vec<QueueItem>,
    state: tauri::State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!("[execute_queue] Called with {} items", items.len());
    
    if items.is_empty() {
        log::warn!("[execute_queue] No items to execute");
        return Ok(());
    }
    
    for (i, item) in items.iter().enumerate() {
        log::info!("[execute_queue] Item {}: {} ({})", i, item.task_id, item.title);
    }
    
    let db_path = state.db_path.clone();
    
    // Spawn background execution
    tokio::spawn(async move {
        let item_count = items.len();
        log::info!("[execute_queue] Spawning background task with {} items", item_count);
        execute_queue_internal(items, db_path, app_handle).await;
        log::info!("[execute_queue] Background task completed");
    });
    
    Ok(())
}

/// Internal queue execution - runs in background
async fn execute_queue_internal(
    items: Vec<QueueItem>,
    db_path: PathBuf,
    app_handle: AppHandle,
) {
    log::info!("[execute_queue_internal] ==========================================");
    log::info!("[execute_queue_internal] STARTING EXECUTION OF {} TASKS", items.len());
    log::info!("[execute_queue_internal] DB path: {:?}", db_path);

    for (index, item) in items.iter().enumerate() {
        log::info!("[execute_queue_internal] ------------------------------------------");
        log::info!("[execute_queue_internal] TASK {}/{}: {}", index + 1, items.len(), item.title);
        log::info!("[execute_queue_internal]   ID: {}", item.task_id);
        log::info!("[execute_queue_internal]   Type: {}", item.task_type);
        log::info!("[execute_queue_internal]   Project: {}", item.project_id);
        
        // First, mark task as in_progress in the database BEFORE emitting event
        // This ensures the UI shows the correct state
        let db_path_clone = db_path.clone();
        let task_id = item.task_id.clone();
        let update_result: Result<(), String> = tokio::task::spawn_blocking(move || {
            let conn = Connection::open(&db_path_clone)
                .map_err(|e| format!("Failed to open DB: {}", e))?;
            
            task_store::update_task_status(&conn, &task_id, TaskStatus::InProgress)
                .map_err(|e| format!("Failed to update task status: {}", e))?;
            
            log::info!("[execute_queue_internal] Task {} marked as in_progress", task_id);
            Ok(())
        }).await.map_err(|e| format!("Task panicked: {:?}", e)).and_then(|r| r);
        
        if let Err(e) = update_result {
            log::error!("[execute_queue_internal] Failed to mark task as in_progress: {}", e);
            // Continue anyway - the executor will also try to update status
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
        
        log::info!("[execute_queue_internal] Emitting queue:task-started for task {}", item.task_id);
        match app_handle.emit("queue:task-started", &event) {
            Ok(_) => log::info!("[execute_queue_internal] Successfully emitted started event"),
            Err(e) => log::error!("[execute_queue_internal] Failed to emit started event: {}", e),
        }
        
        // Execute task in blocking thread with local runtime
        let task_id = item.task_id.clone();
        let _project_id = item.project_id.clone();
        let db_path_clone = db_path.clone();
        let app_handle_clone = app_handle.clone();
        
        log::info!("[execute_queue_internal] Spawning blocking task for {}", item.task_id);
        let result = tokio::task::spawn_blocking(move || {
            log::info!("[execute_queue_internal] Blocking task started for {}", task_id);
            let conn = match Connection::open(&db_path_clone) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[execute_queue_internal] Failed to open DB: {}", e);
                    return Err(format!("DB error: {}", e));
                }
            };
            
            conn.busy_timeout(Duration::from_secs(10))
                .map_err(|e| format!("Failed to set busy timeout: {}", e))?;
            
            // Use a lightweight current-thread runtime instead of Runtime::new().
            // Runtime::new() spawns a multi-threaded pool (3-5 MB per task);
            // current_thread runs on this blocking thread only, so memory
            // overhead is negligible and it's dropped safely in this closure.
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
                    None,
                    Some(app_handle_clone),
                    false,
                ).await
            });
            // rt is dropped here inside the blocking closure — safe.
            log::info!("[execute_queue_internal] Blocking task finished for {}", task_id);
            result
        }).await;
        log::info!("[execute_queue_internal] Spawn blocking returned for {}", item.task_id);
        
        // Handle result and emit completion event
        log::info!("[execute_queue_internal] Task execution completed, handling result");
        match result {
            Ok(Ok(exec_result)) => {
                log::info!("[execute_queue_internal] Task {} succeeded: {}", 
                    item.task_id, exec_result.message);
                
                // Emit completion event with follow_up_tasks for review tasks
                let event = QueueProgressEvent {
                    event_type: if exec_result.success { "completed" } else { "failed" }.to_string(),
                    task_id: item.task_id.clone(),
                    project_id: item.project_id.clone(),
                    payload: serde_json::json!({
                        "message": exec_result.message,
                        "success": exec_result.success,
                        "started_at": exec_result.started_at,
                        "finished_at": exec_result.finished_at,
                        "follow_up_tasks": exec_result.follow_up_tasks,
                    }),
                };
                
                log::info!("[execute_queue_internal] Emitting queue:task-completed for task {}", item.task_id);
                match app_handle.emit("queue:task-completed", &event) {
                    Ok(_) => log::info!("[execute_queue_internal] Successfully emitted completed event"),
                    Err(e) => log::error!("[execute_queue_internal] Failed to emit completed event: {}", e),
                }
                
                // Emit follow-up created events for automatic/batchable follow-ups
                if exec_result.success {
                    for follow_up in &exec_result.follow_up_tasks {
                        if follow_up.execution_mode == "automatic" || follow_up.execution_mode == "batchable" {
                            let follow_up_event = FollowUpCreatedEvent {
                                task_id: follow_up.id.clone(),
                                project_id: item.project_id.clone(),
                                title: follow_up.title.clone(),
                                task_type: follow_up.task_type.clone(),
                                execution_mode: follow_up.execution_mode.clone(),
                            };
                            
                            log::info!("[execute_queue_internal] Emitting follow-up created: {} (mode: {})", 
                                follow_up.id, follow_up.execution_mode);
                            
                            if let Err(e) = app_handle.emit("queue:follow-up-created", &follow_up_event) {
                                log::error!("[execute_queue_internal] Failed to emit follow-up event: {}", e);
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                log::warn!("[execute_queue_internal] Task failed: {}", e);
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
                    log::error!("[execute_queue_internal] Failed to emit failed event: {}", e);
                }
            }
            Err(e) => {
                log::error!("[execute_queue_internal] Task panicked: {:?}", e);
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
                    log::error!("[execute_queue_internal] Failed to emit failed event: {}", e);
                }
            }
        }
        
        // Small delay between tasks to prevent database contention
        log::info!("[execute_queue_internal] Task {}/{} finished, sleeping before next...", index + 1, items.len());
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        log::info!("[execute_queue_internal] Continuing to next task...");
    }
    
    // Emit finished event
    log::info!("[execute_queue_internal] ==========================================");
    log::info!("[execute_queue_internal] ALL {} TASKS COMPLETE", items.len());
    log::info!("[execute_queue_internal] Emitting queue:finished event");
    if let Err(e) = app_handle.emit("queue:finished", ()) {
        log::error!("[execute_queue_internal] Failed to emit finished event: {}", e);
    }
    log::info!("[execute_queue_internal] Queue execution finished");
}

/// Pause queue execution (placeholder)
#[tauri::command]
pub async fn pause_queue() -> Result<(), String> {
    log::info!("[pause_queue] Called");
    Ok(())
}

/// Resume queue execution (placeholder)
#[tauri::command]
pub async fn resume_queue() -> Result<(), String> {
    log::info!("[resume_queue] Called");
    Ok(())
}

/// Clear completed queue items (placeholder)
#[tauri::command]
pub async fn clear_completed_queue_items() -> Result<(), String> {
    log::info!("[clear_completed_queue_items] Called");
    Ok(())
}


