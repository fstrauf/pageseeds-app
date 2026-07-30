use crate::models::task::Task;
use crate::project_config::{ensure_project_config, ProjectConfig};
use crate::reddit::config::MentionStance;
use std::path::Path;

// ─── Structured Config (from project.yaml via ensure) ─────────────────────────

/// Structured Reddit search parameters for the opportunity-search pipeline.
/// Produced by the deterministic `reddit_config_parse_stage` step from
/// [`ProjectConfig`] (YAML via [`ensure_project_config`]).
///
/// `mention_stance` is typed as [`MentionStance`]. Artifact JSON uses UPPERCASE
/// tokens (`"RECOMMENDED"`) for wire compatibility with historical artifacts and
/// DB consumers; deserialize also accepts snake_case (`"recommended"`).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct RedditSearchParams {
    pub product_name: Option<String>,
    #[serde(with = "mention_stance_artifact")]
    #[schemars(with = "String")]
    pub mention_stance: MentionStance,
    pub trigger_topics: Vec<String>,
    pub query_keywords: Vec<String>,
    pub seed_subreddits: Vec<String>,
    pub excluded_subreddits: Vec<String>,
    /// Free-form focus the user entered in the UI before starting the search.
    /// Injected from the task description — not stored in project.yaml.
    #[serde(default)]
    pub user_context: Option<String>,
}

/// Serde adapter: artifact/DB wire form is UPPERCASE via [`MentionStance::as_str`].
/// Accepts both UPPERCASE and snake_case on deserialize ([`MentionStance::from_str`]).
mod mention_stance_artifact {
    use crate::reddit::config::MentionStance;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(stance: &MentionStance, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(stance.as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MentionStance, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(MentionStance::from_str(&s))
    }
}

impl Default for RedditSearchParams {
    fn default() -> Self {
        Self {
            product_name: None,
            mention_stance: MentionStance::Optional,
            trigger_topics: vec![],
            query_keywords: vec![],
            seed_subreddits: vec![],
            excluded_subreddits: vec![],
            user_context: None,
        }
    }
}

/// Map [`ProjectConfig`] → [`RedditSearchParams`].
///
/// Single mapper for the config-parse step and search/enrich fallbacks.
/// `user_context` is task-scoped (not in YAML).
pub fn reddit_search_params_from_config(
    config: &ProjectConfig,
    user_context: Option<String>,
) -> RedditSearchParams {
    RedditSearchParams {
        product_name: config.product_name.clone(),
        mention_stance: config.reddit.mention_stance,
        trigger_topics: config.reddit.trigger_topics.clone(),
        query_keywords: config.reddit.query_keywords.clone(),
        seed_subreddits: config.reddit.seed_subreddits.clone(),
        excluded_subreddits: config.reddit.excluded_subreddits.clone(),
        user_context,
    }
}

/// Extract `user_context` from a reddit_opportunity_search task description.
/// The UI serializes it as `{"user_context": "..."}`; anything else yields None.
pub(crate) fn extract_user_context_from_description(task: &Task) -> Option<String> {
    let desc = task.description.as_deref()?.trim();
    let value: serde_json::Value = serde_json::from_str(desc).ok()?;
    value
        .get("user_context")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// ─── Deterministic Config Parse ───────────────────────────────────────────────

/// Deterministic step: load structured Reddit search params from `project.yaml`.
///
/// Source of truth is YAML via [`ensure_project_config`] (auto-migrates legacy
/// MD when needed). Does **not** invent keywords from brand prose or call an LLM.
///
/// `agent_provider` is unused (kept for step-registry signature compatibility).
pub fn exec_reddit_config_parse(
    task: &Task,
    project_path: &str,
    _agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    log::info!(
        "[reddit_config_parse] starting for project_path={}",
        project_path
    );

    let automation_dir = Path::new(project_path).join(".github").join("automation");
    let user_context = extract_user_context_from_description(task);

    let (config, action) = match ensure_project_config(&automation_dir) {
        Ok(pair) => pair,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!(
                "project config unavailable at {}: {e} — ensure project.yaml exists \
                 (or legacy project.md / reddit_config.md for auto-migration)",
                automation_dir.display()
            ));
        }
    };

    log::info!(
        "[reddit_config_parse] ensure action={:?} product_name={:?}",
        action,
        config.product_name
    );

    let params = reddit_search_params_from_config(&config, user_context);

    log::info!(
        "[reddit_config_parse] loaded from ProjectConfig: {} keywords, {} topics, {} subreddits",
        params.query_keywords.len(),
        params.trigger_topics.len(),
        params.seed_subreddits.len()
    );

    if params.query_keywords.is_empty() && params.trigger_topics.is_empty() {
        return crate::engine::workflows::StepResult::fail_with_output(
            "No query_keywords or trigger_topics in project.yaml reddit block — \
             operator must fill them in YAML (empty is not invented from brand prose)"
                .to_string(),
            serde_json::to_string_pretty(&params).unwrap_or_default(),
        );
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Loaded config: {} keywords, {} topics, {} subreddits",
            params.query_keywords.len(),
            params.trigger_topics.len(),
            params.seed_subreddits.len()
        ),
        output: Some(serde_json::to_string_pretty(&params).unwrap_or_default()),
        artifact_key: None,
    }
}

/// Extract post_id and reply_text from a reddit_reply task description.
/// The description format is:
/// **Subreddit:** r/...
/// **Post URL:** ...
/// **Why Relevant:** ...
/// **Draft Reply:**
/// <reply text>
/// **Post ID:** <post_id>
pub(crate) fn extract_post_details_from_task(task: &Task) -> Option<(String, String)> {
    let desc = task.description.as_ref()?;

    // Extract Post ID (last line with "Post ID:")
    let post_id = desc
        .lines()
        .find(|l| l.trim().starts_with("**Post ID:**"))
        .and_then(|l| l.split("**Post ID:**").nth(1))
        .map(|s| s.trim().to_string())?;

    // Extract Draft Reply (everything between "**Draft Reply:**" and "**Post ID:**")
    let reply_start = desc.find("**Draft Reply:**")? + "**Draft Reply:**".len();
    let reply_end = desc.find("**Post ID:**")?;
    let reply_text = desc[reply_start..reply_end].trim().to_string();

    if post_id.is_empty() || reply_text.is_empty() {
        None
    } else {
        Some((post_id, reply_text))
    }
}
