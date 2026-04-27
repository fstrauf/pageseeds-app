/// Cannibalization strategy review and approval commands.
///
/// Thin IPC wrappers — all business logic lives in the engine modules.

use tauri::State;

use crate::commands::AppState;
use crate::engine::project_paths::ProjectPaths;
use crate::models::cannibalization::{
    ApprovalStatus, CannibalizationStrategy, StrategyReview, StrategyWithReviews,
};
use crate::models::task::{ExecutionMode, Priority, TaskArtifact};

/// Read the cannibalization strategy for a project.
///
/// Tries the latest `cannibalization_audit` task's `cannibalization_strategy` artifact first,
/// then falls back to reading `cannibalization_strategy.json` from the automation directory.
#[tauri::command]
pub fn get_cannibalization_strategy(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<StrategyWithReviews>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Find the most recent cannibalization_audit task for this project
    let mut stmt = db
        .prepare(
            "SELECT id FROM tasks
             WHERE project_id = ?1 AND type = 'cannibalization_audit'
             ORDER BY created_at DESC LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let task_id: Result<Option<String>, _> = stmt
        .query_row([&project_id], |row| row.get(0))
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        });

    let task_id = task_id.map_err(|e| e.to_string())?;

    // Try to get strategy JSON from task artifact
    let strategy_json = if let Some(tid) = &task_id {
        let task = crate::engine::task_store::get_task(&db, tid).map_err(|e| e.to_string())?;
        task.artifacts
            .iter()
            .find(|a| a.key == "cannibalization_strategy")
            .and_then(|a| a.content.clone())
    } else {
        None
    };

    // Fallback: read from automation dir
    let strategy_json = match strategy_json {
        Some(json) => json,
        None => {
            let project = crate::engine::task_store::get_project(&db, &project_id)
                .map_err(|e| e.to_string())?;
            let paths = ProjectPaths::from_project(&project);
            let path = paths.automation_dir.join("cannibalization_strategy.json");
            std::fs::read_to_string(&path).unwrap_or_default()
        }
    };

    if strategy_json.is_empty() {
        return Ok(None);
    }

    let strategy: CannibalizationStrategy =
        serde_json::from_str(&strategy_json).map_err(|e| format!("Invalid strategy JSON: {}", e))?;

    let strategy_id = task_id.unwrap_or_else(|| {
        // Use a synthetic strategy ID based on project + timestamp of file
        format!("strategy-{}-file", project_id)
    });

    let reviews = crate::db::list_strategy_reviews(&db, &strategy_id).map_err(|e| e.to_string())?;

    Ok(Some(StrategyWithReviews {
        strategy,
        reviews,
        strategy_id,
        project_id,
    }))
}

/// Approve or reject a single recommendation.
#[tauri::command]
pub fn set_recommendation_approval(
    state: State<'_, AppState>,
    strategy_id: String,
    project_id: String,
    recommendation_type: String,
    recommendation_id: String,
    status: String,
    notes: Option<String>,
) -> Result<StrategyReview, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    let status_enum = match status.as_str() {
        "approved" => ApprovalStatus::Approved,
        "rejected" => ApprovalStatus::Rejected,
        "needs_review" => ApprovalStatus::NeedsReview,
        _ => ApprovalStatus::Pending,
    };

    crate::db::set_strategy_review(
        &db,
        &strategy_id,
        &project_id,
        &recommendation_type,
        &recommendation_id,
        status_enum,
        None, // approved_by — could be extended with user identity
        notes.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Get all reviews for a strategy.
#[tauri::command]
pub fn get_strategy_reviews(
    state: State<'_, AppState>,
    strategy_id: String,
) -> Result<Vec<StrategyReview>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::list_strategy_reviews(&db, &strategy_id).map_err(|e| e.to_string())
}

