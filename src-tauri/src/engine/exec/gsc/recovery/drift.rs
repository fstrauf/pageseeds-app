use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Drift ────────────────────────────────────────────────────────────────────

/// Reuse the existing drift computation and return it as a StepResult.
/// Writes gsc_recovery_drift.json to the automation dir so the plan step
/// can read it without re-running the full drift query.
pub(crate) fn exec_gsc_recovery_drift(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

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

    let report: GscDriftReport = match rt.block_on(async {
        crate::engine::exec::gsc::exec_gsc_drift(&task.project_id, project_path).await
    }) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Drift computation failed: {}", e),
                output: None,
            }
        }
    };

    // Persist drift report for plan step
    let drift_path = paths.automation_dir.join("gsc_recovery_drift.json");
    let _ = std::fs::create_dir_all(&paths.automation_dir);
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(&drift_path, json);
    }

    StepResult {
        success: true,
        message: format!(
            "Drift: {} indexed, {} not indexed, {} missing from GSC, {} orphans in priority list",
            report.indexed_count,
            report.not_indexed_count,
            report.in_sitemap_not_in_gsc.len(),
            report
                .resubmit_priority
                .iter()
                .filter(|c| !c.has_internal_links)
                .count(),
        ),
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}
