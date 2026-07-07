use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::indexing_health::{
    DistinctivenessVerdict, IndexingCampaignPlan, IndexingCampaignSummary, IndexingTargetContext,
    IndexingTargetPlan, PrerequisiteCheck, PrerequisiteReport, TargetDiagnosis,
};
use crate::models::task::Task;


// ═══════════════════════════════════════════════════════════════════════════════
// Post-action: Spawn child tasks from campaign plan
// ═══════════════════════════════════════════════════════════════════════════════

use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};
use rusqlite::Connection;

/// Read the campaign plan and spawn appropriate child fix tasks.
pub(crate) fn spawn_campaign_children(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Load campaign plan from DB (primary) or JSON fallback
    let plan: IndexingCampaignPlan = {
        let db_plan = crate::db::content_audit::get_latest_audit_artifact(conn, &parent_task.project_id, "indexing_campaign_plan")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<IndexingCampaignPlan>(v).ok());
        match db_plan {
            Some(p) => p,
            None => {
                let plan_path = paths.automation_dir.join("indexing_campaign_plan.json");
                match std::fs::read_to_string(&plan_path) {
                    Ok(raw) => match serde_json::from_str(&raw) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!("[ihc_post_action] failed to parse campaign plan: {}", e);
                            return vec![];
                        }
                    },
                    Err(e) => {
                        log::warn!("[ihc_post_action] plan file not found: {}", e);
                        return vec![];
                    }
                }
            }
        }
    };

    // Load full target contexts so we can attach cluster artifacts to child tasks
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts: HashMap<String, IndexingTargetContext> = std::fs::read_to_string(&contexts_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["targets"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            let ctx: IndexingTargetContext = serde_json::from_value(v).ok()?;
            Some((ctx.target.url.clone(), ctx))
        })
        .collect();

    // Load content audit so fix_content specs get actual failed checks (Stage B)
    let audit = crate::engine::exec::common::load_audit_snapshot(&parent_task.project_id, &paths);
    let audit_by_file = &audit.by_file;
    let audit_by_slug = &audit.by_slug;

    let mut created_ids: Vec<String> = Vec::new();

    // Collect all spawnable targets with priority ordering
    let mut spawnable: Vec<(&IndexingTargetPlan, TaskSpec)> = Vec::new();

    for target in &plan.targets {
        let ctx = contexts.get(&target.url);

        // Skip tasks that require a known article but don't have one
        let requires_article = matches!(
            target.recommended_action.as_str(),
            "fix_content" | "add_links" | "rewrite_title_h1"
        );
        if requires_article {
            match ctx {
                None => {
                    log::warn!(
                        "[ihc_post_action] skipping {} for {} — no target context available",
                        target.recommended_action, target.url
                    );
                    continue;
                }
                Some(ctx) if ctx.target.article_id == 0 => {
                    log::warn!(
                        "[ihc_post_action] skipping {} for {} — no matching article in articles.json (slug lookup failed)",
                        target.recommended_action, target.url
                    );
                    continue;
                }
                _ => {}
            }
        }

        // Look up audit row for this target so fix_content gets real issues
        let audit_row = ctx.and_then(|c| {
            audit_by_file.get(&c.target.file)
                .or_else(|| audit_by_slug.get(&c.target.slug))
        });

        let spec = match target.recommended_action.as_str() {
            "fix_content" => Some(build_fix_content_spec(parent_task, target, ctx, audit_row)),
            "add_links" => Some(build_add_links_spec(parent_task, target, ctx)),
            "rewrite_title_h1" => Some(build_rewrite_spec(parent_task, target, ctx)),
            "merge" => {
                // Merge recommendations require user approval via CannibalizationPicker.
                // Do NOT auto-spawn. Instead, log for visibility.
                log::info!(
                    "[ihc_post_action] merge recommended for {} — awaiting user approval",
                    target.url
                );
                None
            }
            "no_action" | _ => None,
        };

        if let Some(spec) = spec {
            spawnable.push((target, spec));
        }
    }

    // Priority: fix_content > add_links > rewrite_title_h1
    spawnable.sort_by(|(a, _), (b, _)| {
        let priority = |action: &str| match action {
            "fix_content" => 0,
            "add_links" => 1,
            "rewrite_title_h1" => 2,
            _ => 3,
        };
        priority(a.recommended_action.as_str()).cmp(&priority(b.recommended_action.as_str()))
    });

    for (target, spec) in spawnable {
        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[ihc_post_action] spawned {} for {}",
                    task.task_type,
                    target.url
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!(
                    "[ihc_post_action] failed to spawn task for {}: {}",
                    target.url,
                    e
                );
            }
        }
    }

    log::info!(
        "[ihc_post_action] created {} child tasks from campaign plan",
        created_ids.len()
    );
    created_ids
}

