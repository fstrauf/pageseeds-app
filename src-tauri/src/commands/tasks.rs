use super::AppState;
use crate::engine::task_store;
use crate::models::task::{Priority, Task, TaskStatus};
use tauri::State;

#[tauri::command]
pub fn list_tasks(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
    phase: Option<String>,
) -> Result<Vec<Task>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    // Use the light variant to avoid loading large artifact blobs into memory
    // for list views that only need task metadata.
    Ok(task_store::list_tasks_filtered_light(
        &db,
        &project_id,
        status.as_deref(),
        phase.as_deref(),
    )?)
}

#[tauri::command]
pub fn get_task(state: State<'_, AppState>, id: String) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::get_task(&db, &id)?)
}

#[tauri::command]
pub fn create_task(
    state: State<'_, AppState>,
    project_id: String,
    task_type: String,
    title: Option<String>,
    description: Option<String>,
    priority: String,
    auto_enqueue: Option<bool>,
) -> Result<Task, String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::TaskRunPolicy;

    let priority_enum = match priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let run_policy = if auto_enqueue.unwrap_or(false) {
        Some(TaskRunPolicy::AutoEnqueue)
    } else {
        None
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(TaskSpawner::spawn(
        &db,
        TaskSpec {
            project_id,
            task_type,
            title,
            description,
            priority: priority_enum,
            run_policy,
            ..Default::default()
        },
    )?)
}

#[tauri::command]
pub fn update_task_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let status_enum = match status.as_str() {
        "in_progress" => TaskStatus::InProgress,
        "review" => TaskStatus::Review,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        "failed" => TaskStatus::Failed,
        _ => TaskStatus::Todo,
    };
    Ok(task_store::update_task_status(&db, &id, status_enum)?)
}

#[tauri::command]
pub fn update_task(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    description: Option<String>,
    priority: String,
) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let priority_enum = match priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };
    Ok(task_store::update_task(
        &db,
        &id,
        title.as_deref(),
        description.as_deref(),
        priority_enum,
    )?)
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::delete_task(&db, &id)?)
}

#[tauri::command]
pub fn cancel_task(state: State<'_, AppState>, id: String) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(crate::engine::post_actions::cancel_task(&db, &id)?)
}

/// Create content tasks from selected keywords and mark the research task as done.
/// Creates `create_landing_page` tasks for landing page research, `write_article` otherwise.
/// Each keyword string becomes both the task title and the target keyword in the description.
#[tauri::command]
pub fn create_article_tasks_from_keywords(
    state: State<'_, AppState>,
    project_id: String,
    research_task_id: String,
    keywords: Vec<String>,
) -> Result<Vec<Task>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::engine::keyword_selection::create_article_tasks_from_keywords(
        &db,
        &project_id,
        &research_task_id,
        keywords,
    )
}
