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

use rusqlite::Connection;
use std::path::Path;

use crate::models::task::Task;

// ─── Structured Config (from agentic parse step) ──────────────────────────────

/// Structured Reddit configuration parsed from reddit_config.md.
/// This is produced by the agentic `reddit_config_parse_stage` step.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RedditSearchParams {
    pub product_name: Option<String>,
    pub mention_stance: String,
    pub trigger_topics: Vec<String>,
    pub query_keywords: Vec<String>,
    pub seed_subreddits: Vec<String>,
    pub excluded_subreddits: Vec<String>,
}

impl Default for RedditSearchParams {
    fn default() -> Self {
        Self {
            product_name: None,
            mention_stance: "OPTIONAL".to_string(),
            trigger_topics: vec![],
            query_keywords: vec![],
            seed_subreddits: vec![],
            excluded_subreddits: vec![],
        }
    }
}

// ─── Config parsers ───────────────────────────────────────────────────────────

/// Extract lines from the "## Trigger Topics" section of a reddit_config.md.
/// Flexible parsing: accepts "## Trigger Topics", "## Triggers", or "## Topics"
pub(crate) fn extract_trigger_topics(config: &str, max: usize) -> Vec<String> {
    let mut in_section = false;
    let mut topics: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        // Flexible matching for trigger topics section
        let is_trigger_header = trimmed.starts_with("## Trigger Topics")
            || trimmed.starts_with("## Triggers")
            || trimmed.starts_with("## Topics");
        if is_trigger_header {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(topic) = trimmed.strip_prefix("- ") {
                let topic = topic.split('(').next().unwrap_or(topic).trim().to_string();
                if !topic.is_empty() {
                    topics.push(topic);
                    if topics.len() >= max { break; }
                }
            }
        }
    }
    topics
}

/// Extract subreddit names from the "## Seed Subreddits" or "## Target Subreddits" section.
pub(crate) fn extract_seed_subreddits(config: &str) -> Vec<String> {
    let mut in_section = false;
    let mut subs: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Seed Subreddits") || trimmed.starts_with("## Target Subreddits") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(name) = trimmed.strip_prefix("- ") {
                let name = name.trim().trim_start_matches("r/");
                let name = name.split(" — ").next().unwrap_or(name);
                let name = name.split(" - ").next().unwrap_or(name);
                let name = name.trim().to_lowercase();
                if !name.is_empty() { subs.push(name); }
            }
        }
    }
    subs
}

/// Extract compact search queries from the "## Query Keywords" section of reddit_config.md.
/// Flexible parsing: accepts "## Query Keywords", "## Keywords", or "## Queries"
pub(crate) fn extract_query_keywords(config: &str) -> Vec<String> {
    let mut in_section = false;
    let mut keywords: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        // Flexible matching for query keywords section
        let is_keywords_header = trimmed.starts_with("## Query Keywords")
            || trimmed.starts_with("## Keywords")
            || trimmed.starts_with("## Queries");
        if is_keywords_header {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(raw) = trimmed.strip_prefix("- ") {
                let raw = raw.trim();
                if raw.starts_with('"') {
                    if let Some(end) = raw[1..].find('"') {
                        let kw = raw[1..end + 1].trim().to_string();
                        if !kw.is_empty() { keywords.push(kw); }
                        continue;
                    }
                }
                let kw = raw.trim_matches('`').trim().to_string();
                if !kw.is_empty() { keywords.push(kw); }
            }
        }
    }
    keywords
}

/// Extract subreddit names from the "## Excluded Subreddits" section of reddit_config.md.
pub(crate) fn extract_excluded_subreddits(config: &str) -> std::collections::HashSet<String> {
    let mut in_section = false;
    let mut excluded: std::collections::HashSet<String> = Default::default();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Excluded Subreddits") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(name) = trimmed.strip_prefix("- ") {
                let name = name.trim().to_lowercase();
                if !name.is_empty() { excluded.insert(name); }
            }
        }
    }
    excluded
}

