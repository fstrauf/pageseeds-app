use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::task::Task;
use crate::social::content::sources::discover_sources;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 0: Extract Article (for social_generate_from_article workflow)
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_extract_article(task: &Task, project_path: &str) -> StepResult {
    // For now, this is a pass-through step that discovers the article source
    // The actual article extraction happens in social_generate_posts which rediscovers sources
    let config = super::parse_source_config_from_task(task);
    let manifest = match discover_sources(Path::new(project_path), &config) {
        Ok(m) => m,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to discover article source: {}", e),
                output: None,
            };
        }
    };

    if manifest.is_empty() {
        return StepResult {
            success: false,
            message: "No article source found. Check your source configuration.".to_string(),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!("Extracted {} article sources", manifest.articles.len()),
        output: Some(format!("{{\"article_count\":{}}}", manifest.articles.len())),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Collect Content Sources
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_collect_sources(
    task: &Task,
    project_path: &str,
) -> StepResult {
    // Parse source config from task description
    let config = super::parse_source_config_from_task(task);

    log::info!("[social_collect_sources] discovering sources for project {}", task.project_id);
    log::info!("[social_collect_sources] project_path: {}", project_path);
    log::info!("[social_collect_sources] config: articles={}, screenshots={}, specs={}", 
        config.include_articles, config.include_screenshots, config.include_specs);
    log::debug!("[social_collect_sources] task description: {:?}", task.description);

    let manifest = match discover_sources(Path::new(project_path), &config) {
        Ok(m) => m,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to discover sources: {}", e),
                output: None,
            };
        }
    };

    let total = manifest.total_sources();
    log::info!("[social_collect_sources] found {} sources ({} articles, {} screenshots, {} specs)",
        total,
        manifest.articles.len(),
        manifest.screenshots.len(),
        manifest.specs.len()
    );

    if total == 0 {
        return StepResult {
            success: false,
            message: "No content sources found. Check your source configuration.".to_string(),
            output: None,
        };
    }

    // For now, we don't serialize the full manifest (contains PathBuf)
    // Instead, we'll rediscover sources in the next step
    // TODO: Create a serializable manifest structure

    StepResult {
        success: true,
        message: format!("Discovered {} content sources", total),
        output: Some(format!("{{\"total\":{}}}", total)),
    }
}
