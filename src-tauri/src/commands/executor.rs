use tauri::{AppHandle, State};

use crate::commands::{AppState, GscState};
use crate::engine::queue;
use crate::engine::task_store;
use crate::models::queue::{EnqueueItem, EnqueueMode, QueueSnapshot};

// Re-export legacy types used by the frontend via IPC.
pub use crate::engine::queue_runner::QueueItem;

/// Enqueue tasks into the backend-owned queue.
#[tauri::command]
pub async fn enqueue_tasks(
    items: Vec<EnqueueItem>,
    mode: EnqueueMode,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    gsc_state: State<'_, GscState>,
) -> Result<QueueSnapshot, String> {
    log::info!("[enqueue_tasks] Called with {} items, mode={:?}", items.len(), mode);

    let snapshot = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        queue::enqueue_tasks(&db, items, mode).map_err(|e| e.to_string())?
    };

    // Resolve GSC token and start runner if needed
    let gsc_token = resolve_gsc_token_for_queue(&state, &gsc_state, &snapshot).await?;
    queue::ensure_runner_started(state.db_path.clone(), app_handle, gsc_token).await;

    Ok(snapshot)
}

/// Remove a pending item from the queue.
#[tauri::command]
pub async fn remove_queue_item(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<QueueSnapshot, String> {
    log::info!("[remove_queue_item] Called for task {}", task_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    queue::remove_queue_item(&db, &task_id).map_err(|e| e.to_string())
}

/// Pause queue execution (finishes current task, then stops).
#[tauri::command]
pub async fn pause_queue(state: State<'_, AppState>) -> Result<QueueSnapshot, String> {
    log::info!("[pause_queue] Called");
    let db = state.db.lock().map_err(|e| e.to_string())?;
    queue::pause_queue(&db).map_err(|e| e.to_string())
}

/// Resume queue execution.
#[tauri::command]
pub async fn resume_queue(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    gsc_state: State<'_, GscState>,
) -> Result<QueueSnapshot, String> {
    log::info!("[resume_queue] Called");
    let snapshot = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        queue::resume_queue(&db).map_err(|e| e.to_string())?
    };

    let gsc_token = resolve_gsc_token_for_queue(&state, &gsc_state, &snapshot).await?;
    queue::ensure_runner_started(state.db_path.clone(), app_handle, gsc_token).await;

    Ok(snapshot)
}

/// Get the current queue snapshot.
#[tauri::command]
pub async fn get_queue_snapshot(state: State<'_, AppState>) -> Result<QueueSnapshot, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    queue::get_queue_snapshot(&db).map_err(|e| e.to_string())
}

/// Clear completed/failed/skipped items from the active queue run.
#[tauri::command]
pub async fn clear_completed_queue_items(
    state: State<'_, AppState>,
) -> Result<QueueSnapshot, String> {
    log::info!("[clear_completed_queue_items] Called");
    let db = state.db.lock().map_err(|e| e.to_string())?;
    queue::clear_completed_queue_items(&db).map_err(|e| e.to_string())
}

/// Dismiss/hide the active queue run.
#[tauri::command]
pub async fn dismiss_queue(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("[dismiss_queue] Called");
    let db = state.db.lock().map_err(|e| e.to_string())?;
    queue::dismiss_queue(&db).map_err(|e| e.to_string())
}

// ─── Legacy compatibility ───────────────────────────────────────────────────

/// Legacy: execute a queue of tasks. Now delegates to backend queue.
#[tauri::command]
pub async fn execute_queue(
    items: Vec<QueueItem>,
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    log::info!("[execute_queue] Legacy called with {} items, delegating to enqueue_tasks", items.len());

    let enqueue_items: Vec<EnqueueItem> = items
        .into_iter()
        .map(|item| EnqueueItem {
            task_id: item.task_id,
            project_id: item.project_id,
            title: Some(item.title),
            task_type: Some(item.task_type),
            project_name: item.project_name,
        })
        .collect();

    let snapshot = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        queue::enqueue_tasks(&db, enqueue_items, EnqueueMode::Append)
            .map_err(|e| e.to_string())?
    };

    let gsc_token = resolve_gsc_token_for_queue(&state, &gsc_state, &snapshot).await?;
    queue::ensure_runner_started(state.db_path.clone(), app_handle, gsc_token).await;

    Ok(())
}

/// Legacy: mark tasks as queued. Now a no-op because enqueue_tasks handles this.
#[tauri::command]
pub async fn mark_tasks_queued(_task_ids: Vec<String>) -> Result<(), String> {
    log::info!("[mark_tasks_queued] Legacy no-op, queue manages statuses internally");
    Ok(())
}

/// Legacy: reset queued tasks back to todo. Now delegates to remove_queue_item.
#[tauri::command]
pub async fn mark_tasks_todo(
    task_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("[mark_tasks_todo] Legacy called for {} tasks", task_ids.len());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    for task_id in task_ids {
        queue::remove_queue_item(&db, &task_id).ok();
    }
    Ok(())
}

/// Legacy: get queue state by status. Now delegates to get_queue_snapshot.
#[tauri::command]
pub async fn get_queue_state(state: State<'_, AppState>) -> Result<Vec<QueueItem>, String> {
    log::info!("[get_queue_state] Legacy called, delegating to get_queue_snapshot");
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let snapshot = queue::get_queue_snapshot(&db).map_err(|e| e.to_string())?;

    let legacy_items: Vec<QueueItem> = snapshot
        .items
        .into_iter()
        .map(|item| QueueItem {
            task_id: item.task_id,
            project_id: item.project_id,
            title: item.title.unwrap_or_else(|| "Untitled".to_string()),
            task_type: item.task_type.unwrap_or_default(),
            project_name: item.project_name,
            status: Some(item.status.as_str().to_string()),
            error: item.error,
        })
        .collect();

    Ok(legacy_items)
}

// ─── Helpers ────────────────────────────────────────────────────────────────

async fn resolve_gsc_token_for_queue(
    state: &State<'_, AppState>,
    gsc_state: &State<'_, GscState>,
    snapshot: &QueueSnapshot,
) -> Result<Option<String>, String> {
    let first_project_id = snapshot
        .items
        .first()
        .map(|i| i.project_id.as_str())
        .unwrap_or("");

    let project_path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        task_store::get_project(&db, first_project_id)
            .ok()
            .map(|p| p.path)
    };

    if let Some(path) = project_path {
        crate::commands::resolve_gsc_token(gsc_state, &path).await
    } else {
        Ok(None)
    }
}
