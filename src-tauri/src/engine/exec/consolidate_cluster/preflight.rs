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
// Step 2: Preflight
// ═══════════════════════════════════════════════════════════════════════════════

/// Run preflight checks before merging.
pub(crate) fn exec_merge_preflight(
    task: &Task,
    project_path: &str,
    _plan_json: &str,
) -> StepResult {
    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid merge plan JSON: {}", e),
                output: None,
            };
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    // Resolve keeper file from URL slug
    let keeper_slug = keep_url
        .trim_start_matches("/blog/")
        .trim_start_matches('/');
    let keeper_file = match find_file_by_slug(project_path, keeper_slug) {
        Ok(f) => f,
        Err(e) => {
            return StepResult {
                success: false,
                message: e,
                output: None,
            };
        }
    };
    let keeper_exists = keeper_file.as_ref().map(|p| p.exists()).unwrap_or(false);

    // Check keeper is indexable (no noindex in frontmatter)
    let keeper_indexable = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|content| !content.to_lowercase().contains("noindex"))
        .unwrap_or(false);

    // Check redirect files exist
    let mut redirect_files_exist = Vec::new();
    let mut redirect_files_missing = Vec::new();
    let mut redirect_cycles = Vec::new();

    for url in &redirect_urls {
        let slug = url.trim_start_matches("/blog/").trim_start_matches('/');
        if slug == keeper_slug {
            redirect_cycles.push(url.clone());
            continue;
        }
        match find_file_by_slug(project_path, slug) {
            Ok(Some(p)) if p.exists() => redirect_files_exist.push(url.clone()),
            Ok(_) => redirect_files_missing.push(url.clone()),
            Err(e) => {
                return StepResult {
                    success: false,
                    message: e,
                    output: None,
                };
            }
        }
    }

    let can_proceed = keeper_exists
        && keeper_indexable
        && redirect_files_missing.is_empty()
        && redirect_cycles.is_empty();

    let report = MergePreflightReport {
        keeper_file_exists: keeper_exists,
        keeper_is_indexable: keeper_indexable,
        redirect_files_exist: redirect_files_exist.clone(),
        redirect_files_missing: redirect_files_missing.clone(),
        redirect_cycles_detected: redirect_cycles.clone(),
        can_proceed,
        notes: vec![],
    };

    let output = serde_json::to_string_pretty(&report).unwrap_or_default();
    StepResult {
        success: can_proceed,
        message: if can_proceed {
            "Preflight passed: all files exist, no cycles detected".to_string()
        } else {
            format!(
                "Preflight failed: keeper_exists={}, keeper_indexable={}, missing={:?}, cycles={:?}",
                keeper_exists, keeper_indexable, redirect_files_missing, redirect_cycles
            )
        },
        output: Some(output),
    }
}

