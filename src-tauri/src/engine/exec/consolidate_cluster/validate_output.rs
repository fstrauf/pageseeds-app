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
// Step 7: Validate Output
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate the merged keeper and redirect map.
pub(crate) fn exec_merge_validate_output(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

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
    let keeper_slug = keep_url
        .trim_start_matches("/blog/")
        .trim_start_matches('/');
    let keeper_file = find_file_by_slug(project_path, keeper_slug);

    let mut issues: Vec<String> = Vec::new();

    let keeper_valid = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|content| {
            let validation = crate::content::cleaner::validate_mdx_structure(&content);
            if let Err(e) = &validation {
                issues.push(format!("keeper: {}", e));
            }
            validation.is_ok()
        })
        .unwrap_or_else(|| {
            issues.push("Keeper file not found".to_string());
            false
        });

    let word_count = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|c| crate::content::ops::count_words(&c))
        .unwrap_or(0);

    let csv_path = paths.automation_dir.join("redirects.csv");
    let has_redirect_map = csv_path.exists();
    if !has_redirect_map {
        issues.push("No redirects.csv found".to_string());
    }

    let report = MergeValidationReport {
        keeper_valid,
        keeper_word_count: word_count,
        redirect_map_path: Some(csv_path.to_string_lossy().to_string()),
        issues: issues.clone(),
    };

    let all_ok = keeper_valid && has_redirect_map && issues.is_empty();

    StepResult {
        success: all_ok,
        message: if all_ok {
            "Merge validation passed".to_string()
        } else {
            format!("Merge validation found {} issues", issues.len())
        },
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}

