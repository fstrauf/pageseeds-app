use std::sync::Arc;
use tauri::State;
use crate::config::env_resolver::EnvResolver;
use crate::engine::{executor, task_store};
use crate::models::reddit::{
    MigrationResult, RedditOpportunity, RedditStats, SubmissionSummary, ValidationResult,
};
use crate::reddit;
use super::{AppState, GscState};

#[tauri::command]
pub async fn search_reddit(
    query: String,
    subreddit: String,
    limit: Option<i32>,
    sort: Option<String>,
    time_filter: Option<String>,
) -> Result<Vec<SubmissionSummary>, String> {
    reddit::search::search_submissions(
        &query,
        &subreddit,
        limit.unwrap_or(25),
        sort.as_deref().unwrap_or("relevance"),
        time_filter.as_deref().unwrap_or("all"),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_reddit_opportunities(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
) -> Result<Vec<RedditOpportunity>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    reddit::db::list_opportunities(&db, &project_id, status.as_deref())
        .map_err(|e| e.to_string())
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
            let manager = crate::reddit::history::RedditHistoryManager::new(
                std::path::Path::new(&project.path)
            );
            if let Err(e) = manager.mark_posted(&post_id) {
                log::warn!("[history] failed to write posted history for {}: {}", post_id, e);
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
            let manager = crate::reddit::history::RedditHistoryManager::new(
                std::path::Path::new(&project.path)
            );
            if let Err(e) = manager.mark_skipped(&post_id) {
                log::warn!("[history] failed to write skipped history for {}: {}", post_id, e);
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
        .ok_or_else(|| "REDDIT_CLIENT_ID not set — add it to ~/.config/automation/secrets.env".to_string())?;
    let client_secret = resolver
        .resolve("REDDIT_CLIENT_SECRET")
        .map(|(v, _)| v)
        .ok_or_else(|| "REDDIT_CLIENT_SECRET not set — add it to ~/.config/automation/secrets.env".to_string())?;
    let refresh_token = resolver
        .resolve("REDDIT_REFRESH_TOKEN")
        .map(|(v, _)| v)
        .ok_or_else(|| "REDDIT_REFRESH_TOKEN not set — add it to ~/.config/automation/secrets.env".to_string())?;

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
    reddit::db::mark_posted(&db, &post_id, &reply_text, &reply_url)
        .map_err(|e| e.to_string())?;

    if !project_id.is_empty() {
        if let Ok(project) = task_store::get_project(&db, &project_id) {
            let manager = crate::reddit::history::RedditHistoryManager::new(
                std::path::Path::new(&project.path),
            );
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
pub async fn run_reddit_opportunity_search(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    project_id: String,
    user_context: Option<String>,
) -> Result<executor::ExecutionResult, String> {
    use crate::config::{default_execution_mode, default_phase};
    use crate::models::task::{AgentPolicy, Priority, TaskStatus};
    use crate::reddit::config as reddit_cfg;
    use std::path::Path;

    let project_path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        project.path.clone()
    };
    let automation_dir = Path::new(&project_path).join(".github").join("automation");
    let missing = reddit_cfg::missing_config_files(&automation_dir);
    if !missing.is_empty() {
        return Err(format!(
            "Missing required config files: {}. Create them in .github/automation/ first.",
            missing.join(", ")
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let id = format!("task-{}", chrono::Utc::now().timestamp_millis());

    let description = user_context.as_ref().filter(|s| !s.trim().is_empty()).map(|ctx| {
        serde_json::json!({ "user_context": ctx }).to_string()
    });

    let task = crate::models::task::Task {
        id,
        phase: default_phase("reddit_opportunity_search").to_string(),
        execution_mode: default_execution_mode("reddit_opportunity_search"),
        task_type: "reddit_opportunity_search".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        agent_policy: AgentPolicy::Optional,
        title: Some("Reddit Opportunity Search".to_string()),
        description,
        project_id,
        depends_on: vec![],
        artifacts: vec![],
        run: crate::models::task::TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
    };

    let task_id = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        task_store::create_task(&db, &task)
            .map_err(|e| e.to_string())?
            .id
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
        executor::execute_task_with_token(&db, &task_id, token.as_deref(), None, false)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn draft_reddit_reply(
    state: State<'_, AppState>,
    project_id: String,
    post_id: String,
) -> Result<String, String> {
    use crate::engine::{agent as agent_mod, skills};
    use crate::reddit::config as reddit_cfg;
    use std::path::Path;

    let (project_path, agent_provider, opp) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let provider = project.agent_provider.clone().unwrap_or_else(|| "copilot".to_string());
        let opp = crate::reddit::db::get_opportunity(&db, &post_id)
            .map_err(|e| e.to_string())?;
        (project.path.clone(), provider, opp)
    };

    let repo_root = Path::new(&project_path);
    let automation_dir = repo_root.join(".github").join("automation");

    let missing = reddit_cfg::missing_config_files(&automation_dir);
    if !missing.is_empty() {
        return Err(format!(
            "Missing required config files: {}. Create them in .github/automation/ first.",
            missing.join(", ")
        ));
    }

    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .map_err(|e| format!("Failed to read project_summary.md: {}", e))?;
    let reddit_config_raw = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .map_err(|e| format!("Failed to read reddit_config.md: {}", e))?;
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .map_err(|e| format!("Failed to read brandvoice.md: {}", e))?;
    let guardrails = std::fs::read_to_string(
        automation_dir.join("reddit").join("_reply_guardrails.md")
    ).map_err(|e| format!("Failed to read _reply_guardrails.md: {}", e))?;

    let skill_content = skills::load_skill(repo_root, "reddit-reply-drafting")
        .map(|s| s.content)
        .unwrap_or_default();

    let cfg = reddit_cfg::parse_reddit_config(&reddit_config_raw);
    let prompt = crate::reddit::prompts::build_draft_reply_prompt(
        &project_summary,
        &brandvoice,
        &guardrails,
        &skill_content,
        &cfg,
        &opp,
    );

    let reply_text = agent_mod::run_agent(&agent_provider, &prompt, repo_root)
        .map_err(|e| format!("Agent failed: {}", e))?;

    let reply_text = reply_text.trim().to_string();

    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "UPDATE reddit_opportunities SET reply_text = ?1, updated_at = ?2 WHERE post_id = ?3",
            rusqlite::params![reply_text, now, post_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(reply_text)
}

#[tauri::command]
pub fn validate_reddit_reply(
    state: State<'_, AppState>,
    text: String,
    project_id: Option<String>,
) -> ValidationResult {
    let base = validate_reply(&text);
    if !base.valid {
        return base;
    }

    if let Some(pid) = project_id.filter(|s| !s.is_empty()) {
        if let Ok(db) = state.db.lock() {
            if let Ok(project) = task_store::get_project(&db, &pid) {
                let automation_dir = std::path::Path::new(&project.path)
                    .join(".github")
                    .join("automation");
                if let Ok(cfg) = crate::reddit::config::load_reddit_config(&automation_dir) {
                    if cfg.mention_stance == crate::reddit::config::MentionStance::Required {
                        if let Some(product) = &cfg.product_name {
                            if !text.to_lowercase().contains(&product.to_lowercase()) {
                                return ValidationResult {
                                    valid: false,
                                    error: Some(format!(
                                        "Reply must mention \"{}\" by name (mention stance: REQUIRED).",
                                        product
                                    )),
                                };
                            }
                        }
                    }
                }
            }
        }
    }

    ValidationResult { valid: true, error: None }
}

fn validate_reply(text: &str) -> ValidationResult {
    let text = text.trim();

    if text.len() < 10 {
        return ValidationResult { valid: false, error: Some("Reply is too short (minimum 10 characters).".to_string()) };
    }
    if text.contains("http://") || text.contains("https://") {
        return ValidationResult { valid: false, error: Some("Reply must not contain URLs.".to_string()) };
    }
    if regex::Regex::new(r"\[.+?\]\(.+?\)").unwrap().is_match(text) {
        return ValidationResult { valid: false, error: Some("Reply must not contain markdown links.".to_string()) };
    }
    let sentences: Vec<&str> = text.split(['.', '!', '?']).filter(|s| !s.trim().is_empty()).collect();
    if sentences.len() < 3 {
        return ValidationResult { valid: false, error: Some(format!("{} sentence(s) — minimum 3 required.", sentences.len())) };
    }
    if sentences.len() > 5 {
        return ValidationResult { valid: false, error: Some(format!("{} sentences — maximum 5 allowed.", sentences.len())) };
    }
    let word_count = text.split_whitespace().count();
    if word_count < 30 {
        return ValidationResult { valid: false, error: Some(format!("{} words — minimum 30 recommended.", word_count)) };
    }
    if word_count > 250 {
        return ValidationResult { valid: false, error: Some(format!("{} words — maximum 250 recommended.", word_count)) };
    }
    ValidationResult { valid: true, error: None }
}

#[tauri::command]
pub async fn enrich_reddit_opportunities(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    let (project_path, agent_provider) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let provider = project.agent_provider.clone().unwrap_or_else(|| "copilot".to_string());
        (project.path.clone(), provider)
    };

    let db_arc = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        crate::engine::exec::reddit::exec_reddit_enrich(&db, &project_id, &project_path, &agent_provider);
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
    use crate::models::task::{Task, TaskRun, TaskStatus, Priority, ExecutionMode, AgentPolicy};
    use crate::models::reddit::RedditOpportunity;
    use chrono::Utc;

    if post_ids.is_empty() {
        return Err("No opportunities selected".to_string());
    }

    let db = state.db.lock().map_err(|e| e.to_string())?;
    
    // Get the parent task
    let parent_task = task_store::get_task(&db, &task_id)
        .map_err(|e| format!("Failed to get parent task: {}", e))?;
    
    // Find the reddit_results_stage artifact
    let results_artifact = parent_task.artifacts.iter()
        .find(|a| a.key == "reddit_results_stage")
        .ok_or_else(|| "No reddit_results_stage artifact found. Run the search first.".to_string())?;
    
    let artifact_content = results_artifact.content.as_ref()
        .ok_or_else(|| "reddit_results_stage artifact has no content".to_string())?;
    
    // Parse opportunities from JSON
    let opportunities: Vec<RedditOpportunity> = serde_json::from_str(artifact_content)
        .map_err(|e| format!("Failed to parse opportunities: {}", e))?;
    
    // Filter to only selected post_ids
    let selected_opps: Vec<RedditOpportunity> = opportunities.into_iter()
        .filter(|o| post_ids.contains(&o.post_id))
        .collect();
    
    if selected_opps.is_empty() {
        return Err("None of the selected post IDs were found in the search results".to_string());
    }
    
    // Create a task for each selected opportunity
    let mut created_tasks = Vec::new();
    let now = Utc::now().to_rfc3339();
    
    for (idx, opp) in selected_opps.iter().enumerate() {
        let task_id = format!("task-{}", Utc::now().timestamp_millis() + idx as i64);
        
        // Determine priority based on severity
        let priority = match opp.severity.as_deref() {
            Some("CRITICAL") | Some("HIGH") => Priority::High,
            _ => Priority::Medium,
        };
        
        let title = format!(
            "Reply to: {}",
            opp.title.as_deref().unwrap_or("Reddit post").chars().take(50).collect::<String>()
        );
        
        let description = format!(
            "**Subreddit:** r/{}\n\n**Post URL:** {}\n\n**Why Relevant:** {}\n\n**Draft Reply:**\n{}\n\n**Post ID:** {}",
            opp.subreddit.as_deref().unwrap_or("unknown"),
            opp.url.as_deref().unwrap_or(""),
            opp.why_relevant.as_deref().unwrap_or(""),
            opp.reply_text.as_deref().unwrap_or("(no draft reply)"),
            opp.post_id
        );
        
        let task = Task {
            id: task_id,
            project_id: parent_task.project_id.clone(),
            task_type: "reddit_reply".to_string(),
            phase: "engagement".to_string(),
            status: TaskStatus::Todo,
            priority,
            execution_mode: ExecutionMode::Manual,
            agent_policy: AgentPolicy::Optional,
            title: Some(title),
            description: Some(description),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None },
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        
        task_store::create_task(&db, &task)
            .map_err(|e| format!("Failed to create task: {}", e))?;
        
        created_tasks.push(task);
    }
    
    log::info!("[create_reddit_reply_tasks] created {} reply tasks from parent {}", 
        created_tasks.len(), parent_task.id);
    
    Ok(created_tasks)
}
