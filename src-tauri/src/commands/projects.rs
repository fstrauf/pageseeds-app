use super::AppState;
use crate::engine::project_create::{create_or_link_project, CreateProjectParams};
use crate::engine::task_store;
use crate::models::project::{Project, ProjectMode};
use tauri::State;

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::list_projects(&db)?)
}

#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    name: String,
    path: Option<String>,
    content_dir: Option<String>,
    site_url: Option<String>,
    site_id: Option<String>,
    sitemap_url: Option<String>,
    project_mode: Option<ProjectMode>,
    clarity_project_id: Option<String>,
) -> Result<Project, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let outcome = create_or_link_project(
        &db,
        CreateProjectParams {
            name,
            path,
            content_dir,
            site_url,
            site_id,
            sitemap_url,
            project_mode: project_mode.unwrap_or_default(),
            clarity_project_id,
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(outcome.project)
}

#[tauri::command]
pub fn update_project(state: State<'_, AppState>, project: Project) -> Result<Project, String> {
    // Fail fast on un-fetchable site_url values (same contract as create_project).
    if let Some(value) = project.site_url.as_deref() {
        crate::models::project::validate_site_url(value)?;
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::update_project(&db, &project)?)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::delete_project(&db, &id)?)
}
