use tauri::State;
use crate::engine::task_store;
use crate::models::project::Project;
use super::AppState;

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::list_projects(&db)?)
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

    // Clone name before moving it into project
    let name_for_init = name.clone();

    let project = Project {
        id: id.clone(),
        name,
        path: path.clone(),
        content_dir,
        site_url: site_url.clone(),
        site_id: None,
        active: true,
        agent_provider: None,
        seo_provider: Some("ahrefs".to_string()),
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::create_project(&db, &project)?;
    
    // Auto-initialize the project workspace with required files
    // This creates .github/automation/, seo_workspace.json, articles.json, etc.
    let repo_root = std::path::Path::new(&path);
    if let Err(e) = crate::engine::setup_check::initialize_project_workspace(
        repo_root, 
        site_url.as_deref(),
        Some(&name_for_init)
    ) {
        log::warn!("[create_project] Failed to auto-initialize workspace: {}", e);
        // Don't fail project creation if initialization fails - user can fix manually
    }
    
    Ok(project)
}

#[tauri::command]
pub fn update_project(state: State<'_, AppState>, project: Project) -> Result<Project, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::update_project(&db, &project)?)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::delete_project(&db, &id)?)
}
