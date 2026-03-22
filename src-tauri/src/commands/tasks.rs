use tauri::State;
use crate::engine::task_store;
use crate::models::task::Task;
use super::AppState;

#[tauri::command]
pub fn list_tasks(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
    phase: Option<String>,
) -> Result<Vec<Task>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::list_tasks_filtered(&db, &project_id, status.as_deref(), phase.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_task(state: State<'_, AppState>, id: String) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::get_task(&db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_task(
    state: State<'_, AppState>,
    project_id: String,
    task_type: String,
    title: Option<String>,
    priority: String,
) -> Result<Task, String> {
    use crate::config::{default_execution_mode, default_phase};

    let now = chrono::Utc::now().to_rfc3339();
    let id = format!(
        "task-{}",
        chrono::Utc::now().timestamp_millis().to_string()
    );

    let task = Task {
        id,
        phase: default_phase(&task_type).to_string(),
        execution_mode: default_execution_mode(&task_type).to_string(),
        task_type,
        status: "todo".to_string(),
        priority,
        agent_policy: "none".to_string(),
        title,
        description: None,
        project_id,
        depends_on: vec![],
        artifacts: vec![],
        run: crate::models::task::TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::create_task(&db, &task).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_task_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::update_task_status(&db, &id, &status).map_err(|e| e.to_string())
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
    task_store::update_task(&db, &id, title.as_deref(), description.as_deref(), &priority)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::delete_task(&db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_task(state: State<'_, AppState>, id: String) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::update_task_status(&db, &id, "cancelled").map_err(|e| e.to_string())
}
