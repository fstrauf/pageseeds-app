use super::AppState;
use crate::engine::task_store;
use crate::models::task::{AgentPolicy, Priority, Task, TaskStatus};
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
) -> Result<Task, String> {
    use crate::config::{default_execution_mode, default_phase};

    let now = chrono::Utc::now().to_rfc3339();
    let id = format!("task-{}", chrono::Utc::now().timestamp_millis().to_string());
    let priority_enum = match priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let task = Task {
        id,
        phase: default_phase(&task_type).to_string(),
        execution_mode: default_execution_mode(&task_type),
        task_type,
        status: TaskStatus::Todo,
        priority: priority_enum,
        agent_policy: AgentPolicy::None,
        title,
        description,
        project_id,
        depends_on: vec![],
        artifacts: vec![],
        run: crate::models::task::TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::create_task(&db, &task)?)
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
    Ok(task_store::update_task_status(
        &db,
        &id,
        TaskStatus::Cancelled,
    )?)
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
    use crate::engine::keyword_selection;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    let research_task = task_store::get_task(&db, &research_task_id)?;

    let tasks = keyword_selection::build_content_tasks_from_keywords(
        keywords,
        &research_task,
        &research_task_id,
        &project_id,
    )?;

    for task in &tasks {
        task_store::create_task(&db, task)?;
    }

    // Mark the research task done now that keywords have been dispatched.
    task_store::update_task_status(&db, &research_task_id, TaskStatus::Done)?;

    Ok(tasks)
}
