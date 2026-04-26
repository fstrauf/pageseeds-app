
use rusqlite::Connection;
use tauri::AppHandle;

use crate::commands::AppState;
use crate::engine::queue_runner;
use crate::engine::task_store;
use crate::models::task::TaskStatus;

// Re-export types used by the frontend via IPC.
pub use crate::engine::queue_runner::QueueItem;

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
        queue_runner::execute_queue_internal(items, db_path, app_handle).await;
        log::info!("[execute_queue] Background task completed");
    });

    Ok(())
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
