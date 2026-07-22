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
// Step 6: Generate Redirects
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate redirect rules as generic CSV.
pub(crate) fn exec_merge_generate_redirects(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult::fail(format!("Invalid merge plan JSON: {}", e));
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let rules: Vec<RedirectRule> = redirect_urls
        .iter()
        .map(|source| RedirectRule {
            source: source.clone(),
            destination: keep_url.to_string(),
            status: 301,
        })
        .collect();

    // Merge with existing redirects.csv (append, no duplicates)
    let csv_path = paths.automation_dir.join("redirects.csv");
    let mut existing_rules: std::collections::HashMap<String, (String, i32)> =
        std::collections::HashMap::new();

    if let Ok(existing) = std::fs::read_to_string(&csv_path) {
        for line in existing.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                if let Ok(status) = parts[2].trim().parse::<i32>() {
                    existing_rules.insert(
                        parts[0].trim().to_string(),
                        (parts[1].trim().to_string(), status),
                    );
                }
            }
        }
    }

    for rule in &rules {
        existing_rules.insert(
            rule.source.clone(),
            (rule.destination.clone(), rule.status as i32),
        );
    }

    let mut csv = String::from("source,destination,status\n");
    for (source, (destination, status)) in &existing_rules {
        csv.push_str(&format!("{},{},{}\n", source, destination, status));
    }

    if let Err(e) = std::fs::write(&csv_path, &csv) {
        return StepResult::fail(format!("Failed to write redirects.csv: {}", e));
    }

    let output = serde_json::json!({
        "rules": rules,
        "csv_path": csv_path.to_string_lossy().to_string(),
        "count": rules.len(),
    });

    StepResult {
        success: true,
        message: format!(
            "Generated {} redirect rules -> {}",
            rules.len(),
            csv_path.display()
        ),
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
        artifact_key: None,
    }
}