/// Compute engagement, accessibility, and overall scores.
///
/// - engagement  = min(10, upvotes / max(1, days_old) / 10)
/// - accessibility = <10 comments→10, 10–30→8, 30–100→6, 100+→2
/// - relevance   = 5.0 (placeholder; AI pass upgrades it via `exec_reddit_enrich`)
/// - final_score = average of three components
pub(crate) fn compute_scores(upvotes: i64, comment_count: i64, days_old: i64)
    -> (f64, f64, f64, f64, &'static str)
{
    let relevance_score: f64 = 5.0;
    let age = days_old.max(1) as f64;
    let engagement_score = (upvotes as f64 / age / 10.0).min(10.0).max(0.0);
    let accessibility_score: f64 = match comment_count {
        c if c < 10  => 10.0,
        c if c < 30  => 8.0,
        c if c < 100 => 6.0,
        _            => 2.0,
    };
    let final_score = (relevance_score + engagement_score + accessibility_score) / 3.0;
    let severity = if final_score >= 8.5 { "CRITICAL" }
        else if final_score >= 7.0 { "HIGH" }
        else if final_score >= 5.0 { "MEDIUM" }
        else { "LOW" };
    (relevance_score, engagement_score, accessibility_score, final_score, severity)
}

// ─── Agentic Config Parse ─────────────────────────────────────────────────────

/// Agentic step: Parse reddit_config.md and extract structured search parameters.
///
/// This step uses an LLM to semantically parse the markdown config file,
/// extracting trigger topics, query keywords, subreddits, product name, and stance.
/// Cannot be deterministic: understanding markdown structure and identifying
/// semantic sections requires language understanding.
pub(crate) fn exec_reddit_config_parse(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    log::info!("[reddit_config_parse] starting for project_path={}", project_path);

    let automation_dir = Path::new(project_path).join(".github").join("automation");
    
    // Read config files
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .unwrap_or_default();
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .unwrap_or_default();

    if reddit_config.is_empty() && project_summary.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "No reddit_config.md or project_summary.md found — create config files first".to_string(),
            output: None,
        };
    }

    // Build prompt for agentic parsing
    // Simplified prompt that works - see examples/test_reddit_config_parse.rs
    let prompt = format!(
        "Extract Reddit search parameters from the config files below. Return ONLY a JSON object.\n\n\
        ## reddit_config.md\n\
        ```markdown\n\
        {reddit_config}\n\
        ```\n\n\
        ## project_summary.md\n\
        ```markdown\n\
        {project_summary}\n\
        ```\n\n\
        ## brandvoice.md\n\
        ```markdown\n\
        {brandvoice}\n\
        ```\n\n\
        ## Required JSON Output\n\
        Return a JSON object with these exact keys:\n\
        - product_name: string\n\
        - mention_stance: string (REQUIRED, RECOMMENDED, OPTIONAL, or OMIT)\n\
        - trigger_topics: array of strings\n\
        - query_keywords: array of strings (use same as trigger_topics)\n\
        - seed_subreddits: array of strings (WITHOUT r/ prefix)\n\
        - excluded_subreddits: array of strings\n\n\
        ## Example\n\
        If the config has Product Name: Days to Expiry, then return:\n\
        {{\"product_name\": \"Days to Expiry\", ...}}\n\n\
        Do NOT return placeholder text like \"<actual product name>\".\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_summary = project_summary,
        brandvoice = brandvoice
    );

    // Call agent
    match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => {
            // Try to extract JSON object from the output
            let json_str = extract_json_object(&output);
            match serde_json::from_str::<RedditSearchParams>(&json_str) {
                Ok(params) => {
                    // Validate: we need at least some queries or topics
                    if params.query_keywords.is_empty() && params.trigger_topics.is_empty() {
                        crate::engine::workflows::StepResult {
                            success: false,
                            message: "No query keywords or trigger topics found in config — add them to reddit_config.md".to_string(),
                            output: Some(json_str),
                        }
                    } else {
                        crate::engine::workflows::StepResult {
                            success: true,
                            message: format!("Parsed config: {} keywords, {} topics, {} subreddits",
                                params.query_keywords.len(),
                                params.trigger_topics.len(),
                                params.seed_subreddits.len()
                            ),
                            output: Some(serde_json::to_string_pretty(&params).unwrap_or(json_str)),
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[reddit_config_parse] failed to parse agent output as JSON: {}", e);
                    crate::engine::workflows::StepResult {
                        success: false,
                        message: format!("Agent produced invalid JSON: {}", e),
                        output: Some(output),
                    }
                }
            }
        }
        Err(err) => {
            log::warn!("[reddit_config_parse] agent failed: {}", err);
            crate::engine::workflows::StepResult {
                success: false,
                message: format!("Config parsing agent failed: {}", err),
                output: None,
            }
        }
    }
}

