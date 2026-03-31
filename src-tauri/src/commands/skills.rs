use std::sync::Arc;
use tauri::State;
use crate::engine::{executor, normalizer, prompts, skills, skills_search, task_store};
use super::{AppState, GscState};

#[tauri::command]
pub fn list_skills(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<skills::Skill>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    Ok(skills::scan_skills(std::path::Path::new(&project.path)))
}

#[tauri::command]
pub fn get_skill(
    state: State<'_, AppState>,
    project_id: String,
    skill_name: String,
) -> Result<skills::Skill, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    skills::load_skill(std::path::Path::new(&project.path), &skill_name)
        .ok_or_else(|| format!("Skill '{}' not found", skill_name))
}

#[tauri::command]
pub fn build_prompt_preview(
    state: State<'_, AppState>,
    task_id: String,
    skill_name: String,
) -> Result<prompts::PromptContext, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let task = task_store::get_task(&db, &task_id).map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &task.project_id).map_err(|e| e.to_string())?;
    let skill = skills::load_skill(std::path::Path::new(&project.path), &skill_name)
        .ok_or_else(|| format!("Skill '{}' not found", skill_name))?;
    Ok(prompts::build_prompt(
        &task,
        &skill,
        &project.path,
        project.site_url.as_deref(),
    ))
}

#[tauri::command]
pub fn normalize_output(raw: String) -> normalizer::NormalizedArtifact {
    normalizer::normalize_agent_output(&raw)
}

#[tauri::command]
pub fn list_task_artifacts(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<crate::models::task::TaskArtifact>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let task = task_store::get_task(&db, &task_id).map_err(|e| e.to_string())?;
    Ok(task.artifacts)
}

#[tauri::command]
pub fn get_project_overview(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<task_store::ProjectOverview, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::get_project_overview(&db, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn quick_run_workflow(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    project_id: String,
    task_type: String,
    title: String,
    themes: Option<Vec<String>>,
) -> Result<executor::ExecutionResult, String> {
    use crate::config::{default_execution_mode, default_phase};
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, TaskStatus};

    let task_id = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        match task_store::find_active_task_by_type(&db, &project_id, &task_type)
            .map_err(|e| e.to_string())?
        {
            Some(existing) if existing.status == TaskStatus::InProgress => {
                return Err(format!(
                    "Task '{}' is already running ({})",
                    task_type, existing.id
                ));
            }
            Some(existing) => {
                task_store::reset_task_error(&db, &existing.id).map_err(|e| e.to_string())?;
                existing.id
            }
            None => {
                let now = chrono::Utc::now().to_rfc3339();
                let id = format!("task-{}", chrono::Utc::now().timestamp_millis());

                let description: Option<String> = themes.as_ref().filter(|t| !t.is_empty()).map(|t| {
                    serde_json::json!({ "themes": t }).to_string()
                });

                let task = crate::models::task::Task {
                    id,
                    phase: default_phase(&task_type).to_string(),
                    execution_mode: default_execution_mode(&task_type),
                    task_type: task_type.clone(),
                    status: TaskStatus::Todo,
                    priority: Priority::High,
                    agent_policy: AgentPolicy::Optional,
                    title: Some(title),
                    description,
                    project_id,
                    depends_on: vec![],
                    artifacts: vec![],
                    run: crate::models::task::TaskRun::default(),
                    created_at: now.clone(),
                    updated_at: now,
                };

                task_store::create_task(&db, &task)
                    .map_err(|e| e.to_string())?
                    .id
            }
        }
    };

    let token = gsc_state
        .token
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .filter(|t| !t.is_expired())
        .map(|t| t.access_token.clone());

    let db_arc = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        
        // Create a new runtime to run the async executor
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        rt.block_on(async {
            executor::execute_task_with_token(&db, &task_id, token.as_deref(), None, false).await
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Vector Search Commands ───────────────────────────────────────────────────

/// Check if Ollama is available for skill embeddings.
#[tauri::command]
pub async fn check_embedding_status() -> Result<skills_search::EmbeddingStatus, String> {
    Ok(skills_search::check_status().await)
}

/// Index all skills for semantic search.
/// Returns the number of skills that were indexed (or re-indexed due to changes).
#[tauri::command]
pub async fn index_skills(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<usize, String> {
    let skills = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        skills::scan_skills(std::path::Path::new(&project.path))
    };

    let db_arc = Arc::clone(&state.db);
    tokio::task::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        skills_search::index_skills_blocking(&db, &project_id, &skills).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Search skills by semantic similarity.
/// Returns skills sorted by relevance to the query.
#[tauri::command]
pub async fn search_skills(
    state: State<'_, AppState>,
    project_id: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<skills_search::ScoredSkill>, String> {
    let all_skills = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        skills::scan_skills(std::path::Path::new(&project.path))
    };

    let db_arc = Arc::clone(&state.db);
    let limit = limit.unwrap_or(5);
    tokio::task::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        skills_search::search_skills_blocking(&db, &project_id, &query, limit, &all_skills)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
