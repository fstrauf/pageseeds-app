use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Post-action helper ───────────────────────────────────────────────────────

/// Spawn child `fix_indexing_internal_links` tasks from a recovery plan.
/// Called by post_actions.rs after gsc_indexing_recovery completes.
pub(crate) fn spawn_recovery_child_tasks(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};

    let paths = ProjectPaths::from_path(project_path);
    let plan_path = paths.automation_dir.join("gsc_recovery_plan.json");

    let plan: RecoveryPlan = match std::fs::read_to_string(&plan_path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("[recovery_post_action] failed to parse plan: {}", e);
                return vec![];
            }
        },
        Err(e) => {
            log::warn!("[recovery_post_action] plan file not found: {}", e);
            return vec![];
        }
    };

    let mut created_ids: Vec<String> = Vec::new();

    for target in &plan.targets {
        // Article-scoped key — same as IHC and operator slug spawn so all paths dedupe.
        let idempotency_key =
            crate::engine::indexing_link_fix::fix_indexing_internal_links_idempotency_key(
                &parent_task.project_id,
                target.article_id,
            );

        let source_candidates = target
            .source_candidates
            .iter()
            .map(|s| crate::models::indexing_health::LinkSourceCandidate {
                article_id: s.article_id,
                slug: s.slug.clone(),
                title: s.title.clone(),
                file: s.file.clone(),
                reason: s.reason.clone(),
            })
            .collect();

        let target_artifact = crate::engine::indexing_link_fix::indexing_link_target_artifact(
            Some(parent_task.id.clone()),
            crate::models::indexing_health::IndexingLinkTarget {
                url: target.url.clone(),
                slug: target.slug.clone(),
                article_id: target.article_id,
                file: target.file.clone(),
                reason_code: target.reason_code.clone(),
                incoming_link_count_before: target.incoming_link_count_before,
                target_keyword: target.target_keyword.clone(),
                source_candidates,
            },
            "gsc_recovery_plan",
        );

        let priority = if target.priority_score >= 100 {
            Priority::High
        } else {
            Priority::Medium
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_indexing_internal_links".to_string(),
            title: Some(format!(
                "Fix links for {} ({})",
                target.slug, target.reason_code
            )),
            description: Some(format!(
                "Add inbound internal links to {}. Reason: {}. Baseline incoming: {}.",
                target.url, target.priority_reason, target.incoming_link_count_before
            )),
            run_policy: Some(TaskRunPolicy::AutoEnqueue),
            agent_policy: AgentPolicy::Required,
            priority,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![target_artifact],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 14 }),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[recovery_post_action] spawned child task {} for {}",
                    task.id,
                    target.url
                );
                // Record in recovery history
                let _ = crate::gsc::db::insert_recovery_history(
                    conn,
                    &parent_task.project_id,
                    &target.url,
                    Some(target.article_id),
                    &parent_task.id,
                    &task.id,
                    &target.reason_code,
                    target.incoming_link_count_before as i64,
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!(
                    "[recovery_post_action] failed to spawn task for {}: {}",
                    target.url,
                    e
                );
            }
        }
    }

    log::info!(
        "[recovery_post_action] created {} child tasks from plan",
        created_ids.len()
    );
    created_ids
}

