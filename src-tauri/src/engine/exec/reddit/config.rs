use crate::models::task::Task;
use std::path::Path;

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
            if trimmed.starts_with("##") {
                break;
            }
            if let Some(topic) = trimmed.strip_prefix("- ") {
                let topic = topic.split('(').next().unwrap_or(topic).trim().to_string();
                if !topic.is_empty() {
                    topics.push(topic);
                    if topics.len() >= max {
                        break;
                    }
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
        if trimmed.starts_with("## Seed Subreddits") || trimmed.starts_with("## Target Subreddits")
        {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") {
                break;
            }
            if let Some(name) = trimmed.strip_prefix("- ") {
                let name = name.trim().trim_start_matches("r/");
                let name = name.split(" — ").next().unwrap_or(name);
                let name = name.split(" - ").next().unwrap_or(name);
                let name = name.trim().to_lowercase();
                if !name.is_empty() {
                    subs.push(name);
                }
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
            if trimmed.starts_with("##") {
                break;
            }
            if let Some(raw) = trimmed.strip_prefix("- ") {
                let raw = raw.trim();
                if raw.starts_with('"') {
                    if let Some(end) = raw[1..].find('"') {
                        let kw = raw[1..end + 1].trim().to_string();
                        if !kw.is_empty() {
                            keywords.push(kw);
                        }
                        continue;
                    }
                }
                let kw = raw.trim_matches('`').trim().to_string();
                if !kw.is_empty() {
                    keywords.push(kw);
                }
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
            if trimmed.starts_with("##") {
                break;
            }
            if let Some(name) = trimmed.strip_prefix("- ") {
                let name = name.trim().to_lowercase();
                if !name.is_empty() {
                    excluded.insert(name);
                }
            }
        }
    }
    excluded
}

// ─── Agentic Config Parse ─────────────────────────────────────────────────────

/// Agentic step: Parse reddit_config.md and extract structured search parameters.
///
/// This step uses an LLM to semantically parse the markdown config file,
/// extracting trigger topics, query keywords, subreddits, product name, and stance.
/// Cannot be deterministic: understanding markdown structure and identifying
/// semantic sections requires language understanding.
pub fn exec_reddit_config_parse(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    log::info!(
        "[reddit_config_parse] starting for project_path={}",
        project_path
    );

    let automation_dir = Path::new(project_path).join(".github").join("automation");

    // Primary: project.md (consolidated). Fallback: legacy files.
    let project_context = std::fs::read_to_string(automation_dir.join("project.md"))
        .or_else(|_| {
            // Legacy fallback: stitch old files together
            let summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
                .unwrap_or_default();
            let brand =
                std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();
            let brief = std::fs::read_to_string(automation_dir.join("seo_content_brief.md"))
                .unwrap_or_default();
            Ok::<String, std::io::Error>(format!("{}\n\n{}\n\n{}", summary, brand, brief))
        })
        .unwrap_or_default();
    let reddit_config =
        std::fs::read_to_string(automation_dir.join("reddit_config.md")).unwrap_or_default();

    if reddit_config.is_empty() && project_context.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "No reddit_config.md or project.md found — create config files first"
                .to_string(),
            output: None,
        };
    }

    // Build prompt for agentic parsing.
    // reddit_config.md remains the primary source for Reddit-specific constraints,
    // while project context expands topic coverage and helps infer missing or weak search inputs.
    let prompt = format!(
        "Extract and improve Reddit search parameters from the config files below. Return ONLY a JSON object.\n\n\
        ## reddit_config.md\n\
        ```markdown\n\
        {reddit_config}\n\
        ```\n\n\
        ## Project Context\n\
        ```markdown\n\
        {project_context}\n\
        ```\n\n\
        ## How To Use The Inputs\n\
        - Treat reddit_config.md as the PRIMARY source for Reddit-specific guidance, constraints, exclusions, and any explicitly provided search themes or subreddit targets.\n\
        - Use Project Context to EXPAND and REFINE the search plan so it better matches the product, audience, pain points, terminology, and adjacent use cases.\n\
        - If reddit_config.md is sparse, generic, or missing detail, infer stronger trigger_topics, query_keywords, and seed_subreddits from Project Context.\n\
        - Prefer concrete search phrases real Reddit users would post about, not abstract category labels.\n\
        - Prefer subreddits where the target audience actually discusses the underlying problem, not just the product category in the abstract.\n\
        - Avoid generic, low-signal, or off-topic queries and avoid subreddits that are too broad unless they are clearly relevant.\n\n\
        ## Required JSON Output\n\
        Return a JSON object with these exact keys:\n\
        - product_name: string\n\
        - mention_stance: string (REQUIRED, RECOMMENDED, OPTIONAL, or OMIT)\n\
        - trigger_topics: array of strings (high-level Reddit problem themes)\n\
        - query_keywords: array of strings (specific Reddit search phrases; do NOT just copy trigger_topics unless that is genuinely best)\n\
        - seed_subreddits: array of strings (WITHOUT r/ prefix)\n\
        - excluded_subreddits: array of strings\n\n\
        ## Output Quality Rules\n\
        - trigger_topics should represent the core problems, moments, or intents that would lead someone to post on Reddit.\n\
        - query_keywords should be more search-oriented than trigger_topics and can include pain-point phrasing, question phrasing, and outcome phrasing.\n\
        - seed_subreddits should include communities where those problems are discussed, even if the subreddit is adjacent rather than an exact category match.\n\
        - Keep excluded_subreddits if they are explicitly specified; otherwise return an empty array when none are clear.\n\
        ## Example\n\
        If the config has Product Name: Days to Expiry, then return:\n\
        {{\"product_name\": \"Days to Expiry\", ...}}\n\n\
        Do NOT return placeholder text like \"<actual product name>\".\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_context = project_context
    );

    // Call agent
    match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => {
            log::info!(
                "[reddit_config_parse] agent output ({} chars): {:?}",
                output.len(),
                &output[..output.len().min(2000)]
            );

            // Try to extract JSON object from the output
            let json_str = match extract_json_object(&output) {
                Ok(json) => {
                    log::info!(
                        "[reddit_config_parse] extracted JSON ({} chars)",
                        json.len()
                    );
                    json
                }
                Err(e) => {
                    log::warn!("[reddit_config_parse] JSON extraction failed: {}", e);

                    // Save full output for debugging
                    let debug_path = std::env::temp_dir().join(format!(
                        "kimi_error_{}.txt",
                        chrono::Utc::now().timestamp_millis()
                    ));
                    let _ = std::fs::write(&debug_path, &output);
                    log::warn!(
                        "[reddit_config_parse] full output saved to: {:?}",
                        debug_path
                    );

                    return crate::engine::workflows::StepResult {
                        success: false,
                        message: format!("Failed to extract JSON from agent output: {}", e),
                        output: Some(output),
                    };
                }
            };

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
                            message: format!(
                                "Parsed config: {} keywords, {} topics, {} subreddits",
                                params.query_keywords.len(),
                                params.trigger_topics.len(),
                                params.seed_subreddits.len()
                            ),
                            output: Some(serde_json::to_string_pretty(&params).unwrap_or(json_str)),
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[reddit_config_parse] JSON parse error: {}", e);
                    log::warn!(
                        "[reddit_config_parse] extracted content that failed to parse: {}",
                        &json_str[..json_str.len().min(1000)]
                    );

                    crate::engine::workflows::StepResult {
                        success: false,
                        message: format!("Agent returned invalid JSON structure: {}", e),
                        output: Some(json_str),
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
/// Extract and validate JSON from agent output.
///
/// Tries multiple strategies in order:
/// 1. Markdown code block (```json ... ```)
/// 2. Plain code block (``` ... ```)
/// 3. Raw JSON object ({...})
///
/// Returns Err if no valid JSON found or if extracted content isn't valid JSON.
pub fn extract_json_object(output: &str) -> Result<String, String> {
    let trimmed = output.trim();

    if trimmed.is_empty() {
        return Err("Agent output is empty".to_string());
    }

    // Strategy 1: Look for ```json ... ``` code block
    for opener in ["```json\n", "```json\r\n", "```JSON\n", "```Json\n"] {
        if let Some(start) = trimmed.find(opener) {
            let after_open = start + opener.len();
            let rest = &trimmed[after_open..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                if is_valid_json(candidate) {
                    return Ok(candidate.to_string());
                }
            }
        }
    }

    // Strategy 2: Look for plain ``` ... ``` code block
    if let Some(start) = trimmed.find("```\n") {
        let after_open = start + 4;
        let rest = &trimmed[after_open..];
        if let Some(end) = rest.find("```") {
            let candidate = rest[..end].trim();
            if is_valid_json(candidate) {
                return Ok(candidate.to_string());
            }
        }
    }

    // Strategy 3: Look for raw JSON object (outermost braces)
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                let candidate = &trimmed[start..=end];
                if is_valid_json(candidate) {
                    return Ok(candidate.to_string());
                }
            }
        }
    }

    // Strategy 4: Look for raw JSON array
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end > start {
                let candidate = &trimmed[start..=end];
                if is_valid_json(candidate) {
                    return Ok(candidate.to_string());
                }
            }
        }
    }

    // Nothing worked - provide helpful error
    let preview = if trimmed.len() > 500 {
        format!("{}... ({} total chars)", &trimmed[..500], trimmed.len())
    } else {
        trimmed.to_string()
    };
    Err(format!(
        "No valid JSON found in agent output. Preview: {}",
        preview
    ))
}

/// Quick validation that a string is valid JSON
fn is_valid_json(s: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(s).is_ok()
}