/// Load structured search params from the reddit_config_parse_stage artifact.
/// Returns None if no artifact found or parsing fails.
fn load_search_params_from_artifact(task: &Task, _project_path: &str) -> Option<RedditSearchParams> {
    // Look for artifact from reddit_config_parse_stage
    let artifact = task.artifacts.iter().find(|a| a.key == "reddit_config_parse_stage")?;
    let content = artifact.content.as_ref()?;
    
    log::info!("[reddit_search] found structured params artifact ({} chars)", content.len());
    
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
            log::warn!("[reddit_search] failed to parse artifact as RedditSearchParams: {}", e);
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
    }
}

// ─── Search ───────────────────────────────────────────────────────────────────

/// Deterministic Reddit search step.
///
/// Reads queries/subreddits from the structured search params artifact (produced by
/// reddit_config_parse_stage), calls the Reddit API, applies the 14-day filter and
/// MEDIUM+ score filter, deduplicates, and returns the top 10 posts by score.
pub(crate) async fn exec_reddit_search(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    const MAX_AGE_DAYS: i64 = 14;
    const MAX_SEARCH_PAIRS: usize = 50;
    const MAX_RESULTS: usize = 10;

    log::info!("[reddit_search] starting for project={} path={}", task.project_id, project_path);

    // Try to load structured search params from artifact (produced by reddit_config_parse_stage)
    let params = load_search_params_from_artifact(task, project_path);
    
    // Fallback: parse config directly if no artifact (backward compatibility)
    let params = match params {
        Some(p) => p,
        None => {
            log::info!("[reddit_search] no structured params artifact found, falling back to direct config parse");
            let config_path = format!("{}/.github/automation/reddit_config.md", project_path);
            match std::fs::read_to_string(&config_path) {
                Ok(config) => parse_config_fallback(&config),
                Err(e) => return crate::engine::workflows::StepResult {
                    success: false,
                    message: format!("reddit_config.md not found at {} — create it first: {}", config_path, e),
                    output: None,
                },
            }
        }
    };

    // Build queries list from keywords or topics
    let queries: Vec<String> = if !params.query_keywords.is_empty() {
        params.query_keywords.clone()
    } else {
        params.trigger_topics.clone()
    };

    let seed_subs = params.seed_subreddits;
    let excluded_subs = params.excluded_subreddits;
    let mention_stance = params.mention_stance;

    log::info!(
        "[reddit_search] queries ({}) {:?}  seed_subreddits ({}) {:?}",
        queries.len(), &queries[..queries.len().min(5)],
        seed_subs.len(), &seed_subs[..seed_subs.len().min(5)]
    );

    if queries.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "No search queries found. The reddit_config_parse_stage should have extracted query_keywords or trigger_topics from reddit_config.md.".to_string(),
            output: None,
        };
    }

    let search_pairs: Vec<(String, String)> = if seed_subs.is_empty() {
        log::warn!("[reddit_search] no seed subreddits — falling back to global search");
        queries.iter().take(MAX_SEARCH_PAIRS).map(|q| (String::new(), q.clone())).collect()
    } else {
        seed_subs.iter()
            .flat_map(|sub| queries.iter().map(move |q| (sub.clone(), q.clone())))
            .take(MAX_SEARCH_PAIRS)
            .collect()
    };
    log::info!("[reddit_search] {} search pairs", search_pairs.len());

    let mut all_posts: Vec<serde_json::Value> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = Default::default();
    let mut too_old = 0usize;
    let mut excluded_sub_count = 0usize;
    let mut below_threshold = 0usize;
    let mut history_filtered = 0usize;

    let history_manager = crate::reddit::history::RedditHistoryManager::new(
        std::path::Path::new(project_path)
    );
    let handled_ids = history_manager.get_all_handled_ids();

    for (subreddit, query) in &search_pairs {
        let posts = match crate::reddit::search::search_submissions(query, subreddit, 10, "relevance", "week").await {
            Ok(p) => p,
            Err(e) => {
                log::warn!("[reddit_search] search failed sub={:?} q={:?}: {}", subreddit, query, e);
                continue;
            }
        };

        let before = all_posts.len();
        for post in posts {
            let post_id = post.post_id.clone();

            if let Some(ref sub) = post.subreddit {
                if excluded_subs.contains(&sub.to_lowercase()) {
                    excluded_sub_count += 1;
                    continue;
                }
            }

            let days_old = post.days_old.unwrap_or(0);
            if days_old > MAX_AGE_DAYS {
                too_old += 1;
                continue;
            }

            if !seen_ids.insert(post_id.clone()) { continue; }

            if handled_ids.contains(&post_id) {
                history_filtered += 1;
                continue;
            }

            let upvotes = post.upvotes.unwrap_or(0);
            let comments = post.comment_count.unwrap_or(0);
            let (relevance, engagement, accessibility, final_score, severity) =
                compute_scores(upvotes, comments, days_old);

            if final_score < 5.0 {
                below_threshold += 1;
                continue;
            }

            all_posts.push(serde_json::json!({
                "post_id": post_id,
                "title": post.title,
                "url": post.url,
                "subreddit": post.subreddit,
                "author": post.author,
                "upvotes": upvotes,
                "comment_count": comments,
                "days_old": days_old,
                "created_at": post.created_at,
                "posted_date": post.created_at,
                "selftext": post.selftext,
                "relevance_score": relevance,
                "engagement_score": engagement,
                "accessibility_score": accessibility,
                "final_score": final_score,
                "severity": severity,
                "mention_stance": mention_stance,
            }));
        }
        log::info!("[reddit_search] +{} accepted (total {})", all_posts.len() - before, all_posts.len());
    }

    all_posts.sort_by(|a, b| {
        let fa = a["final_score"].as_f64().unwrap_or(0.0);
        let fb = b["final_score"].as_f64().unwrap_or(0.0);
        fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal)
    });
    all_posts.truncate(MAX_RESULTS);

    log::info!(
        "[reddit_search] done — kept={} too_old={} excluded_sub={} below_threshold={} history_filtered={}",
        all_posts.len(), too_old, excluded_sub_count, below_threshold, history_filtered
    );

    if all_posts.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No Reddit posts found across {} search pairs ({} too old, {} excluded, {} below threshold)",
                search_pairs.len(), too_old, excluded_sub_count, below_threshold
            ),
            output: None,
        };
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Found {} Reddit posts ({} too old, {} excluded, {} below threshold, {} already handled)",
            all_posts.len(), too_old, excluded_sub_count, below_threshold, history_filtered
        ),
        output: Some(serde_json::to_string(&serde_json::json!({"posts": all_posts})).unwrap_or_default()),
    }
}

