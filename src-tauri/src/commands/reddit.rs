use super::AppState;
use crate::config::env_resolver::EnvResolver;
use crate::engine::task_store;
use crate::models::reddit::{
    MigrationResult, RedditOpportunity, RedditStats, SubmissionSummary, ValidationResult,
};
use crate::reddit;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn search_reddit(
    query: String,
    subreddit: String,
    limit: Option<i32>,
    sort: Option<String>,
    time_filter: Option<String>,
) -> Result<Vec<SubmissionSummary>, String> {
    let result = reddit::search::search_submissions(
        &query,
        &subreddit,
        limit.unwrap_or(25),
        sort.as_deref().unwrap_or("relevance"),
        time_filter.as_deref().unwrap_or("all"),
        0,    // no delay for manual single searches
        None, // manual search uses public API; OAuth not needed for single queries
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(result.posts)
}

#[tauri::command]
pub fn list_reddit_opportunities(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
) -> Result<Vec<RedditOpportunity>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::list_opportunities(&db, &project_id, status.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_reddit_opportunity(
    state: State<'_, AppState>,
    opportunity: RedditOpportunity,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::upsert_opportunity(&db, &opportunity).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_reddit_posted(
    state: State<'_, AppState>,
    post_id: String,
    reply_text: String,
    reply_url: String,
    project_id: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::mark_posted(&db, &post_id, &reply_text, &reply_url).map_err(|e| e.to_string())?;

    if let Some(pid) = project_id.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(project) = task_store::get_project(&db, pid) {
            let manager = crate::reddit::history::RedditHistoryManager::new(std::path::Path::new(
                &project.path,
            ));
            if let Err(e) = manager.mark_posted(&post_id) {
                log::warn!(
                    "[history] failed to write posted history for {}: {}",
                    post_id,
                    e
                );
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn mark_reddit_skipped(
    state: State<'_, AppState>,
    post_id: String,
    project_id: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::mark_skipped(&db, &post_id).map_err(|e| e.to_string())?;

    if let Some(pid) = project_id.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(project) = task_store::get_project(&db, pid) {
            let manager = crate::reddit::history::RedditHistoryManager::new(std::path::Path::new(
                &project.path,
            ));
            if let Err(e) = manager.mark_skipped(&post_id) {
                log::warn!(
                    "[history] failed to write skipped history for {}: {}",
                    post_id,
                    e
                );
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn post_to_reddit(
    state: State<'_, AppState>,
    post_id: String,
    reply_text: String,
    project_id: String,
) -> Result<String, String> {
    let project_path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        project.path.clone()
    };
    let resolver = EnvResolver::new(&project_path);

    let client_id = resolver
        .resolve("REDDIT_CLIENT_ID")
        .map(|(v, _)| v)
        .ok_or_else(|| {
            "REDDIT_CLIENT_ID not set — add it to ~/.config/automation/secrets.env".to_string()
        })?;
    let client_secret = resolver
        .resolve("REDDIT_CLIENT_SECRET")
        .map(|(v, _)| v)
        .ok_or_else(|| {
            "REDDIT_CLIENT_SECRET not set — add it to ~/.config/automation/secrets.env".to_string()
        })?;
    let refresh_token = resolver
        .resolve("REDDIT_REFRESH_TOKEN")
        .map(|(v, _)| v)
        .ok_or_else(|| {
            "REDDIT_REFRESH_TOKEN not set — add it to ~/.config/automation/secrets.env".to_string()
        })?;

    let result = crate::reddit::post::submit_comment(
        &post_id,
        &reply_text,
        &client_id,
        &client_secret,
        &refresh_token,
    )
    .await
    .map_err(|e| e.to_string())?;

    let reply_url = result.permalink;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::mark_posted(&db, &post_id, &reply_text, &reply_url).map_err(|e| e.to_string())?;

    if !project_id.is_empty() {
        if let Ok(project) = task_store::get_project(&db, &project_id) {
            let manager = crate::reddit::history::RedditHistoryManager::new(std::path::Path::new(
                &project.path,
            ));
            if let Err(e) = manager.mark_posted(&post_id) {
                log::warn!("[post_to_reddit] history write failed: {}", e);
            }
        }
    }

    Ok(reply_url)
}

#[tauri::command]
pub fn get_reddit_statistics(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<RedditStats, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::get_statistics(&db, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn draft_reddit_reply(
    state: State<'_, AppState>,
    project_id: String,
    post_id: String,
) -> Result<String, String> {
    use crate::db::global_settings;

    let (project_path, agent_provider, opp) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let provider =
            global_settings::resolve_agent_provider(&db, project.agent_provider.as_deref());
        let opp = crate::reddit::db::get_opportunity(&db, &post_id).map_err(|e| e.to_string())?;
        (project.path.clone(), provider, opp)
    };

    let reply_text =
        crate::reddit::draft::generate_draft_reply(&project_path, &agent_provider, &opp).await?;

    // Persist the generated reply
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "UPDATE reddit_opportunities SET reply_text = ?1, updated_at = ?2 WHERE post_id = ?3",
            rusqlite::params![reply_text, now, post_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(reply_text)
}

#[tauri::command]
pub fn validate_reddit_reply(
    state: State<'_, AppState>,
    text: String,
    project_id: Option<String>,
) -> ValidationResult {
    let base = crate::reddit::validation::validate_reply(&text);
    if !base.valid {
        return base;
    }

    if let Some(pid) = project_id.filter(|s| !s.is_empty()) {
        if let Ok(db) = state.db.lock() {
            if let Ok(project) = task_store::get_project(&db, &pid) {
                let automation_dir = std::path::Path::new(&project.path)
                    .join(".github")
                    .join("automation");
                let stance_check =
                    crate::reddit::validation::validate_project_stance(&text, &automation_dir);
                if !stance_check.valid {
                    return stance_check;
                }
            }
        }
    }

    ValidationResult {
        valid: true,
        error: None,
    }
}

#[tauri::command]
pub async fn enrich_reddit_opportunities(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    use crate::db::global_settings;
    use crate::models::task::{AgentPolicy, TaskRunPolicy, Priority, Task, TaskRun, TaskStatus, TaskReviewSurface, FollowUpPolicy};
    use chrono::Utc;

    let (project_path, agent_provider) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let provider =
            global_settings::resolve_agent_provider(&db, project.agent_provider.as_deref());
        (project.path.clone(), provider)
    };

    // Create a synthetic task for enrichment (no artifact - will use fallback parsing)
    let synthetic_task = Task {
        id: format!("enrich-{}", Utc::now().timestamp_millis()),
        project_id: project_id.clone(),
        task_type: "reddit_enrich".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::InProgress,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: crate::models::task::TaskReviewSurface::None,
        follow_up_policy: crate::models::task::FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some("Reddit Enrichment".to_string()),
        description: None,
        depends_on: vec![],
        artifacts: vec![], // Empty - will fall back to deterministic parsing
        run: TaskRun {
            attempts: 0,
            last_error: None,
            provider: None,
            ..Default::default()
        },
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };

    let db_arc = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        crate::engine::exec::reddit::exec_reddit_enrich(
            &db,
            &synthetic_task,
            &project_path,
            &agent_provider,
        );
        Ok::<String, String>("Enrichment complete".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn migrate_reddit_db(
    state: State<'_, AppState>,
    project_id: String,
    source_path: String,
) -> Result<MigrationResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let path = std::path::PathBuf::from(&source_path);
    if !path.exists() {
        return Err(format!("File not found: {}", source_path));
    }
    reddit::db::migrate_from_client_ops(&db, &project_id, &path).map_err(|e| e.to_string())
}

/// Create reddit_reply tasks from selected opportunities in a completed search task.
#[tauri::command]
pub fn create_reddit_reply_tasks(
    state: State<'_, AppState>,
    task_id: String,
    post_ids: Vec<String>,
) -> Result<Vec<crate::models::task::Task>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::reddit::spawner::create_reply_tasks_from_opportunities(&db, &task_id, &post_ids)
}
