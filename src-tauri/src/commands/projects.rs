use super::AppState;
use crate::engine::task_store;
use crate::models::project::{Project, ProjectMode};
use tauri::State;

fn managed_project_root(project_id: &str) -> Result<std::path::PathBuf, String> {
    let db_path = crate::db::default_db_path();
    let app_dir = db_path
        .parent()
        .ok_or_else(|| "Could not resolve application data directory".to_string())?;
    let root = app_dir.join("managed_projects").join(project_id);
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("Failed to create managed project directory: {}", e))?;
    Ok(root)
}

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
    let project_mode = project_mode.unwrap_or_default();
    let resolved_path = match project_mode {
        ProjectMode::Workspace => path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Workspace projects require a repository path".to_string())?
            .to_string(),
        ProjectMode::LiveSite => {
            let site_url = site_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "Live site projects require a site URL".to_string())?;
            let root = managed_project_root(&id)?;
            log::info!(
                "[create_project] creating live-site project '{}' for {} in {:?}",
                id,
                site_url,
                root,
            );
            root.to_string_lossy().to_string()
        }
    };
    let normalized_content_dir = match project_mode {
        ProjectMode::Workspace => content_dir,
        ProjectMode::LiveSite => None,
    };

    let project = Project {
        id: id.clone(),
        name,
        path: resolved_path.clone(),
        content_dir: normalized_content_dir,
        site_url: site_url.clone(),
        site_id,
        sitemap_url,
        project_mode: project_mode.clone(),
        active: true,
        agent_provider: None,
        seo_provider: Some("ahrefs".to_string()),
        clarity_project_id,
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::create_project(&db, &project)?;

    // Seed default scheduler rules so AutoEnqueue tasks (collect_gsc, ctr_audit,
    // update_research_shortlist) actually run on a schedule.
    if let Err(e) = crate::engine::scheduler::seed_default_rules(&db, &id) {
        log::warn!("[create_project] Failed to seed scheduler rules: {}", e);
    }

    if project_mode == ProjectMode::Workspace {
        // Auto-initialize the project workspace with required files.
        let repo_root = std::path::Path::new(&resolved_path);
        if let Err(e) = crate::engine::setup_check::initialize_project_workspace(
            repo_root,
            site_url.as_deref(),
            Some(&name_for_init),
        ) {
            log::warn!(
                "[create_project] Failed to auto-initialize workspace: {}",
                e
            );
            // Don't fail project creation if initialization fails - user can fix manually
        }
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