// ─── Persist ─────────────────────────────────────────────────────────────────

/// Parse a JSON array of Reddit opportunity objects and upsert each into SQLite.
///
/// Tolerates partial fields — only `post_id` is required.
/// Clears pending rows from previous runs before inserting fresh results.
/// Rows with reply_status='posted' or 'skipped' are preserved.
pub(crate) fn persist_reddit_opportunities(conn: &Connection, project_id: &str, json_str: &str) {
    log::info!("[reddit] persist_reddit_opportunities project={} json_len={}", project_id, json_str.len());

    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            let preview = crate::engine::text::char_prefix(json_str, 200);
            log::warn!("[reddit] failed to parse JSON: {} — first 200 chars: {:?}", e, preview);
            return;
        }
    };

    let array = if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else if let Some(arr) = ["opportunities", "results", "posts", "items"]
        .iter()
        .find_map(|key| value.get(key).and_then(|v| v.as_array()).cloned())
    {
        arr
    } else {
        log::warn!("[reddit] unrecognised JSON structure — keys: {:?}",
            value.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()));
        return;
    };

    // Clear pending rows from previous runs; preserve posted/skipped for history dedup.
    let deleted = conn.execute(
        "DELETE FROM reddit_opportunities WHERE project_id=?1 AND reply_status='pending'",
        rusqlite::params![project_id],
    ).unwrap_or(0);
    if deleted > 0 {
        log::info!("[reddit] cleared {} stale pending rows for project={}", deleted, project_id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut upserted = 0usize;
    let mut skipped = 0usize;

    for item in &array {
        let post_id = match item.get("post_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => { skipped += 1; continue; }
        };

        let already_handled: bool = conn.query_row(
            "SELECT COUNT(*) FROM reddit_opportunities WHERE post_id=?1 AND reply_status IN ('posted','skipped')",
            rusqlite::params![post_id],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0;
        if already_handled {
            skipped += 1;
            continue;
        }

        let opp = crate::models::reddit::RedditOpportunity {
            post_id,
            title: item.get("title").and_then(|v| v.as_str()).map(str::to_string),
            url: item.get("url").and_then(|v| v.as_str()).map(str::to_string),
            subreddit: item.get("subreddit").and_then(|v| v.as_str()).map(str::to_string),
            author: item.get("author").and_then(|v| v.as_str()).map(str::to_string),
            posted_date: item.get("posted_date").and_then(|v| v.as_str()).map(str::to_string),
            upvotes: item.get("upvotes").and_then(|v| v.as_i64()),
            comment_count: item.get("comment_count").and_then(|v| v.as_i64()),
            relevance_score: item.get("relevance_score").and_then(|v| v.as_f64()),
            engagement_score: item.get("engagement_score").and_then(|v| v.as_f64()),
            accessibility_score: item.get("accessibility_score").and_then(|v| v.as_f64()),
            final_score: item.get("final_score").and_then(|v| v.as_f64()),
            severity: item.get("severity").and_then(|v| v.as_str()).map(str::to_string),
            why_relevant: item.get("why_relevant").and_then(|v| v.as_str()).map(str::to_string),
            key_pain_points: item.get("key_pain_points")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            website_fit: item.get("website_fit").and_then(|v| v.as_str()).map(str::to_string),
            mention_stance: item.get("mention_stance").and_then(|v| v.as_str()).map(str::to_string),
            reply_status: item.get("reply_status").and_then(|v| v.as_str()).unwrap_or("pending").to_string(),
            reply_text: item.get("reply_text").and_then(|v| v.as_str()).map(str::to_string),
            reply_url: item.get("reply_url").and_then(|v| v.as_str()).map(str::to_string),
            reply_upvotes: item.get("reply_upvotes").and_then(|v| v.as_i64()),
            reply_replies: item.get("reply_replies").and_then(|v| v.as_i64()),
            posted_at: item.get("posted_at").and_then(|v| v.as_str()).map(str::to_string),
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match crate::reddit::db::upsert_opportunity(conn, &opp) {
            Ok(_) => { upserted += 1; }
            Err(e) => {
                log::warn!("[reddit] upsert failed post_id={}: {}", opp.post_id, e);
                skipped += 1;
            }
        }
    }

    log::info!("[reddit] done — upserted={} skipped={} project={}", upserted, skipped, project_id);
}

// ─── Enrichment ───────────────────────────────────────────────────────────────

/// AI enrichment pass: fills in `why_relevant`, `key_pain_points`, `website_fit`,
/// and draft `reply_text`, and recalculates `relevance_score` / `final_score`.
///
/// Fetches up to 5 un-enriched posts per call; silently returns if none pending.
pub fn exec_reddit_enrich(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    agent_provider: &str,
) {
    use crate::engine::agent;
    use std::path::Path;

    log::info!("[reddit_enrich] starting for project={}", project_id);

    let rows: Vec<(String, Option<String>, Option<String>, Option<f64>, Option<f64>)> = {
        let mut result = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT post_id, title, subreddit, engagement_score, accessibility_score \
             FROM reddit_opportunities \
             WHERE project_id=?1 \
               AND (why_relevant IS NULL OR reply_text IS NULL) \
               AND reply_status != 'skipped' \
             LIMIT 5",
        ) {
            if let Ok(mapped) = stmt.query_map(rusqlite::params![project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<f64>>(4)?,
                ))
            }) {
                for item in mapped.flatten() { result.push(item); }
            } else {
                log::warn!("[reddit_enrich] query failed");
                return;
            }
        } else {
            log::warn!("[reddit_enrich] prepare failed");
            return;
        }
        result
    };

    if rows.is_empty() {
        log::info!("[reddit_enrich] no unenriched posts — skipping");
        return;
    }
    log::info!("[reddit_enrich] {} posts to enrich", rows.len());

    let automation_dir = Path::new(project_path).join(".github").join("automation");
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md")).unwrap_or_default();
    let reddit_config_raw = std::fs::read_to_string(automation_dir.join("reddit_config.md")).unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();
    let guardrails = std::fs::read_to_string(
        automation_dir.join("reddit").join("_reply_guardrails.md")
    ).unwrap_or_default();

    if project_summary.is_empty() && reddit_config_raw.is_empty() {
        log::warn!("[reddit_enrich] no project context — skipping");
        return;
    }

    let cfg = crate::reddit::config::parse_reddit_config(&reddit_config_raw);
    let product_name = cfg.product_name.as_deref().unwrap_or("the product").to_string();
    let mention_stance_str = cfg.mention_stance.as_str().to_string();
    let stance_instruction = match cfg.mention_stance {
        crate::reddit::config::MentionStance::Required => format!(
            "REQUIRED: The reply MUST contain the exact product name \"{}\" — no vague substitutes.",
            product_name
        ),
        crate::reddit::config::MentionStance::Recommended => format!(
            "RECOMMENDED: Mention \"{}\" by name if the topic is a natural fit.",
            product_name
        ),
        crate::reddit::config::MentionStance::Optional => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally.",
            product_name
        ),
        crate::reddit::config::MentionStance::Omit =>
            "OMIT: Do NOT mention any product name in this reply.".to_string(),
    };

    let posts_block: String = rows.iter().enumerate().map(|(i, (pid, title, sub, _, _))| {
        format!(
            "{}. post_id=\"{}\"  subreddit=\"{}\"  title=\"{}\"",
            i + 1,
            pid,
            sub.as_deref().unwrap_or("unknown"),
            title.as_deref().unwrap_or("(no title)").replace('"', "'").chars().take(200).collect::<String>()
        )
    }).collect::<Vec<_>>().join("\n");

    let prompt = format!(
        r#"You are a copywriter. Your only job is to read the post titles below and produce a JSON array.

DO NOT run any shell commands. DO NOT fetch any URLs. Work ONLY from the post titles and subreddits provided.

## PRODUCT CONTEXT
{project_summary}

## REDDIT CONFIG
{reddit_config_raw}

## BRAND VOICE
{brandvoice}

## REPLY GUARDRAILS
{guardrails}

## PRODUCT MENTION RULES
Product name: {product_name}
Mention stance: {mention_stance_str}
{stance_instruction}

## POST TITLES
{posts_block}

## OUTPUT FORMAT
Return a JSON array with exactly {count} objects:
[
  {{
    "post_id": "<exact post_id>",
    "relevance_score": <integer 0-10>,
    "why_relevant": "<one sentence>",
    "key_pain_points": ["<pain 1>", "<pain 2>"],
    "website_fit": "<one sentence>",
    "reply_text": "<3-5 sentence plain-text reply>"
  }}
]

reply_text: plain text only, no markdown, no bullets, no URLs.
Return ONLY the raw JSON array."#,
        project_summary = project_summary,
        reddit_config_raw = reddit_config_raw,
        brandvoice = brandvoice,
        guardrails = guardrails,
        product_name = product_name,
        mention_stance_str = mention_stance_str,
        stance_instruction = stance_instruction,
        posts_block = posts_block,
        count = rows.len(),
    );

    let output = match agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[reddit_enrich] agent failed: {}", e);
            return;
        }
    };

    let json_str = extract_json_array(&output);
    let enrichments: Vec<serde_json::Value> = match serde_json::from_str(&json_str) {
        Ok(serde_json::Value::Array(arr)) => arr,
        _ => {
            let preview = crate::engine::text::char_prefix(&output, 300);
            log::warn!("[reddit_enrich] could not parse agent output as JSON array — first 300 chars: {:?}",
                preview);
            return;
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut updated = 0usize;

    for item in &enrichments {
        let post_id = match item.get("post_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        let relevance_score = item.get("relevance_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(5.0)
            .max(0.0).min(10.0);

        let why_relevant = item.get("why_relevant").and_then(|v| v.as_str()).unwrap_or("");
        let website_fit = item.get("website_fit").and_then(|v| v.as_str()).unwrap_or("");
        let reply_text = item.get("reply_text").and_then(|v| v.as_str()).unwrap_or("");
        let pain_points_json = item.get("key_pain_points")
            .and_then(|v| v.as_array())
            .map(|arr| serde_json::to_string(arr).unwrap_or_else(|_| "[]".to_string()))
            .unwrap_or_else(|| "[]".to_string());

        let (engagement_score, accessibility_score): (f64, f64) = rows.iter()
            .find(|(pid, _, _, _, _)| pid == post_id)
            .map(|(_, _, _, eng, acc)| (eng.unwrap_or(5.0), acc.unwrap_or(5.0)))
            .unwrap_or((5.0, 5.0));

        let final_score = (relevance_score + engagement_score + accessibility_score) / 3.0;
        let severity = if final_score >= 8.5 { "CRITICAL" }
            else if final_score >= 7.0 { "HIGH" }
            else if final_score >= 5.0 { "MEDIUM" }
            else { "LOW" };

        match conn.execute(
            "UPDATE reddit_opportunities \
             SET relevance_score=?1, why_relevant=?2, key_pain_points=?3, website_fit=?4, \
                 final_score=?5, severity=?6, reply_text=?7, mention_stance=?8, updated_at=?9 \
             WHERE post_id=?10 AND project_id=?11",
            rusqlite::params![
                relevance_score, why_relevant, pain_points_json, website_fit,
                final_score, severity,
                if reply_text.is_empty() { None } else { Some(reply_text) },
                &mention_stance_str,
                now, post_id, project_id
            ],
        ) {
            Ok(n) if n > 0 => { updated += 1; }
            Ok(_) => log::warn!("[reddit_enrich] post_id={} not found in DB", post_id),
            Err(e) => log::warn!("[reddit_enrich] update failed for {}: {}", post_id, e),
        }
    }

    log::info!("[reddit_enrich] enriched+drafted {}/{} posts project={}", updated, rows.len(), project_id);
}

/// Fetch enriched Reddit opportunities from the database and return them as JSON.
/// This is called as the final step of the reddit workflow to return concrete
/// posting suggestions with drafted replies to the user.
pub fn exec_reddit_fetch_results(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> crate::engine::workflows::StepResult {
    use crate::models::reddit::RedditOpportunity;
    
    log::info!("[reddit_fetch_results] fetching enriched opportunities for project={}", project_id);
    
    let mut opportunities: Vec<RedditOpportunity> = Vec::new();
    
    match conn.prepare(
        "SELECT post_id, title, url, subreddit, author, posted_date, upvotes, comment_count,
                relevance_score, engagement_score, accessibility_score, final_score, severity,
                why_relevant, key_pain_points, website_fit, mention_stance, reply_status,
                reply_text, reply_url, reply_upvotes, reply_replies, posted_at,
                project_id, created_at, updated_at
         FROM reddit_opportunities
         WHERE project_id=?1 AND reply_status='pending'
         ORDER BY final_score DESC NULLS LAST, relevance_score DESC NULLS LAST
         LIMIT 20"
    ) {
        Ok(mut stmt) => {
            match stmt.query_map(rusqlite::params![project_id], |row| {
                let pain_points_json: String = row.get::<_, String>(14).unwrap_or_else(|_| "[]".to_string());
                let pain_points: Vec<String> = serde_json::from_str(&pain_points_json).unwrap_or_default();
                
                Ok(RedditOpportunity {
                    post_id: row.get(0)?,
                    title: row.get(1).ok(),
                    url: row.get(2).ok(),
                    subreddit: row.get(3).ok(),
                    author: row.get(4).ok(),
                    posted_date: row.get(5).ok(),
                    upvotes: row.get(6).ok(),
                    comment_count: row.get(7).ok(),
                    relevance_score: row.get(8).ok(),
                    engagement_score: row.get(9).ok(),
                    accessibility_score: row.get(10).ok(),
                    final_score: row.get(11).ok(),
                    severity: row.get(12).ok(),
                    why_relevant: row.get(13).ok(),
                    key_pain_points: pain_points,
                    website_fit: row.get(15).ok(),
                    mention_stance: row.get(16).ok(),
                    reply_status: row.get(17).unwrap_or_else(|_| "pending".to_string()),
                    reply_text: row.get(18).ok(),
                    reply_url: row.get(19).ok(),
                    reply_upvotes: row.get(20).ok(),
                    reply_replies: row.get(21).ok(),
                    posted_at: row.get(22).ok(),
                    project_id: row.get(23)?,
                    created_at: row.get(24)?,
                    updated_at: row.get(25)?,
                })
            }) {
                Ok(rows) => {
                    for opp in rows.flatten() {
                        opportunities.push(opp);
                    }
                }
                Err(e) => {
                    log::warn!("[reddit_fetch_results] query failed: {}", e);
                }
            }
        }
        Err(e) => {
            log::warn!("[reddit_fetch_results] prepare failed: {}", e);
        }
    }
    
    // Count opportunities with drafted replies
    let with_replies = opportunities.iter().filter(|o| o.reply_text.is_some()).count();
    
    log::info!("[reddit_fetch_results] found {} opportunities ({} with drafted replies)", 
        opportunities.len(), with_replies);
    
    if opportunities.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No pending Reddit opportunities found. Run the search to find new posts.".to_string(),
            output: Some("[]".to_string()),
        };
    }
    
    match serde_json::to_string_pretty(&opportunities) {
        Ok(json) => crate::engine::workflows::StepResult {
            success: true,
            message: format!(
                "Found {} Reddit opportunities with {} drafted replies. Review them below:",
                opportunities.len(),
                with_replies
            ),
            output: Some(json),
        },
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to serialize opportunities: {}", e),
            output: None,
        },
    }
}

