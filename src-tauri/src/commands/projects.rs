use tauri::State;
use crate::engine::task_store;
use crate::models::project::Project;
use super::AppState;

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::list_projects(&db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    name: String,
    path: String,
    content_dir: Option<String>,
    site_url: Option<String>,
) -> Result<Project, String> {
    let id = name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "_")
        .trim_matches('_')
        .to_string();
    let id = if id.is_empty() {
        format!("project_{}", chrono::Utc::now().timestamp())
    } else {
        id
    };

    let project = Project {
        id,
        name,
        path,
        content_dir,
        site_url,
        site_id: None,
        active: true,
        agent_provider: None,
        seo_provider: Some("ahrefs".to_string()),
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::create_project(&db, &project).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_project(state: State<'_, AppState>, project: Project) -> Result<Project, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::update_project(&db, &project).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::delete_project(&db, &id).map_err(|e| e.to_string())
}