/// Common TaskSpec skeleton for all campaign-spawned fix tasks.
/// Each variant provides its unique task_type, title, description, idempotency_key, and artifacts.
fn fix_task_spec(
    parent: &Task,
    task_type: &str,
    title: String,
    description: String,
    idempotency_key: String,
    artifacts: Vec<crate::models::task::TaskArtifact>,
) -> TaskSpec {
    TaskSpec {
        project_id: parent.project_id.clone(),
        task_type: task_type.to_string(),
        title: Some(title),
        description: Some(description),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 30 }),
        depends_on: vec![parent.id.clone()],
        artifacts,
        ..Default::default()
    }
}

pub(crate) fn build_fix_content_spec(
    parent: &Task,
    target: &IndexingTargetPlan,
    ctx: Option<&IndexingTargetContext>,
    audit_row: Option<&serde_json::Value>,
) -> TaskSpec {
    let url_slug = crate::content::slug::extract_slug_from_url(&target.url);
    let article_id = ctx.map(|c| c.target.article_id).unwrap_or(0);
    let idempotency_key = format!("fix_content_article:{}:{}", parent.project_id, article_id);

    // Build artifacts required by the fix_content_article pipeline
    let mut artifacts = vec![];
    if let Some(ctx) = ctx {
        let article_id = ctx.target.article_id;
        if article_id > 0 {
            // Build suggestions from actual audit failed checks instead of generic stubs
            let mut suggestions = vec![];
            if let Some(audit) = audit_row {
                if let Some(checks) = audit["checks"].as_object() {
                    for (check_name, check_data) in checks {
                        if check_data["pass"].as_bool() == Some(false) {
                            let label = check_data["label"].as_str().unwrap_or(check_name);
                            let value = check_data["value"].as_str().unwrap_or("");
                            let current = if value.is_empty() { "check failed".to_string() } else { value.to_string() };
                            suggestions.push(serde_json::json!({
                                "category": check_name,
                                "current": current,
                                "proposed": format!("Fix: {}", label),
                                "reason": label,
                                "priority": "high"
                            }));
                        }
                    }
                }
                // Also include quality critical issues if present
                if let Some(critical) = audit["quality_critical"].as_array() {
                    for issue in critical {
                        if let Some(text) = issue.as_str() {
                            suggestions.push(serde_json::json!({
                                "category": "quality_critical",
                                "current": "quality issue",
                                "proposed": format!("Fix: {}", text),
                                "reason": text,
                                "priority": "high"
                            }));
                        }
                    }
                }
            }
            // Fallback to at least one generic suggestion if audit had no failed checks
            if suggestions.is_empty() {
                suggestions.push(serde_json::json!({
                    "category": "content_depth",
                    "current": "content flagged as poor",
                    "proposed": "Improve depth, structure, and keyword usage",
                    "reason": "Content audit health = poor but no specific check failures were recorded",
                    "priority": "medium"
                }));
            }

            let rec_key = format!("recommendations_{}", article_id);
            let rec_content = serde_json::json!({
                "article_id": article_id,
                "article_file": &ctx.target.file,
                "article_title": &ctx.target.title,
                "target_keyword": &ctx.target.target_keyword,
                "suggestions": suggestions
            });
            artifacts.push(crate::models::task::TaskArtifact {
                key: rec_key,
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("indexing_health_campaign".to_string()),
                content: Some(rec_content.to_string()),
            });
        }
    }

    fix_task_spec(
        parent,
        "fix_content_article",
        format!("Fix content: {}", url_slug),
        format!(
            "URL: {}\nRecommended action: fix_content (content audit health = poor)\nParent campaign: {}",
            target.url, parent.id
        ),
        idempotency_key,
        artifacts,
    )
}

