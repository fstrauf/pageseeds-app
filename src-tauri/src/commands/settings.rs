use tauri::State;
use crate::config::env_resolver::{EnvResolver, SecretsStatus};
use crate::engine::{agent, task_store};
use super::AppState;

#[tauri::command]
pub fn get_secrets_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<SecretsStatus, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let resolver = EnvResolver::new(&project.path);
    Ok(resolver.secrets_status())
}

#[tauri::command]
pub fn get_secrets_file_path() -> String {
    crate::config::env_resolver::secrets_env_path()
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub fn import_env_file(source_path: String) -> Result<Vec<String>, String> {
    crate::config::env_resolver::import_from_env_file(
        std::path::Path::new(&source_path)
    )
}

#[tauri::command]
pub fn check_project_setup(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::engine::setup_check::ProjectSetup, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    Ok(crate::engine::setup_check::resolve(
        &project_id,
        &project.path,
        project.content_dir.as_deref(),
    ))
}

#[tauri::command]
pub fn get_project_config_files_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<crate::engine::setup_check::ProjectConfigFileStatus>, String> {
    let project = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?
    };
    Ok(crate::engine::setup_check::collect_config_file_statuses(
        &project.path,
        project.content_dir.as_deref(),
    ))
}

#[tauri::command]
pub fn init_workspace_config(
    state: State<'_, AppState>,
    project_id: String,
    content_dir: String,
    site_url: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let automation_dir = std::path::Path::new(&project.path)
        .join(".github")
        .join("automation");
    let path = crate::engine::setup_check::write_workspace_config(
        &automation_dir,
        &content_dir,
        &site_url,
    )?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn check_agent_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<agent::AgentStatus, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let provider = project.agent_provider.as_deref().unwrap_or("copilot");
    Ok(agent::detect_agents(provider))
}

#[tauri::command]
pub fn set_agent_provider(
    state: State<'_, AppState>,
    project_id: String,
    provider: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "UPDATE projects SET agent_provider = ?1 WHERE id = ?2",
        rusqlite::params![provider, project_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