// ─── Follow-up Task Creation ─────────────────────────────────────────────────

/// Create reddit_reply tasks from opportunities found during search.
/// Returns the IDs of created tasks.
pub fn create_reddit_reply_tasks_from_opportunities(
    conn: &rusqlite::Connection,
    parent_task: &crate::models::task::Task,
    _project_path: &str,
) -> Vec<String> {
    use crate::models::task::{Task, TaskStatus, Priority, ExecutionMode, AgentPolicy, TaskRun};
    use chrono::Utc;
    
    let mut created_ids = Vec::new();
    
    // Fetch pending opportunities for this project that have drafted replies
    let opportunities: Vec<crate::models::reddit::RedditOpportunity> = 
        match crate::reddit::db::list_opportunities(conn, &parent_task.project_id, Some("pending")) {
            Ok(ops) => ops.into_iter()
                .filter(|o| o.reply_text.is_some())
                .collect(),
            Err(e) => {
                log::warn!("[create_reddit_reply_tasks] failed to fetch opportunities: {}", e);
                return created_ids;
            }
        };
    
    log::info!("[create_reddit_reply_tasks] creating tasks for {} opportunities", opportunities.len());
    
    for opp in opportunities {
        let task_id = format!("task-{}", Utc::now().timestamp_millis() + created_ids.len() as i64);
        let severity_priority = match opp.severity.as_deref() {
            Some("CRITICAL") | Some("HIGH") => Priority::High,
            _ => Priority::Medium,
        };
        
        let title = format!("Reply to: {}", opp.title.as_deref().unwrap_or("Reddit post"));
        let description = format!(
            "Subreddit: r/{}\nPost URL: {}\n\nWhy relevant: {}\n\nDraft reply:\n{}\n\nPost ID: {}",
            opp.subreddit.as_deref().unwrap_or("unknown"),
            opp.url.as_deref().unwrap_or(""),
            opp.why_relevant.as_deref().unwrap_or(""),
            opp.reply_text.as_deref().unwrap_or(""),
            opp.post_id
        );
        
        let reply_task = Task {
            id: task_id.clone(),
            project_id: parent_task.project_id.clone(),
            task_type: "reddit_reply".to_string(),
            phase: "engagement".to_string(),
            status: TaskStatus::Todo,
            priority: severity_priority,
            execution_mode: ExecutionMode::Manual, // User needs to manually review and post
            agent_policy: AgentPolicy::Optional,
            title: Some(title),
            description: Some(description),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None },
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        
        match crate::engine::task_store::create_task(conn, &reply_task) {
            Ok(_) => {
                log::info!("[create_reddit_reply_tasks] created task {} for post {}", task_id, opp.post_id);
                created_ids.push(task_id);
            }
            Err(e) => {
                log::warn!("[create_reddit_reply_tasks] failed to create task for {}: {}", opp.post_id, e);
            }
        }
    }
    
    log::info!("[create_reddit_reply_tasks] created {} reply tasks", created_ids.len());
    created_ids
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Strip markdown code fences and extract the first JSON array from agent output.
pub(crate) fn extract_json_array(output: &str) -> String {
    let trimmed = output.trim();
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            return trimmed[start..=end].to_string();
        }
    }
    trimmed.to_string()
}

/// Extract a JSON object from text (looks for {...})
pub fn extract_json_object(output: &str) -> String {
    let trimmed = output.trim();
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return trimmed[start..=end].to_string();
        }
    }
    trimmed.to_string()
}
