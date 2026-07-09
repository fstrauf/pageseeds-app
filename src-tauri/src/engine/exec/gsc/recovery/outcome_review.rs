use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Outcome Review (Phase 2) ─────────────────────────────────────────────────

/// Re-inspect a target URL in GSC after a wait period.
/// Reads the target URL from the task's indexing_link_target artifact.
pub(crate) fn exec_gsc_indexing_outcome_inspect(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Extract target URL from artifact
    let target_url = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_link_target")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v["target"]["url"].as_str().map(String::from));

    let target_url = match target_url {
        Some(u) => u,
        None => {
            return StepResult {
                success: false,
                message: "No target URL found in indexing_link_target artifact".to_string(),
                output: None,
            }
        }
    };

    // Load previous status from gsc_recovery_plan or outcome baseline
    let baseline_path = paths
        .automation_dir
        .join("gsc_indexing_outcome_baseline.json");
    let baseline: Option<serde_json::Value> = std::fs::read_to_string(&baseline_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let previous_reason = baseline
        .as_ref()
        .and_then(|v| v["reason_code"].as_str())
        .unwrap_or("unknown");

    // If no token, we can't inspect — return pending
    let token = match gsc_token {
        Some(t) => t.to_string(),
        None => {
            return StepResult {
                success: true,
                message: "No GSC token available — outcome inspection deferred".to_string(),
                output: Some(
                    serde_json::json!({
                        "target_url": target_url,
                        "status": "deferred",
                        "previous_reason": previous_reason,
                    })
                    .to_string(),
                ),
            }
        }
    };

    // Resolve site_url (GSC property) from manifest
    let site_url = resolve_site_url(project_path);

    // Inspect the URL
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to create tokio runtime: {}", e),
                output: None,
            }
        }
    };

    let inspect_result = rt.block_on(async {
        crate::gsc::indexing::inspect_batch(&token, &site_url, vec![target_url.clone()]).await
    });

    match inspect_result {
        Ok(records) => {
            let record = records.first();
            let current_reason = record
                .and_then(|r| r.reason_code.as_deref())
                .unwrap_or("unknown");
            let current_verdict = record
                .and_then(|r| r.verdict.as_deref())
                .unwrap_or("unknown");

            let outcome = serde_json::json!({
                "target_url": target_url,
                "previous_reason": previous_reason,
                "current_reason": current_reason,
                "current_verdict": current_verdict,
                "inspected_at": chrono::Utc::now().to_rfc3339(),
            });

            StepResult {
                success: true,
                message: format!(
                    "Re-inspected {}: {} → {}",
                    target_url, previous_reason, current_reason
                ),
                output: Some(outcome.to_string()),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("URL Inspection API failed: {}", e),
            output: None,
        },
    }
}

/// Compare before/after indexing status and write a structured outcome report.
pub(crate) fn exec_gsc_indexing_outcome_report(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load the inspect output from the previous step (stored as artifact or on disk)
    let inspect_path = paths
        .automation_dir
        .join(format!("gsc_outcome_inspect_{}.json", task.id));
    let inspect_data: Option<serde_json::Value> = std::fs::read_to_string(&inspect_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .or_else(|| {
            // Fallback: try to read from task artifacts
            task.artifacts
                .iter()
                .find(|a| a.key == "gsc_indexing_outcome_inspect")
                .and_then(|a| a.content.as_ref())
                .and_then(|c| serde_json::from_str(c).ok())
        });

    let (target_url, previous_reason, current_reason) = inspect_data
        .as_ref()
        .map(|v| {
            (
                v["target_url"].as_str().unwrap_or("").to_string(),
                v["previous_reason"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                v["current_reason"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })
        .unwrap_or_default();

    let outcome_status = if current_reason == "indexed_pass" {
        "resolved"
    } else if current_reason == previous_reason {
        "still_not_indexed"
    } else if current_reason == "unknown" {
        "unknown"
    } else {
        "regressed"
    };

    let report = serde_json::json!({
        "target_url": target_url,
        "previous_reason": previous_reason,
        "current_reason": current_reason,
        "outcome_status": outcome_status,
        "reported_at": chrono::Utc::now().to_rfc3339(),
        "campaign_task_id": task.artifacts.iter().find(|a| a.key == "indexing_link_target")
            .and_then(|a| a.content.as_ref())
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .and_then(|v| v["campaign_task_id"].as_str().map(String::from)),
    });

    let report_path = paths
        .automation_dir
        .join(format!("gsc_outcome_report_{}.json", task.id));
    let _ = std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    );

    StepResult {
        success: true,
        message: format!("Outcome report for {}: {}", target_url, outcome_status),
        output: Some(report.to_string()),
    }
}

