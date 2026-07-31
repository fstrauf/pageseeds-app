use crate::engine::workflows::StepResult;
use crate::models::merge_patch::RedirectRule;
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 6: Generate Redirects
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate redirect rules as generic CSV.
pub(crate) fn exec_merge_generate_redirects(task: &Task, project_path: &str) -> StepResult {
    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult::fail(format!("Invalid merge plan JSON: {}", e));
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("").to_string();
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
            destination: keep_url.clone(),
            status: 301,
        })
        .collect();

    let project = std::path::Path::new(project_path);
    let csv_path = match crate::engine::merge_apply::upsert_redirects_csv(
        project,
        &keep_url,
        &redirect_urls,
    ) {
        Ok(p) => p,
        Err(e) => return StepResult::fail(e),
    };
    let redirects_warning =
        crate::engine::merge_apply::redirects_gitignore_warning(project);

    let mut output = serde_json::json!({
        "rules": rules,
        "csv_path": csv_path.to_string_lossy().to_string(),
        "count": rules.len(),
    });
    if let Some(ref warn) = redirects_warning {
        output["redirects_warning"] = serde_json::json!(warn);
    }

    let mut message = format!(
        "Generated {} redirect rules -> {}",
        rules.len(),
        csv_path.display()
    );
    if let Some(ref warn) = redirects_warning {
        message.push_str(" — ");
        message.push_str(warn);
    }

    StepResult {
        success: true,
        message,
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
        artifact_key: None,
    }
}
