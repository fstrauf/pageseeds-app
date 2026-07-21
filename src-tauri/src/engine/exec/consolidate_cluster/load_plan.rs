use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Load Plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Load the approved merge plan for this cluster from the strategy artifact.
pub(crate) fn exec_merge_load_plan(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Extract cluster_id from task title (e.g. "Merge cluster: cash_secured_puts")
    let cluster_id = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .unwrap_or("")
        .trim();

    if cluster_id.is_empty() {
        return StepResult::fail("Cannot determine cluster_id from task title".to_string());
    }

    // Find strategy artifact on task
    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        // Fallback: read from automation dir
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() {
        return StepResult::fail("No cannibalization_strategy artifact found".to_string());
    }

    let strategy: serde_json::Value = match serde_json::from_str(&strategy_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult::fail(format!("Invalid strategy JSON: {}", e));
        }
    };

    let recommendations = strategy["merge_recommendations"].as_array();
    let rec = recommendations.and_then(|arr| {
        arr.iter()
            .find(|r| r["cluster_id"].as_str().unwrap_or("") == cluster_id)
    });

    let rec = match rec {
        Some(r) => r.clone(),
        None => {
            return StepResult::fail(format!("No merge recommendation found for cluster '{}'", cluster_id));
        }
    };

    let output = serde_json::to_string_pretty(&rec).unwrap_or_default();
    StepResult {
        success: true,
        message: format!("Loaded merge plan for cluster: {}", cluster_id),
        output: Some(output),
    }
}

