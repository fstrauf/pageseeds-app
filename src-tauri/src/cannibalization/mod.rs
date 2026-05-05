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
    ApprovalStatus, CalculatorRecommendation, CannibalizationSelection, CannibalizationStrategy,
    HubRecommendation, MergeRecommendation, RecommendationTaskStatus, StrategyReview,
    StrategyWithReviews, TerritoryRecommendation,
};
use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskRunPolicy, TaskStatus};
use rusqlite::{Connection, OptionalExtension};

// ═══════════════════════════════════════════════════════════════════════════════
// Hub Page Backfill
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect existing hub-like pages and persist `page_type = "hub"` in the DB + articles.json.
///
/// Heuristics used:
/// - URL slug starts with `hub/`, `guide/`, `hub_`, or `guide_`
/// - Title contains "complete guide" or "ultimate guide"
/// - Word count > 2000 AND target_keyword is broad (3+ words or generic single word)
///
/// Returns the number of articles updated.
pub fn backfill_hub_page_types(db: &Connection, project_id: &str) -> Result<usize> {
    let articles = task_store::list_articles(db, project_id)?;
    let mut updated = 0;

    for article in &articles {
        let slug = article.url_slug.to_lowercase();
        let title = article.title.to_lowercase();
        let kw = article
            .target_keyword
            .as_deref()
            .unwrap_or("")
            .to_lowercase();

        let is_hub_url = slug.starts_with("hub/")
            || slug.starts_with("guide/")
            || slug.starts_with("hub_")
            || slug.starts_with("guide_");

        let is_hub_title = title.contains("complete guide")
            || title.contains("ultimate guide")
            || title.contains("complete overview");

        let is_hub_by_content = article.word_count > 2000
            && (kw.split_whitespace().count() >= 3 || kw.is_empty() && article.word_count > 3000);

        if is_hub_url || is_hub_title || is_hub_by_content {
            db.execute(
                "UPDATE articles SET page_type = 'hub' WHERE id = ?1 AND project_id = ?2",
                rusqlite::params![article.id, project_id],
            )?;
            updated += 1;
        }
    }

    if updated > 0 {
        if let Ok(project) = task_store::get_project(db, project_id) {
            let _ = crate::db::export::write_articles_to_repo(
                db,
                project_id,
                std::path::Path::new(&project.path),
            );
        }
    }

    log::info!(
        "[backfill_hub_page_types] Updated {} articles to page_type='hub' for project {}",
        updated,
        project_id
    );
    Ok(updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Strategy Loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Load strategy JSON using dual-source logic:
/// 1. Try the task artifact if `strategy_id` is a real task
/// 2. Fall back to `cannibalization_strategy.json` in the automation dir
pub fn load_strategy_json(db: &Connection, strategy_id: &str, project_id: &str) -> Result<String> {
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

    let task_id: Option<String> = stmt.query_row([project_id], |row| row.get(0)).optional()?;

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

// ═══════════════════════════════════════════════════════════════════════════════
// Task Spawning from Selection (task-drawer picker flow)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create follow-up tasks from user selections in the task drawer.
///
/// Validates selections against the parent task's strategy artifact,
/// creates child tasks via TaskSpawner, marks the parent done,
/// and returns the full created tasks.
///
/// Uses `serde_json::Value` instead of the strict `CannibalizationStrategy`
/// model because the reducer emits loose shapes (e.g. hub gaps without
/// `intent`, risks as strings) that do not deserialize into the typed struct.
pub fn spawn_tasks_from_selection(
    db: &Connection,
    parent_task_id: &str,
    selections: &[CannibalizationSelection],
) -> Result<Vec<Task>> {
    if selections.is_empty() {
        return Err(crate::error::Error::Validation(
            "No selections provided".to_string(),
        ));
    }

    let parent_task = task_store::get_task(db, parent_task_id)?;

    if parent_task.task_type != "cannibalization_audit" {
        return Err(crate::error::Error::Validation(
            "Parent task is not a cannibalization audit".to_string(),
        ));
    }

    let strategy_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .ok_or_else(|| crate::error::Error::StrategyNotFound)?;

    let strategy: serde_json::Value = serde_json::from_str(&strategy_json)
        .map_err(|e| crate::error::Error::InvalidJson(format!("strategy: {}", e)))?;

    // Helper to extract string from a JSON value safely.
    fn get_str<'v>(v: &'v serde_json::Value, key: &str) -> Option<&'v str> {
        v.get(key).and_then(|x| x.as_str())
    }

    // Build a set of valid recommendation keys and lookup maps.
    let mut valid_keys = std::collections::HashSet::new();

    let merge_recs = strategy
        .get("merge_recommendations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut merge_map = std::collections::HashMap::new();
    for rec in &merge_recs {
        if let Some(id) = get_str(rec, "cluster_id") {
            if !id.is_empty() {
                valid_keys.insert(format!("merge:{}", id));
                merge_map.insert(id.to_string(), rec);
            }
        }
    }

    let hub_recs = strategy
        .get("hub_recommendations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut hub_map = std::collections::HashMap::new();
    for rec in &hub_recs {
        if let Some(id) = get_str(rec, "topic") {
            if !id.is_empty() {
                valid_keys.insert(format!("hub:{}", id));
                hub_map.insert(id.to_string(), rec);
            }
        }
    }

    let territory_recs = strategy
        .get("territory_recommendations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut territory_map = std::collections::HashMap::new();
    for rec in &territory_recs {
        if let Some(id) = get_str(rec, "theme") {
            if !id.is_empty() {
                valid_keys.insert(format!("territory:{}", id));
                territory_map.insert(id.to_string(), rec);
            }
        }
    }

    let calculator_recs = strategy
        .get("calculator_recommendations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut calculator_map = std::collections::HashMap::new();
    for rec in &calculator_recs {
        if let Some(id) = get_str(rec, "strategy") {
            if !id.is_empty() {
                valid_keys.insert(format!("calculator:{}", id));
                calculator_map.insert(id.to_string(), rec);
            }
        }
    }

    // Validate all selections
    for sel in selections {
        let key = format!("{}:{}", sel.recommendation_type, sel.recommendation_id);
        if !valid_keys.contains(&key) {
            return Err(crate::error::Error::Validation(format!(
                "Selection {} not found in strategy artifact",
                key
            )));
        }
    }

    let artifact = TaskArtifact {
        key: "cannibalization_strategy".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("cannibalization_audit".to_string()),
        content: Some(strategy_json),
    };

    let depends_on = vec![parent_task_id.to_string()];
    let project_id = parent_task.project_id.clone();
    let mut created_tasks = Vec::new();

    for sel in selections {
        let task = match sel.recommendation_type.as_str() {
            "merge" => {
                let rec = merge_map.get(&sel.recommendation_id).ok_or_else(|| {
                    crate::error::Error::Validation("Merge rec not found".to_string())
                })?;
                let cluster_id = sel.recommendation_id.clone();
                let keep_url = get_str(rec, "keep_url").unwrap_or("");
                let redirect_urls: Vec<String> = rec
                    .get("redirect_urls")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let idempotency_key = format!("can_fix:merge:{}:{}", project_id, cluster_id);
                let spec = TaskSpec {
                    project_id: project_id.clone(),
                    task_type: "consolidate_cluster".to_string(),
                    title: Some(format!("Merge cluster: {}", cluster_id)),
                    description: Some(format!(
                        "Merge: keep {} → redirect {:?}",
                        keep_url, redirect_urls
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
                TaskSpawner::spawn(db, spec)?
            }
            "hub" => {
                let rec = hub_map.get(&sel.recommendation_id).ok_or_else(|| {
                    crate::error::Error::Validation("Hub rec not found".to_string())
                })?;
                let topic = sel.recommendation_id.clone();
                let suggested_title = get_str(rec, "suggested_title")
                    .unwrap_or(&topic)
                    .to_string();
                let suggested_url = get_str(rec, "suggested_url").unwrap_or("").to_string();
                let intent = get_str(rec, "intent").unwrap_or("").to_string();
                let source_pages: Vec<i64> = rec
                    .get("source_pages")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
                    .unwrap_or_default();
                let spoke_pages: Vec<i64> = rec
                    .get("spoke_pages")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
                    .unwrap_or_default();
                let outline: Vec<String> = rec
                    .get("outline")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let idempotency_key = format!("can_fix:hub:{}:{}", project_id, topic);
                let hub_brief = serde_json::json!({
                    "topic": topic,
                    "suggested_title": suggested_title,
                    "suggested_url": suggested_url,
                    "intent": intent,
                    "source_pages": source_pages,
                    "spoke_pages": spoke_pages,
                    "outline": outline,
                });
                let hub_artifact = TaskArtifact {
                    key: "hub_brief".to_string(),
                    path: None,
                    artifact_type: Some("json".to_string()),
                    source: Some("cannibalization_audit".to_string()),
                    content: Some(hub_brief.to_string()),
                };
                let spec = TaskSpec {
                    project_id: project_id.clone(),
                    task_type: "write_article".to_string(),
                    title: Some(format!("Create hub: {}", suggested_title)),
                    description: Some(format!("Hub page: {} → {}", suggested_url, suggested_title)),
                    priority: Priority::Medium,
                    run_policy: Some(TaskRunPolicy::UserEnqueue),
                    review_surface: None,
                    agent_policy: AgentPolicy::Required,
                    depends_on: depends_on.clone(),
                    artifacts: vec![hub_artifact],
                    idempotency_key: Some(idempotency_key),
                    dedup_policy: Some(DeduplicationPolicy::SkipIfAnyExists),
                    ..Default::default()
                };
                TaskSpawner::spawn(db, spec)?
            }
            "territory" => {
                let rec = territory_map.get(&sel.recommendation_id).ok_or_else(|| {
                    crate::error::Error::Validation("Territory rec not found".to_string())
                })?;
                let theme = sel.recommendation_id.clone();
                let priority = get_str(rec, "priority").unwrap_or("medium").to_string();
                let idempotency_key = format!("can_fix:territory:{}:{}", project_id, theme);
                let spec = TaskSpec {
                    project_id: project_id.clone(),
                    task_type: "territory_research".to_string(),
                    title: Some(format!("Research territory: {}", theme)),
                    description: Some(format!(
                        "Territory research: {} (priority: {})",
                        theme, priority
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
                TaskSpawner::spawn(db, spec)?
            }
            "calculator" => {
                let rec = calculator_map.get(&sel.recommendation_id).ok_or_else(|| {
                    crate::error::Error::Validation("Calculator rec not found".to_string())
                })?;
                let strategy_name = sel.recommendation_id.clone();
                let ticker_universe = get_str(rec, "ticker_universe").unwrap_or("").to_string();
                let idempotency_key =
                    format!("can_fix:calculator:{}:{}", project_id, strategy_name);
                let spec = TaskSpec {
                    project_id: project_id.clone(),
                    task_type: "calculator_rollout".to_string(),
                    title: Some(format!("Calculator rollout: {}", strategy_name)),
                    description: Some(format!(
                        "Calculator: {} (universe: {})",
                        strategy_name, ticker_universe
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
                TaskSpawner::spawn(db, spec)?
            }
            other => {
                return Err(crate::error::Error::Validation(format!(
                    "Unknown recommendation type: {}",
                    other
                )));
            }
        };
        created_tasks.push(task);
    }

    // Mark parent as done
    task_store::update_task_status(db, parent_task_id, TaskStatus::Done)?;

    Ok(created_tasks)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{FollowUpPolicy, TaskReviewSurface, TaskRunPolicy};

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn test_project_in(conn: &Connection) -> String {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp', 1)",
            [],
        )
        .unwrap();
        "proj1".to_string()
    }

    fn make_task(task_type: &str, project_id: &str) -> crate::models::task::Task {
        crate::models::task::Task {
            id: format!("test-{}", uuid::Uuid::new_v4()),
            task_type: task_type.to_string(),
            phase: "investigation".to_string(),
            status: TaskStatus::Review,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::CannibalizationPicker,
            follow_up_policy: FollowUpPolicy::UserSelection,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{} test", task_type)),
            description: None,
            project_id: project_id.to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun {
                attempts: 0,
                last_error: None,
                provider: None,
                ..Default::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    fn strategy_artifact() -> TaskArtifact {
        let json = serde_json::json!({
            "generated_at": "2024-01-01T00:00:00Z",
            "merge_recommendations": [
                {
                    "cluster_id": "risk-management",
                    "confidence": "high",
                    "keep_url": "/blog/risk",
                    "redirect_urls": ["/blog/risk-old"],
                    "reason": "Duplicate coverage"
                }
            ],
            "hub_recommendations": [
                {
                    "topic": "risk-management",
                    "suggested_url": "/hub/risk-management",
                    "suggested_title": "Risk Management Hub",
                    "spoke_pages": [1, 2],
                    "reason": "Gap detected"
                }
            ],
            "territory_recommendations": [
                {
                    "theme": "portfolio-hedging",
                    "priority": "high",
                    "suggested_tasks": ["Research hedging strategies"]
                }
            ],
            "calculator_recommendations": [
                {
                    "strategy": "black-scholes",
                    "ticker_universe": "US equities",
                    "indexing_policy": "weekly",
                    "reason": "High demand"
                }
            ],
            "risks": ["Candidate X failed: missing data"]
        });
        TaskArtifact {
            key: "cannibalization_strategy".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("cannibalization_audit".to_string()),
            content: Some(json.to_string()),
        }
    }

    #[test]
    fn selection_rejects_empty_selections() {
        let conn = in_memory_db();
        let project_id = test_project_in(&conn);
        let mut parent = make_task("cannibalization_audit", &project_id);
        parent.artifacts.push(strategy_artifact());
        task_store::create_task(&conn, &parent).unwrap();

        let result = spawn_tasks_from_selection(&conn, &parent.id, &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No selections provided"),
            "expected empty-selection error, got: {}",
            err
        );
    }

    #[test]
    fn selection_rejects_invalid_parent_task_type() {
        let conn = in_memory_db();
        let project_id = test_project_in(&conn);
        let mut parent = make_task("content_audit", &project_id);
        parent.artifacts.push(strategy_artifact());
        task_store::create_task(&conn, &parent).unwrap();

        let selections = vec![CannibalizationSelection {
            recommendation_type: "merge".to_string(),
            recommendation_id: "risk-management".to_string(),
        }];
        let result = spawn_tasks_from_selection(&conn, &parent.id, &selections);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not a cannibalization audit"),
            "expected type-mismatch error, got: {}",
            err
        );
    }

    #[test]
    fn selection_rejects_id_not_in_artifact() {
        let conn = in_memory_db();
        let project_id = test_project_in(&conn);
        let mut parent = make_task("cannibalization_audit", &project_id);
        parent.artifacts.push(strategy_artifact());
        task_store::create_task(&conn, &parent).unwrap();

        let selections = vec![CannibalizationSelection {
            recommendation_type: "merge".to_string(),
            recommendation_id: "does-not-exist".to_string(),
        }];
        let result = spawn_tasks_from_selection(&conn, &parent.id, &selections);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found in strategy artifact"),
            "expected not-found error, got: {}",
            err
        );
    }

    #[test]
    fn selection_creates_expected_child_task_types() {
        let conn = in_memory_db();
        let project_id = test_project_in(&conn);
        let mut parent = make_task("cannibalization_audit", &project_id);
        parent.artifacts.push(strategy_artifact());
        task_store::create_task(&conn, &parent).unwrap();

        let selections = vec![
            CannibalizationSelection {
                recommendation_type: "merge".to_string(),
                recommendation_id: "risk-management".to_string(),
            },
            CannibalizationSelection {
                recommendation_type: "hub".to_string(),
                recommendation_id: "risk-management".to_string(),
            },
            CannibalizationSelection {
                recommendation_type: "territory".to_string(),
                recommendation_id: "portfolio-hedging".to_string(),
            },
            CannibalizationSelection {
                recommendation_type: "calculator".to_string(),
                recommendation_id: "black-scholes".to_string(),
            },
        ];

        let tasks = spawn_tasks_from_selection(&conn, &parent.id, &selections).unwrap();
        assert_eq!(tasks.len(), 4);

        let types: Vec<String> = tasks.iter().map(|t| t.task_type.clone()).collect();
        assert!(types.contains(&"consolidate_cluster".to_string()));
        assert!(types.contains(&"write_article".to_string()));
        assert!(types.contains(&"territory_research".to_string()));
        assert!(types.contains(&"calculator_rollout".to_string()));
    }

    #[test]
    fn selection_marks_parent_done() {
        let conn = in_memory_db();
        let project_id = test_project_in(&conn);
        let mut parent = make_task("cannibalization_audit", &project_id);
        parent.artifacts.push(strategy_artifact());
        task_store::create_task(&conn, &parent).unwrap();

        let selections = vec![CannibalizationSelection {
            recommendation_type: "merge".to_string(),
            recommendation_id: "risk-management".to_string(),
        }];

        spawn_tasks_from_selection(&conn, &parent.id, &selections).unwrap();
        let updated = task_store::get_task(&conn, &parent.id).unwrap();
        assert_eq!(updated.status, TaskStatus::Done);
    }

    #[test]
    fn selection_is_idempotent() {
        let conn = in_memory_db();
        let project_id = test_project_in(&conn);
        let mut parent = make_task("cannibalization_audit", &project_id);
        parent.artifacts.push(strategy_artifact());
        task_store::create_task(&conn, &parent).unwrap();

        let selections = vec![CannibalizationSelection {
            recommendation_type: "merge".to_string(),
            recommendation_id: "risk-management".to_string(),
        }];

        let first = spawn_tasks_from_selection(&conn, &parent.id, &selections).unwrap();
        assert_eq!(first.len(), 1);

        // Second call should return the existing task (idempotency via TaskSpawner)
        let second = spawn_tasks_from_selection(&conn, &parent.id, &selections).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].id, second[0].id);
    }
}
