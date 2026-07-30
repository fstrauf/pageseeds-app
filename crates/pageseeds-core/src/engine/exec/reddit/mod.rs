/// Reddit search and enrichment execution module.
///
/// Covers:
///   - exec_reddit_config_parse   (deterministic: ProjectConfig/YAML → structured JSON)
///   - exec_reddit_search         (deterministic API search + scoring)
///   - persist_reddit_opportunities (upsert enriched opportunities to SQLite)
///   - exec_reddit_enrich         (AI pass: fill why_relevant + draft reply)
///   - compute_scores
use crate::models::task::Task;
use std::path::Path;

mod config;
mod enrich;
mod reply;
mod search;

pub(crate) use config::*;
pub(crate) use enrich::*;
pub(crate) use reply::*;
pub(crate) use search::*;

// Public re-exports for integration tests
pub use config::exec_reddit_config_parse;
pub use config::reddit_search_params_from_config;
pub use enrich::{PersistOutcome, persist_reddit_opportunities};
pub use reply::exec_reddit_post_reply;

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod persist_tests;

/// Load structured search params from the reddit_config_parse_stage artifact.
/// Returns None if no artifact found or parsing fails.
pub(crate) fn load_search_params_from_artifact(
    task: &Task,
    _project_path: &str,
) -> Option<RedditSearchParams> {
    // Look for artifact from reddit_config_parse_stage
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "reddit_config_parse_stage")?;
    let content = artifact.content.as_ref()?;

    log::info!(
        "[reddit_search] found structured params artifact ({} chars)",
        content.len()
    );

    // Try to parse as RedditSearchParams
    match serde_json::from_str::<RedditSearchParams>(content) {
        Ok(params) => {
            log::info!(
                "[reddit_search] loaded params: {} keywords, {} topics, {} subreddits",
                params.query_keywords.len(),
                params.trigger_topics.len(),
                params.seed_subreddits.len()
            );
            Some(params)
        }
        Err(e) => {
            log::warn!(
                "[reddit_search] failed to parse artifact as RedditSearchParams: {}",
                e
            );
            None
        }
    }
}

/// Load structured search params from `project.yaml` via ensure + mapper.
///
/// Used when the config-parse stage artifact is missing (backward compat).
/// Does **not** re-parse live MD for structured knobs.
pub(crate) fn load_search_params_from_project_config(
    project_path: &str,
    user_context: Option<String>,
) -> Result<RedditSearchParams, String> {
    let automation_dir = Path::new(project_path).join(".github").join("automation");
    let (config, _) = crate::project_config::ensure_project_config(&automation_dir)
        .map_err(|e| format!("project config unavailable: {e}"))?;
    Ok(reddit_search_params_from_config(&config, user_context))
}

/// Resolve search params: config-parse artifact first, then ProjectConfig/YAML.
///
/// Shared by search and enrich so both use the same fallback chain.
/// When the artifact is present but lacks `user_context` (older artifacts),
/// fills it from the task description.
pub(crate) fn resolve_search_params(
    task: &Task,
    project_path: &str,
) -> Result<RedditSearchParams, String> {
    if let Some(mut params) = load_search_params_from_artifact(task, project_path) {
        if params.user_context.is_none() {
            params.user_context = extract_user_context_from_description(task);
        }
        return Ok(params);
    }
    log::info!("[reddit] no structured params artifact, falling back to ProjectConfig");
    load_search_params_from_project_config(
        project_path,
        extract_user_context_from_description(task),
    )
}
