/// Deterministic context builder for the content fix pipeline.
///
/// 1. Reads recommendations.json from the automation dir.
/// 2. Finds the article's recommendations by article_id (from task artifact).
/// 3. Reads the current MDX file.
/// 4. Builds a structured context JSON consumed by the generate step.
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;
use super::fix_generate::normalize_target_keyword;

pub(crate) fn exec_fix_content_article_context(
    task: &Task,
    project_path: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Resolve article_id from task artifacts
    let article_id = match super::fix_content_article_id(task) {
        Some(id) => id,
        None => {
            return StepResult {
                success: false,
                message: "No article_id found in task artifacts".to_string(),
                output: None,
            };
        }
    };

    // Try to load recommendations from task artifact first (self-contained),
    // fall back to recommendations.json on disk.
    let article_rec = task
        .artifacts
        .iter()
        .find(|a| a.key.starts_with("recommendations_"))
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .or_else(|| {
            let rec_path = paths.automation_dir.join("recommendations.json");
            std::fs::read_to_string(&rec_path).ok().and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s).ok().and_then(|rec| {
                    rec["articles"].as_array().and_then(|articles| {
                        articles
                            .iter()
                            .find(|a| {
                                a["article_id"]
                                    .as_i64()
                                    .or_else(|| a["article_id"].as_str().and_then(|s| s.parse().ok()))
                                    == Some(article_id)
                            })
                            .cloned()
                    })
                })
            })
        });

    let article_rec = match article_rec {
        Some(a) => a,
        None => {
            return StepResult {
                success: false,
                message: format!(
                    "Article {} not found in task artifacts or recommendations.json",
                    article_id
                ),
                output: None,
            };
        }
    };

    let file = article_rec["article_file"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let article_title = article_rec["article_title"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let target_keyword = article_rec["target_keyword"]
        .as_str()
        .map(|s| normalize_target_keyword(s, article_id));
    let suggestions = article_rec["suggestions"].clone();

    // Read current file content
    let file_path = match crate::engine::exec::audit_health::resolve_content_file(repo_root, &file) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!(
                    "File not found: {}. Run sanitize_content to repair paths.",
                    file
                ),
                output: None,
            };
        }
    };

    let file_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read file {}: {}", file_path.display(), e),
                output: None,
            };
        }
    };

    // Build structured context (lightweight — generate step reads the file itself)
    let context = serde_json::json!({
        "article_id": article_id,
        "article_title": article_title,
        "article_file": file,
        "target_keyword": target_keyword,
        "suggestions": suggestions,
    });

    let context_json = match serde_json::to_string_pretty(&context) {
        Ok(s) => s,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize context: {}", e),
                output: None,
            };
        }
    };

    StepResult {
        success: true,
        message: format!(
            "Built fix context for article {} ({} suggestions, {} chars content)",
            article_id,
            suggestions.as_array().map(|a| a.len()).unwrap_or(0),
            file_content.len()
        ),
        output: Some(context_json),
    }
}