/// Create follow-up tasks from approved recommendations.
///
/// Only creates tasks for recommendations with `approval_status = Approved`.
/// Uses idempotency keys to prevent duplicates.
#[tauri::command]
pub fn create_tasks_from_approved_recommendations(
    state: State<'_, AppState>,
    strategy_id: String,
    project_id: String,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Load strategy from task artifact or file
    let strategy_json = if strategy_id.starts_with("strategy-") && strategy_id.contains("-file") {
        // Synthetic ID — read from file
        let project = crate::engine::task_store::get_project(&db, &project_id)
            .map_err(|e| e.to_string())?;
        let paths = ProjectPaths::from_project(&project);
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        // Real task ID — load task and extract artifact
        let task = crate::engine::task_store::get_task(&db, &strategy_id)
            .map_err(|e| e.to_string())?;
        task.artifacts
            .iter()
            .find(|a| a.key == "cannibalization_strategy")
            .and_then(|a| a.content.clone())
            .unwrap_or_default()
    };

    if strategy_json.is_empty() {
        return Err("No strategy found".to_string());
    }

    let strategy: CannibalizationStrategy =
        serde_json::from_str(&strategy_json).map_err(|e| format!("Invalid strategy JSON: {}", e))?;

    let reviews = crate::db::list_strategy_reviews(&db, &strategy_id).map_err(|e| e.to_string())?;

    // Build a set of approved recommendation IDs
    let approved: std::collections::HashSet<String> = reviews
        .into_iter()
        .filter(|r| r.approval_status == ApprovalStatus::Approved)
        .map(|r| format!("{}:{}", r.recommendation_type, r.recommendation_id))
        .collect();

    let artifact = TaskArtifact {
        key: "cannibalization_strategy".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("cannibalization_audit".to_string()),
        content: Some(strategy_json),
    };

    let mut created_ids = Vec::new();

    // Merge recommendations
    for rec in &strategy.merge_recommendations {
        let key = format!("merge:{}", rec.cluster_id);
        if !approved.contains(&key) {
            continue;
        }
        let idempotency_key = format!(
            "can_fix:merge:{}:{}:{}",
            project_id, strategy_id, rec.cluster_id
        );
        let spec = crate::engine::spawner::TaskSpec {
            project_id: project_id.clone(),
            task_type: "fix_content_merge".to_string(),
            title: Some(format!("Merge cluster: {}", rec.cluster_id)),
            description: Some(format!(
                "Approved merge: keep {} → redirect {:?}",
                rec.keep_url, rec.redirect_urls
            )),
            priority: Priority::Medium,
            execution_mode: Some(ExecutionMode::Spec),
            agent_policy: crate::models::task::AgentPolicy::Required,
            depends_on: vec![strategy_id.clone()],
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key),
            ..Default::default()
        };
        match crate::engine::spawner::TaskSpawner::spawn(&db, spec) {
            Ok(task) => created_ids.push(task.id),
            Err(e) => log::warn!("Failed to create merge task: {}", e),
        }
    }

    // Hub recommendations
    for rec in &strategy.hub_recommendations {
        let key = format!("hub:{}", rec.topic);
        if !approved.contains(&key) {
            continue;
        }
        let idempotency_key = format!(
            "can_fix:hub:{}:{}:{}",
            project_id, strategy_id, rec.topic
        );
        let spec = crate::engine::spawner::TaskSpec {
            project_id: project_id.clone(),
            task_type: "fix_hub_page".to_string(),
            title: Some(format!("Create hub: {}", rec.suggested_title)),
            description: Some(format!(
                "Approved hub page: {} → {}",
                rec.suggested_url, rec.suggested_title
            )),
            priority: Priority::Medium,
            execution_mode: Some(ExecutionMode::Spec),
            agent_policy: crate::models::task::AgentPolicy::Required,
            depends_on: vec![strategy_id.clone()],
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key),
            ..Default::default()
        };
        match crate::engine::spawner::TaskSpawner::spawn(&db, spec) {
            Ok(task) => created_ids.push(task.id),
            Err(e) => log::warn!("Failed to create hub task: {}", e),
        }
    }

    // Territory recommendations
    for rec in &strategy.territory_recommendations {
        let key = format!("territory:{}", rec.theme);
        if !approved.contains(&key) {
            continue;
        }
        let idempotency_key = format!(
            "can_fix:territory:{}:{}:{}",
            project_id, strategy_id, rec.theme
        );
        let spec = crate::engine::spawner::TaskSpec {
            project_id: project_id.clone(),
            task_type: "research_territory".to_string(),
            title: Some(format!("Research territory: {}", rec.theme)),
            description: Some(format!(
                "Approved territory research: {} (priority: {})",
                rec.theme, rec.priority
            )),
            priority: Priority::Medium,
            execution_mode: Some(ExecutionMode::Spec),
            agent_policy: crate::models::task::AgentPolicy::Required,
            depends_on: vec![strategy_id.clone()],
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key),
            ..Default::default()
        };
        match crate::engine::spawner::TaskSpawner::spawn(&db, spec) {
            Ok(task) => created_ids.push(task.id),
            Err(e) => log::warn!("Failed to create territory task: {}", e),
        }
    }

    Ok(created_ids)
}
