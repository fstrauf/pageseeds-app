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
// Step 1: Check Prerequisites
// ═══════════════════════════════════════════════════════════════════════════════

/// Check freshness of prerequisite artifacts.
/// If any auto-runnable prerequisite is stale, spawns the helper task
/// and returns failure so the parent task pauses until prerequisites
/// are satisfied. Re-run the parent after helpers complete.
pub(crate) fn exec_ihc_check_prerequisites(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let checks = vec![
        check_gsc_collection_fresh(&paths),
        check_artifact(&paths, "link_scan.json", chrono::Duration::days(7)),
        check_artifact(&paths, "content_audit.json", chrono::Duration::days(14)),
    ];

    // Clusters are a nice-to-have, not a blocker. The campaign runs fine without them.
    let cluster_check = check_artifact(&paths, "cannibalization_clusters.json", chrono::Duration::days(30));

    let all_fresh = checks.iter().all(|c| c.fresh);
    let report = PrerequisiteReport {
        all_fresh,
        checks: {
            let mut c = checks.clone();
            c.push(cluster_check.clone());
            c
        },
    };

    let stale_auto: Vec<&PrerequisiteCheck> = checks
        .iter()
        .filter(|c| !c.fresh && c.action.as_deref().unwrap_or("").starts_with("auto_enqueue"))
        .collect();

    let stale_user: Vec<&PrerequisiteCheck> = checks
        .iter()
        .filter(|c| {
            !c.fresh
                && c.action
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("user_must_run")
        })
        .collect();

    // Spawn cluster refresh as a best-effort helper (don't block campaign on it)
    let mut cluster_helper: Option<(String, String, String)> = None;
    if !cluster_check.fresh {
        if let Ok(conn) = rusqlite::Connection::open(crate::db::default_db_path()) {
            let spec = crate::engine::spawner::TaskSpec {
                project_id: task.project_id.clone(),
                task_type: "cannibalization_audit".to_string(),
                title: Some("cannibalization audit (auto-refresh)".to_string()),
                description: Some("Auto-spawned by indexing_health_campaign because cannibalization_clusters.json was stale.".to_string()),
                run_policy: Some(crate::models::task::TaskRunPolicy::AutoEnqueue),
                priority: crate::models::task::Priority::High,
                agent_policy: crate::models::task::AgentPolicy::None,
                idempotency_key: Some(format!("auto-refresh:cannibalization_clusters:{}", task.project_id)),
                dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::SkipIfActive),
                depends_on: vec![],
                artifacts: vec![],
                ..Default::default()
            };
            match crate::engine::spawner::TaskSpawner::spawn(&conn, spec) {
                Ok(spawned) => {
                    let ty = spawned.task_type.clone();
                    let item = crate::models::queue::EnqueueItem {
                        task_id: spawned.id.clone(),
                        project_id: spawned.project_id,
                        title: spawned.title.clone(),
                        task_type: Some(ty),
                        project_name: None,
                    };
                    if let Err(e) = crate::engine::queue::enqueue_tasks(&conn, vec![item], crate::models::queue::EnqueueMode::Append) {
                        log::warn!("[ihc] failed to enqueue cluster helper: {}", e);
                    }
                    cluster_helper = Some((spawned.id, spawned.task_type, spawned.status.to_string()));
                }
                Err(e) => log::warn!("[ihc] failed to spawn cluster helper: {}", e),
            }
        }
    }

    // Spawn auto-runnable helper tasks in the background and enqueue them
    let mut helpers: Vec<(String, String, String)> = vec![]; // (id, task_type, status)
    if !stale_auto.is_empty() {
        let db_path = crate::db::default_db_path();
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            for check in &stale_auto {
                let task_type = artifact_to_task_type(&check.artifact);
                let idempotency_key = format!(
                    "auto-refresh:{}:{}",
                    task_type, task.project_id
                );
                let spec = crate::engine::spawner::TaskSpec {
                    project_id: task.project_id.clone(),
                    task_type: task_type.to_string(),
                    title: Some(format!("{} (auto-refresh)", task_type.replace('_', " "))),
                    description: Some(format!(
                        "Auto-spawned by indexing_health_campaign because {} was stale.",
                        check.artifact
                    )),
                    run_policy: Some(crate::models::task::TaskRunPolicy::AutoEnqueue),
                    priority: crate::models::task::Priority::High,
                    agent_policy: crate::models::task::AgentPolicy::None,
                    idempotency_key: Some(idempotency_key),
                    dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::SkipIfActive),
                    depends_on: vec![],
                    artifacts: vec![],
                    ..Default::default()
                };
                match crate::engine::spawner::TaskSpawner::spawn(&conn, spec) {
                    Ok(spawned) => {
                        helpers.push((spawned.id.clone(), spawned.task_type.clone(), spawned.status.to_string()));
                        // Also enqueue to the active queue so it actually runs
                        let item = crate::models::queue::EnqueueItem {
                            task_id: spawned.id,
                            project_id: spawned.project_id,
                            title: spawned.title.clone(),
                            task_type: Some(spawned.task_type),
                            project_name: None,
                        };
                        if let Err(e) = crate::engine::queue::enqueue_tasks(&conn, vec![item], crate::models::queue::EnqueueMode::Append) {
                            log::warn!("[ihc] failed to enqueue helper {}: {}", task_type, e);
                        }
                    }
                    Err(e) => log::warn!("[ihc] failed to spawn {}: {}", task_type, e),
                }
            }
        }
    }

    let output = match serde_json::to_string_pretty(&report) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize prerequisite report: {}", e),
                output: None,
            }
        }
    };

    // Write report to disk for downstream steps
    let report_path = paths.automation_dir.join("indexing_prerequisites.json");
    let _ = std::fs::create_dir_all(&paths.automation_dir);
    let _ = std::fs::write(&report_path, &output);

    if !stale_user.is_empty() {
        let names: Vec<String> = stale_user.iter().map(|c| c.artifact.clone()).collect();
        return StepResult {
            success: false,
            message: format!(
                "User action required before campaign can run: {}",
                names.join(", ")
            ),
            output: Some(output),
        };
    }

    if !helpers.is_empty() {
        let helper_lines: Vec<String> = helpers
            .iter()
            .map(|(id, ty, status)| format!("  • {} ({}) — status: {}", id, ty, status))
            .collect();
        return StepResult {
            success: false,
            message: format!(
                "Waiting for {} helper task(s) to complete before campaign can run:\n{}",
                helpers.len(),
                helper_lines.join("\n")
            ),
            output: Some(output),
        };
    }

    let msg = match cluster_helper {
        Some((id, ty, status)) => format!(
            "All required prerequisites are fresh. Cluster data refresh running in background: {} ({}) — status: {}",
            id, ty, status
        ),
        None => "All prerequisite artifacts are fresh.".to_string(),
    };
    StepResult {
        success: true,
        message: msg,
        output: Some(output),
    }
}

