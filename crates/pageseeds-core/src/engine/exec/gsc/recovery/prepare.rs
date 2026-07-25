use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Freshness defaults ───────────────────────────────────────────────────────

const MAX_GSC_AGE_HOURS: u64 = 24;
const MAX_LINK_SCAN_AGE_HOURS: u64 = 24;
const DEFAULT_SITEMAP_LIMIT: usize = 200;

// ─── Prepare ──────────────────────────────────────────────────────────────────

/// Check data freshness and refresh link scan when stale.
/// GSC collection refresh is attempted via the existing collect helper when
/// a token is available; if not, the step warns but does not fail so that
/// planning can fall back to sitemap-only mode.
pub(crate) fn exec_gsc_recovery_prepare(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let mut messages: Vec<String> = Vec::new();

    // 1. Check GSC collection freshness
    let gsc_collection_path = paths.automation_dir.join("gsc_collection.json");
    let gsc_age = file_age_hours(&gsc_collection_path);
    let gsc_fresh = gsc_age.map(|h| h < MAX_GSC_AGE_HOURS).unwrap_or(false);

    if gsc_fresh {
        messages.push(format!("GSC data fresh ({}h old)", gsc_age.unwrap_or(0)));
    } else if gsc_collection_path.exists() {
        messages.push(format!(
            "GSC data stale ({}h old) — will use available data or refresh if possible",
            gsc_age.unwrap_or(999)
        ));
    } else {
        messages.push(
            "GSC data missing — will use sitemap-only mode or refresh if possible".to_string(),
        );
    }

    // 2. Refresh link scan if stale or missing
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_age = file_age_hours(&link_scan_path);
    let link_fresh = link_age
        .map(|h| h < MAX_LINK_SCAN_AGE_HOURS)
        .unwrap_or(false);

    if !link_fresh || !link_scan_path.exists() {
        messages.push(format!(
            "Link scan {} — refreshing",
            if link_scan_path.exists() {
                format!("stale ({}h old)", link_age.unwrap_or(999))
            } else {
                "missing".to_string()
            }
        ));
        match refresh_link_scan(&paths, &task.project_id) {
            Ok(summary) => messages.push(summary),
            Err(e) => {
                return StepResult::fail(format!("Failed to refresh link scan: {}", e));
            }
        }
    } else {
        messages.push(format!("Link scan fresh ({}h old)", link_age.unwrap_or(0)));
    }

    // 3. Optionally refresh GSC data if stale and token available
    // For V1, we call the existing collect helper when the token is present.
    // This avoids duplicating the auth + inspect pipeline.
    if !gsc_fresh && gsc_token.is_some() {
        messages.push("Attempting GSC refresh via existing collection helper…".to_string());
        let collect_result =
            crate::engine::exec::gsc::exec_collect_gsc(task, project_path, gsc_token);
        if collect_result.success {
            messages.push("GSC refresh succeeded".to_string());
        } else {
            messages.push(format!(
                "GSC refresh failed: {} — continuing with cached data",
                collect_result.message
            ));
        }
    }

    let freshness = serde_json::json!({
        "gsc_data_age_hours": gsc_age,
        "gsc_data_fresh": gsc_fresh,
        "link_scan_age_hours": link_age,
        "link_scan_fresh": link_fresh,
        "partial_gsc_collection": false,
    });

    StepResult {
        success: true,
        message: messages.join(". "),
        output: Some(freshness.to_string()),
        artifact_key: None,
    }
}
