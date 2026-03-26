/// Queue execution commands for PageSeeds
///
/// Handles background task queue execution with proper async handling
/// and event emission for UI updates.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::commands::AppState;
use crate::engine::executor;

/// A queue item for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub task_type: String,
    #[serde(rename = "projectName")]
    pub project_name: Option<String>,
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

/// Execute a queue of tasks across projects
#[tauri::command]
pub async fn execute_queue(
    items: Vec<QueueItem>,
    state: tauri::State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!("[execute_queue] Called with {} items", items.len());
    
    let db_path = state.db_path.clone();
    
    // Spawn background execution
    tokio::spawn(async move {
        execute_queue_internal(items, db_path, app_handle).await;
    });
    
    Ok(())
}

/// Internal queue execution - runs in background
async fn execute_queue_internal(
    items: Vec<QueueItem>,
    db_path: PathBuf,
    app_handle: AppHandle,
) {
    log::info!("[execute_queue_internal] Starting execution of {} tasks", items.len());
    
    for (index, item) in items.iter().enumerate() {
        log::info!("[execute_queue_internal] Task {}/{}: {} ({})", 
            index + 1, items.len(), item.title, item.task_id);
        
        // Emit started event
        let event = QueueProgressEvent {
            event_type: "started".to_string(),
            task_id: item.task_id.clone(),
            project_id: item.project_id.clone(),
            payload: serde_json::json!({
                "index": index,
                "total": items.len(),
                "title": item.title,
                "task_type": item.task_type,
            }),
        };
        
        if let Err(e) = app_handle.emit("queue:task-started", &event) {
            log::error!("[execute_queue_internal] Failed to emit started event: {}", e);
        }
        
        // Execute task in blocking thread with local runtime
        let task_id = item.task_id.clone();
        let project_id = item.project_id.clone();
        let db_path_clone = db_path.clone();
        let app_handle_clone = app_handle.clone();
        
        let result = tokio::task::spawn_blocking(move || {
            let conn = match Connection::open(&db_path_clone) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("[execute_queue_internal] Failed to open DB: {}", e);
                    return Err(format!("DB error: {}", e));
                }
            };
            
            conn.busy_timeout(Duration::from_secs(10))
                .map_err(|e| format!("Failed to set busy timeout: {}", e))?;
            
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => return Err(format!("Runtime error: {}", e)),
            };
            
            // Use the runtime to block on the async executor
            rt.block_on(async {
                executor::execute_task_with_token(
                    &conn,
                    &task_id,
                    None,
                    Some(app_handle_clone),
                    false,
                ).await
            })
        }).await;
        
        // Handle result and emit completion event
        match result {
            Ok(Ok(exec_result)) => {
                // Emit completion event
                let event = QueueProgressEvent {
                    event_type: if exec_result.success { "completed" } else { "failed" }.to_string(),
                    task_id: item.task_id.clone(),
                    project_id: item.project_id.clone(),
                    payload: serde_json::json!({
                        "message": exec_result.message,
                        "success": exec_result.success,
                    }),
                };
                
                if let Err(e) = app_handle.emit("queue:task-completed", &event) {
                    log::error!("[execute_queue_internal] Failed to emit completed event: {}", e);
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
                log::error!("[execute_queue_internal] Task panicked: {}", e);
                let event = QueueProgressEvent {
                    event_type: "failed".to_string(),
                    task_id: item.task_id.clone(),
                    project_id: item.project_id.clone(),
                    payload: serde_json::json!({
                        "error": format!("Task panicked: {}", e),
                        "message": "Task execution failed",
                    }),
                };
                
                if let Err(e) = app_handle.emit("queue:task-failed", &event) {
                    log::error!("[execute_queue_internal] Failed to emit failed event: {}", e);
                }
            }
        }
    }
    
    // Emit finished event
    log::info!("[execute_queue_internal] All tasks complete");
    if let Err(e) = app_handle.emit("queue:finished", ()) {
        log::error!("[execute_queue_internal] Failed to emit finished event: {}", e);
    }
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

/// Direct task execution - runs a single task immediately without queue
/// This is for debugging/troubleshooting when the queue system isn't working
#[tauri::command]
pub async fn execute_task_direct(
    task_id: String,
    project_id: String,
    state: tauri::State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<executor::ExecutionResult, String> {
    log::info!("[execute_task_direct] Called for task {} in project {}", task_id, project_id);
    
    let db_path = state.db_path.clone();
    
    // Execute directly in blocking thread
    let result = tokio::task::spawn_blocking(move || {
        let conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to open DB: {}", e)),
        };
        
        conn.busy_timeout(Duration::from_secs(10))
            .map_err(|e| format!("Failed to set busy timeout: {}", e))?;
        
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => return Err(format!("Runtime error: {}", e)),
        };
        
        rt.block_on(async {
            executor::execute_task_with_token(
                &conn,
                &task_id,
                None,
                Some(app_handle),
                false,
            ).await
        })
    }).await;
    
    match result {
        Ok(Ok(exec_result)) => {
            log::info!("[execute_task_direct] Task completed: success={}, message={}", 
                exec_result.success, exec_result.message);
            Ok(exec_result)
        }
        Ok(Err(e)) => {
            log::error!("[execute_task_direct] Task failed: {}", e);
            Err(e)
        }
        Err(e) => {
            log::error!("[execute_task_direct] Task panicked: {}", e);
            Err(format!("Task execution panicked: {}", e))
        }
    }
}