pub(crate) fn artifact_to_task_type(artifact: &str) -> &str {
    match artifact {
        "gsc_collection.json" => "collect_gsc",
        "link_scan.json" => "cluster_and_link",
        "content_audit.json" => "content_audit",
        _ => artifact.trim_end_matches(".json"),
    }
}

/// Freshness check for the GSC prerequisite (issue #25).
///
/// `gsc_collection.json` (URL Inspection) is only half of `collect_gsc` — the
/// other half is the Search Analytics sync into `ctr_query_metrics`, which
/// `ctr_audit` / `cannibalization_audit` / `content_review` read. This check
/// therefore ALSO requires the `gsc_metrics_synced_at` marker written by the
/// sync, and fails closed when the marker is missing or older than 7 days,
/// even if the collection file itself is fresh. The artifact stays
/// `gsc_collection.json` so the failure action still maps to re-running
/// `collect_gsc` and the system self-heals.
pub(crate) fn check_gsc_collection_fresh(paths: &ProjectPaths) -> PrerequisiteCheck {
    let collection = check_artifact(paths, "gsc_collection.json", chrono::Duration::days(7));
    if !collection.fresh {
        return collection;
    }
    let marker = check_artifact(
        paths,
        crate::engine::exec::gsc::GSC_METRICS_SYNC_MARKER,
        chrono::Duration::days(crate::engine::exec::common::GSC_METRICS_MAX_AGE_DAYS),
    );
    if marker.fresh {
        return collection;
    }
    PrerequisiteCheck {
        artifact: "gsc_collection.json".to_string(),
        fresh: false,
        age_hours: marker.age_hours,
        action: Some("auto_enqueue_gsc_collection".to_string()),
    }
}

pub(crate) fn check_artifact(
    paths: &ProjectPaths,
    filename: &str,
    max_age: chrono::Duration,
) -> PrerequisiteCheck {
    let path = paths.automation_dir.join(filename);
    let (fresh, age_hours) = if path.exists() {
        match std::fs::metadata(&path) {
            Ok(meta) => match meta.modified() {
                Ok(modified) => match modified.elapsed() {
                    Ok(elapsed) => {
                        let hours = elapsed.as_secs() / 3600;
                        (hours < max_age.num_hours() as u64, Some(hours as i64))
                    }
                    Err(_) => (false, None),
                },
                Err(_) => (false, None),
            },
            Err(_) => (false, None),
        }
    } else {
        (false, None)
    };

    let action = if fresh {
        None
    } else {
        match filename {
            "cannibalization_strategy.json" => {
                Some("auto_enqueue_cannibalization_audit".to_string())
            }
            _ => Some(format!("auto_enqueue_{}", filename.trim_end_matches(".json"))),
        }
    };

    PrerequisiteCheck {
        artifact: filename.to_string(),
        fresh,
        age_hours,
        action,
    }
}
