/// Reddit search and enrichment execution module.
///
/// Covers:
///   - exec_reddit_config_parse   (agentic: parse reddit_config.md → structured JSON)
///   - exec_reddit_search         (deterministic API search + scoring)
///   - persist_reddit_opportunities (upsert enriched opportunities to SQLite)
///   - exec_reddit_enrich         (AI pass: fill why_relevant + draft reply)
///   - extract_trigger_topics     (parse reddit_config.md)
///   - extract_seed_subreddits
///   - extract_query_keywords
///   - extract_excluded_subreddits
///   - compute_scores
///   - extract_json_array
use crate::models::task::Task;

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
pub use enrich::{PersistOutcome, persist_reddit_opportunities};
pub use reply::exec_reddit_post_reply;

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

/// Parse config directly as fallback when no artifact is available.
pub(crate) fn parse_config_fallback(config: &str) -> RedditSearchParams {
    let queries = {
        let kw = extract_query_keywords(config);
        if kw.is_empty() {
            extract_trigger_topics(config, 10)
        } else {
            kw
        }
    };

    let seed_subs = extract_seed_subreddits(config);
    let excluded: Vec<String> = extract_excluded_subreddits(config).into_iter().collect();
    let cfg = crate::reddit::config::parse_reddit_config(config);

    RedditSearchParams {
        product_name: cfg.product_name,
        mention_stance: cfg.mention_stance.as_str().to_string(),
        trigger_topics: extract_trigger_topics(config, 10),
        query_keywords: queries,
        seed_subreddits: seed_subs,
        excluded_subreddits: excluded,
        user_context: None,
    }
}
