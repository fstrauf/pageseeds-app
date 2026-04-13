use tauri::State;
use crate::config::env_resolver::{EnvResolver, SecretsStatus};
use crate::db::global_settings;
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

/// Initialize a complete project workspace with all required files.
/// This is called automatically when a project is created or when
/// the user clicks "Initialize Project" from setup warnings.
/// 
/// Creates:
/// - .github/automation/ directory structure
/// - seo_workspace.json (with auto-discovered content_dir)
/// - articles.json (empty)
/// - project.md (template)
/// - reddit_config.md (template)
/// - reddit/_reply_guardrails.md (template)
/// - artifacts/, task_results/ directories
/// - Updates .gitignore
/// 
/// Returns a list of files that were created.
#[tauri::command]
pub fn initialize_project_workspace(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    
    let site_url_hint = project.site_url.as_deref();
    let project_name = Some(project.name.as_str());
    
    crate::engine::setup_check::initialize_project_workspace(repo_root, site_url_hint, project_name)
        .map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// GLOBAL AGENT PROVIDER SETTINGS
// ═══════════════════════════════════════════════════════════════════════════════

/// Check agent status using the GLOBAL agent provider setting.
/// This is the preferred way - agent provider is a user preference, not project-specific.
#[tauri::command]
pub async fn check_agent_status(
    state: State<'_, AppState>,
) -> Result<agent::AgentStatus, String> {
    let provider = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        global_settings::get_agent_provider(&db)
    };
    // Use async version with timeout to prevent UI blocking
    Ok(agent::detect_agents_async(&provider).await)
}

/// Set the GLOBAL agent provider.
/// This applies to ALL projects since it's a user tool preference.
#[tauri::command]
pub fn set_agent_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<String, String> {
    log::info!("[set_agent_provider] Setting global agent provider to '{}'", provider);
    
    let db = state.db.lock().map_err(|e| e.to_string())?;
    
    global_settings::set_agent_provider(&db, &provider)
        .map_err(|e| {
            log::error!("[set_agent_provider] Failed to save: {}", e);
            e.to_string()
        })?;
    
    log::info!("[set_agent_provider] Successfully set global agent provider to '{}'", provider);
    Ok(provider)
}

/// Get the global agent provider (for UI initialization).
#[tauri::command]
pub fn get_global_agent_provider(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(global_settings::get_agent_provider(&db))
}

/// Get all global settings (for debugging/admin).
#[tauri::command]
pub fn get_global_settings(
    state: State<'_, AppState>,
) -> Result<Vec<(String, String)>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    global_settings::get_all(&db).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// LEGACY: Per-project agent provider (deprecated, kept for backward compatibility)
// ═══════════════════════════════════════════════════════════════════════════════

/// DEPRECATED: Check agent status for a specific project.
/// Uses project's agent_provider if set, falls back to global.
/// 
/// This is kept for backward compatibility but new code should use check_agent_status().
#[tauri::command]
pub async fn check_agent_status_for_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<agent::AgentStatus, String> {
    let provider = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        // First check if project has a legacy agent_provider
        if let Ok(project) = task_store::get_project(&db, &project_id) {
            if let Some(project_provider) = project.agent_provider {
                project_provider
            } else {
                // Fall back to global
                global_settings::get_agent_provider(&db)
            }
        } else {
            // Project not found, use global
            global_settings::get_agent_provider(&db)
        }
    };
    Ok(agent::detect_agents_async(&provider).await)
}

/// Get the path to the application log file
#[tauri::command]
pub fn get_log_file_path() -> Result<String, String> {
    // Log directory is in the standard platform location
    let log_dir = dirs::data_local_dir()
        .ok_or("Could not determine log directory")?
        .join("com.pageseeds.app")
        .join("logs");
    
    // Log files are named pageseeds.log (the plugin handles rotation)
    let log_file = log_dir.join("pageseeds.log");
    
    Ok(log_file.to_string_lossy().to_string())
}