pub(crate) fn build_add_links_spec(
    parent: &Task,
    target: &IndexingTargetPlan,
    ctx: Option<&IndexingTargetContext>,
) -> TaskSpec {
    let url_slug = crate::content::slug::extract_slug_from_url(&target.url);
    // Use article_id (not parent.id) so dedup works across repeated campaign runs.
    let article_id = ctx.map(|c| c.target.article_id).unwrap_or(0);
    let idempotency_key = format!("ihc-add-links:{}:{}", parent.project_id, article_id);

    // Build the indexing_link_target artifact that fix_indexing_internal_links expects
    let mut artifacts = vec![];
    if let Some(ctx) = ctx {
        let source_candidates_json: Vec<serde_json::Value> = ctx
            .source_candidates
            .iter()
            .map(|s| {
                serde_json::json!({
                    "article_id": s.article_id,
                    "slug": &s.slug,
                    "title": &s.title,
                    "file": &s.file,
                    "reason": &s.reason,
                })
            })
            .collect();

        let artifact_content = serde_json::json!({
            "campaign_task_id": &parent.id,
            "target": {
                "url": &ctx.target.url,
                "slug": &ctx.target.slug,
                "article_id": ctx.target.article_id,
                "file": &ctx.target.file,
                "reason_code": &ctx.target.reason_code,
                "incoming_link_count_before": ctx.target.incoming_links,
                "target_keyword": &ctx.target.target_keyword,
                "source_candidates": source_candidates_json,
            }
        });

        artifacts.push(crate::models::task::TaskArtifact {
            key: "indexing_link_target".to_string(),
            path: None,
            artifact_type: Some("indexing_link_target".to_string()),
            source: Some("indexing_health_campaign".to_string()),
            content: Some(artifact_content.to_string()),
        });
    }

    fix_task_spec(
        parent,
        "fix_indexing_internal_links",
        format!("Add links: {}", url_slug),
        format!(
            "URL: {}\nRecommended action: add_links (zero incoming internal links)\nParent campaign: {}",
            target.url, parent.id
        ),
        idempotency_key,
        artifacts,
    )
}

pub(crate) fn build_rewrite_spec(
    parent: &Task,
    target: &IndexingTargetPlan,
    ctx: Option<&IndexingTargetContext>,
) -> TaskSpec {
    let url_slug = crate::content::slug::extract_slug_from_url(&target.url);
    // Use article_id (not parent.id) so dedup works across repeated campaign runs.
    let article_id = ctx.map(|c| c.target.article_id).unwrap_or(0);
    let idempotency_key = format!("ihc-rewrite:{}:{}", parent.project_id, article_id);

    // Build a richer description that includes cluster context if available
    let mut description = format!(
        "URL: {}\nRecommended action: rewrite_title_h1\nReason: {}\nParent campaign: {}",
        target.url,
        target
            .distinctiveness_verdict
            .as_ref()
            .map(|v| v.reason.clone())
            .unwrap_or_default(),
        parent.id
    );

    if let Some(v) = &target.distinctiveness_verdict {
        if let Some(title) = &v.suggested_title {
            description.push_str(&format!("\nSuggested title: {}", title));
        }
        if let Some(h1) = &v.suggested_h1 {
            description.push_str(&format!("\nSuggested H1: {}", h1));
        }
    }

    // Build cluster context artifact for the agent
    let mut artifacts = vec![];
    if let Some(ctx) = ctx {
        if let Ok(json) = serde_json::to_string_pretty(ctx) {
            artifacts.push(crate::models::task::TaskArtifact {
                key: "indexing_target_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("indexing_health_campaign".to_string()),
                content: Some(json),
            });
        }
    }

    fix_task_spec(
        parent,
        "fix_indexing",
        format!("Rewrite title/H1: {}", url_slug),
        description,
        idempotency_key,
        artifacts,
    )
}

