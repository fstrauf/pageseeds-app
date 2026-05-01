/// Cannibalization strategy domain module.
///
/// Business logic for loading strategies, managing reviews, and spawning
/// follow-up tasks from approved recommendations. Commands in
/// `commands/cannibalization.rs` are thin wrappers around these functions.
use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::engine::task_store;
use crate::error::Result;
use crate::models::cannibalization::{
    ApprovalStatus, CannibalizationStrategy, CalculatorRecommendation, HubRecommendation,
    MergeRecommendation, RecommendationTaskStatus, StrategyReview, StrategyWithReviews,
    TerritoryRecommendation,
};
use crate::models::task::{AgentPolicy, Priority, TaskArtifact, TaskRunPolicy};
use rusqlite::{Connection, OptionalExtension};

// ═══════════════════════════════════════════════════════════════════════════════
// Strategy Loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Load strategy JSON using dual-source logic:
/// 1. Try the task artifact if `strategy_id` is a real task
/// 2. Fall back to `cannibalization_strategy.json` in the automation dir
pub fn load_strategy_json(
    db: &Connection,
    strategy_id: &str,
    project_id: &str,
) -> Result<String> {
    // 1. Try task artifact (only if this is a real task ID)
    let from_task = if !strategy_id.starts_with("strategy-") || !strategy_id.contains("-file") {
        task_store::get_task(db, strategy_id)
            .ok()
            .and_then(|t| {
                t.artifacts
                    .iter()
                    .find(|a| a.key == "cannibalization_strategy")
                    .and_then(|a| a.content.clone())
            })
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    if let Some(json) = from_task {
        return Ok(json);
    }

    // 2. Fall back to file
    let project = task_store::get_project(db, project_id)?;
    let paths = ProjectPaths::from_project(&project);
    let path = paths.automation_dir.join("cannibalization_strategy.json");
    let json = std::fs::read_to_string(&path).unwrap_or_default();

    if json.is_empty() {
        Err(crate::error::Error::StrategyNotFound)
    } else {
        Ok(json)
    }
}

/// Resolve the strategy ID for a project.
/// Returns the latest `cannibalization_audit` task ID, or a synthetic file-based ID.
pub fn resolve_strategy_id(db: &Connection, project_id: &str) -> Result<String> {
    let mut stmt = db.prepare(
        "SELECT id FROM tasks
         WHERE project_id = ?1 AND type = 'cannibalization_audit'
         ORDER BY created_at DESC LIMIT 1",
    )?;

    let task_id: Option<String> = stmt
        .query_row([project_id], |row| row.get(0))
        .optional()?;

    Ok(task_id.unwrap_or_else(|| format!("strategy-{}-file", project_id)))
}

/// Load the full `StrategyWithReviews` view model for a project.
pub fn get_strategy_with_reviews(
    db: &Connection,
    project_id: &str,
) -> Result<Option<StrategyWithReviews>> {
    let strategy_id = resolve_strategy_id(db, project_id)?;

    let strategy_json = match load_strategy_json(db, &strategy_id, project_id) {
        Ok(json) => json,
        Err(crate::error::Error::StrategyNotFound) => return Ok(None),
        Err(e) => return Err(e),
    };

    let mut strategy: CannibalizationStrategy = serde_json::from_str(&strategy_json)
        .map_err(|e| crate::error::Error::InvalidJson(format!("strategy: {}", e)))?;

    // Defensive dedup: stale strategy files or old audit runs may contain duplicate
    // merge recommendations with the same cluster_id. Deduplicate here so the UI
    // never renders "click one, all ticked" cards.
    {
        let mut seen = std::collections::HashSet::new();
        strategy.merge_recommendations.retain(|rec| {
            if rec.cluster_id.is_empty() {
                return false;
            }
            seen.insert(rec.cluster_id.clone())
        });
    }

    let reviews = crate::db::list_strategy_reviews(db, &strategy_id)?;
    let task_statuses = load_recommendation_task_statuses(db, project_id)?;

    Ok(Some(StrategyWithReviews {
        strategy,
        reviews,
        task_statuses,
        strategy_id,
        project_id: project_id.to_string(),
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Task Status Lookup
// ═══════════════════════════════════════════════════════════════════════════════

/// Query the idempotency key table for existing cannibalization fix tasks
/// and map them back to recommendation type + id.
pub fn load_recommendation_task_statuses(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<RecommendationTaskStatus>> {
    let pattern = format!("can_fix:%:{}:%", project_id);
    let mut stmt = conn.prepare(
        "SELECT k.key, t.id, t.status
         FROM task_idempotency_keys k
         JOIN tasks t ON k.task_id = t.id
         WHERE k.key LIKE ?1",
    )?;

    let rows = stmt.query_map([&pattern], |row| {
        let key: String = row.get(0)?;
        let task_id: String = row.get(1)?;
        let status: String = row.get(2)?;

        // Parse key: can_fix:{type}:{project_id}:{rec_id}
        let parts: Vec<&str> = key.split(':').collect();
        let (rec_type, rec_id) = if parts.len() >= 4 {
            (parts[1].to_string(), parts[3].to_string())
        } else {
            ("unknown".to_string(), key.clone())
        };

        Ok(RecommendationTaskStatus {
            recommendation_type: rec_type,
            recommendation_id: rec_id,
            task_id: Some(task_id),
            task_status: Some(status),
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Task Spawning
// ═══════════════════════════════════════════════════════════════════════════════

/// Create follow-up tasks from approved recommendations.
///
/// Only creates tasks for recommendations with `approval_status = Approved`.
/// Uses idempotency keys to prevent duplicates.
pub fn spawn_tasks_from_approved(
    db: &Connection,
    strategy_id: &str,
    project_id: &str,
) -> Result<Vec<String>> {
    let strategy_json = load_strategy_json(db, strategy_id, project_id)?;

    let mut strategy: CannibalizationStrategy = serde_json::from_str(&strategy_json)
        .map_err(|e| crate::error::Error::InvalidJson(format!("strategy: {}", e)))?;

    // Deduplicate merge recommendations by cluster_id — stale strategy files may
    // contain duplicates that bypass the reducer dedup.
    {
        let mut seen = std::collections::HashSet::new();
        strategy.merge_recommendations.retain(|rec| {
            if rec.cluster_id.is_empty() {
                return false;
            }
            seen.insert(rec.cluster_id.clone())
        });
    }

    let reviews = crate::db::list_strategy_reviews(db, strategy_id)?;

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

    // Only depend on the strategy task if it actually exists in the DB.
    let strategy_is_task = task_store::get_task(db, strategy_id).is_ok();
    let depends_on = if strategy_is_task {
        vec![strategy_id.to_string()]
    } else {
        vec![]
    };

    let mut created_ids = Vec::new();

    // Merge recommendations
    for rec in &strategy.merge_recommendations {
        let key = format!("merge:{}", rec.cluster_id);
        if !approved.contains(&key) {
            continue;
        }
        let idempotency_key = format!("can_fix:merge:{}:{}", project_id, rec.cluster_id);
        let spec = TaskSpec {
            project_id: project_id.to_string(),
            task_type: "consolidate_cluster".to_string(),
            title: Some(format!("Merge cluster: {}", rec.cluster_id)),
            description: Some(format!(
                "Approved merge: keep {} → redirect {:?}",
                rec.keep_url, rec.redirect_urls
            )),
            priority: Priority::Medium,
            run_policy: Some(TaskRunPolicy::UserEnqueue),
            agent_policy: AgentPolicy::Required,
            depends_on: depends_on.clone(),
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfAnyExists),
            ..Default::default()
        };
        match TaskSpawner::spawn(db, spec) {
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
        let idempotency_key = format!("can_fix:hub:{}:{}", project_id, rec.topic);
        let hub_brief = serde_json::json!({
            "topic": rec.topic,
            "suggested_title": rec.suggested_title,
            "suggested_url": rec.suggested_url,
            "intent": rec.intent,
            "source_pages": rec.source_pages,
            "spoke_pages": rec.spoke_pages,
            "outline": rec.outline,
        });
        let hub_artifact = TaskArtifact {
            key: "hub_brief".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("cannibalization_audit".to_string()),
            content: Some(hub_brief.to_string()),
        };
        let spec = TaskSpec {
            project_id: project_id.to_string(),
            task_type: "write_article".to_string(),
            title: Some(format!("Create hub: {}", rec.suggested_title)),
            description: Some(format!(
                "Approved hub page: {} → {}",
                rec.suggested_url, rec.suggested_title
            )),
            priority: Priority::Medium,
            run_policy: Some(TaskRunPolicy::UserEnqueue),
            // Hub write_article tasks are spawned from already-approved strategy
            // recommendations. The strategy review was the human checkpoint; forcing
            // artifact review on the resulting article is redundant.
            review_surface: None,
            agent_policy: AgentPolicy::Required,
            depends_on: depends_on.clone(),
            artifacts: vec![hub_artifact],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfAnyExists),
            ..Default::default()
        };
        match TaskSpawner::spawn(db, spec) {
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
        let idempotency_key = format!("can_fix:territory:{}:{}", project_id, rec.theme);
        let spec = TaskSpec {
            project_id: project_id.to_string(),
            task_type: "territory_research".to_string(),
            title: Some(format!("Research territory: {}", rec.theme)),
            description: Some(format!(
                "Approved territory research: {} (priority: {})",
                rec.theme, rec.priority
            )),
            priority: Priority::Medium,
            run_policy: Some(TaskRunPolicy::UserEnqueue),
            agent_policy: AgentPolicy::Required,
            depends_on: depends_on.clone(),
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfAnyExists),
            ..Default::default()
        };
        match TaskSpawner::spawn(db, spec) {
            Ok(task) => created_ids.push(task.id),
            Err(e) => log::warn!("Failed to create territory task: {}", e),
        }
    }

    // Calculator recommendations
    for rec in &strategy.calculator_recommendations {
        let key = format!("calculator:{}", rec.strategy);
        if !approved.contains(&key) {
            continue;
        }
        let idempotency_key = format!("can_fix:calculator:{}:{}", project_id, rec.strategy);
        let spec = TaskSpec {
            project_id: project_id.to_string(),
            task_type: "calculator_rollout".to_string(),
            title: Some(format!("Calculator rollout: {}", rec.strategy)),
            description: Some(format!(
                "Approved calculator: {} (universe: {})",
                rec.strategy, rec.ticker_universe
            )),
            priority: Priority::Medium,
            run_policy: Some(TaskRunPolicy::UserEnqueue),
            agent_policy: AgentPolicy::Required,
            depends_on: depends_on.clone(),
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfAnyExists),
            ..Default::default()
        };
        match TaskSpawner::spawn(db, spec) {
            Ok(task) => created_ids.push(task.id),
            Err(e) => log::warn!("Failed to create calculator task: {}", e),
        }
    }

    // Defensive dedup: duplicate recommendations may share the same idempotency key.
    let mut seen = std::collections::HashSet::new();
    created_ids.retain(|id| seen.insert(id.clone()));

    Ok(created_ids)
}
