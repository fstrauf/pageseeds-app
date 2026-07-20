use crate::models::article::Article;
use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskRun, TaskStatus};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Build content tasks from selected keywords and mark the research task as done.
/// Validates inputs, deduplicates, and constructs task specs.
pub fn build_content_tasks_from_keywords(
    requested_keywords: Vec<String>,
    research_task: &Task,
    research_task_id: &str,
    project_id: &str,
    brief_ctx: &ContentBriefContext,
) -> Result<Vec<Task>, String> {
    use crate::config::{
        default_follow_up_policy, default_phase, default_review_surface, default_run_policy,
    };

    // Determine content task type based on research task type
    let content_task_type = if research_task.task_type == "research_landing_pages" {
        "create_landing_page"
    } else {
        "write_article"
    };
    if research_task.project_id != project_id {
        return Err("Research task does not belong to this project".to_string());
    }

    let allowed_keywords = extract_selectable_keywords(research_task);
    if allowed_keywords.is_empty() {
        return Err(
            "No selectable keywords found on the research task. Re-run keyword research first."
                .to_string(),
        );
    }

    let mut seen_requested = HashSet::new();
    let requested_keywords: Vec<String> = requested_keywords
        .into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .filter(|k| seen_requested.insert(normalize_keyword(k)))
        .collect();

    if requested_keywords.is_empty() {
        return Err("Select at least one keyword".to_string());
    }

    let allowed_set: HashSet<String> = allowed_keywords
        .iter()
        .map(|k| normalize_keyword(k))
        .collect();
    let invalid: Vec<String> = requested_keywords
        .iter()
        .filter(|k| !allowed_set.contains(&normalize_keyword(k)))
        .cloned()
        .collect();

    if !invalid.is_empty() {
        return Err(format!(
            "Some selected keywords are outside the workflow selection list: {}",
            invalid.join(", ")
        ));
    }

    let metrics = extract_keyword_metrics(research_task);
    let lp_meta = if content_task_type == "create_landing_page" {
        extract_landing_page_meta(research_task)
    } else {
        HashMap::new()
    };
    let article_meta = if content_task_type == "write_article" {
        extract_article_keyword_meta(research_task)
    } else {
        HashMap::new()
    };

    let mut created = Vec::new();
    for (idx, keyword) in requested_keywords.iter().enumerate() {
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!("task-{}-{}", chrono::Utc::now().timestamp_millis(), idx);
        let title = to_title_case(keyword);
        let metric = metrics.get(&normalize_keyword(keyword));
        let priority_enum = compute_task_priority(metric);
        let brief = build_content_brief(
            keyword,
            metric,
            lp_meta.get(&normalize_keyword(keyword)),
            article_meta.get(&normalize_keyword(keyword)),
            brief_ctx,
        );
        let description = build_content_task_description(&brief);
        let provenance = build_keyword_provenance_artifact(keyword, research_task_id);
        let brief_artifact = build_content_brief_artifact(&brief, research_task_id);

        let task = Task {
            id,
            phase: default_phase(content_task_type).to_string(),
            run_policy: default_run_policy(content_task_type),
            review_surface: default_review_surface(content_task_type),
            follow_up_policy: default_follow_up_policy(content_task_type),
            task_type: content_task_type.to_string(),
            status: TaskStatus::Todo,
            priority: priority_enum,
            agent_policy: AgentPolicy::None,
            title: Some(title),
            description: Some(description),
            project_id: project_id.to_string(),
            depends_on: vec![research_task_id.to_string()],
            artifacts: vec![provenance, brief_artifact],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
            not_before: None,
        };
        created.push(task);
    }

    Ok(created)
}

/// Create content tasks from selected keywords and mark the research task done.
///
/// Single creation path shared by the Tauri command and the pageseeds-cli
/// binary: validates the keywords against the research task's selection
/// artifact, persists one content task per keyword, then transitions the
/// research task to done (user-selection lifecycle complete).
pub fn create_article_tasks_from_keywords(
    conn: &rusqlite::Connection,
    project_id: &str,
    research_task_id: &str,
    keywords: Vec<String>,
) -> Result<Vec<Task>, String> {
    let research_task = crate::engine::task_store::get_task(conn, research_task_id)
        .map_err(|e| e.to_string())?;

    let brief_ctx = load_content_brief_context(conn, project_id, &research_task);

    let tasks = build_content_tasks_from_keywords(
        keywords,
        &research_task,
        research_task_id,
        project_id,
        &brief_ctx,
    )?;

    for task in &tasks {
        crate::engine::task_store::create_task(conn, task).map_err(|e| e.to_string())?;
    }

    // Mark the research task done now that keywords have been dispatched.
    crate::engine::task_store::update_task_status(conn, research_task_id, TaskStatus::Done)
        .map_err(|e| e.to_string())?;

    Ok(tasks)
}

/// Assemble the project-side context needed to build content briefs at task
/// creation time: research themes/territory from the research task's
/// artifacts, plus the project's articles and valid internal-link targets
/// from SQLite. Every piece degrades gracefully to empty when unavailable.
fn load_content_brief_context(
    conn: &rusqlite::Connection,
    project_id: &str,
    research_task: &Task,
) -> ContentBriefContext {
    let articles = crate::engine::task_store::list_articles(conn, project_id).unwrap_or_default();
    // Valid targets = project slugs minus redirected slugs (needs the project
    // path for redirects.csv); without a project row there are no candidates.
    let valid_link_targets = crate::engine::task_store::get_project(conn, project_id)
        .and_then(|p| crate::engine::task_store::load_valid_link_targets(conn, project_id, &p.path))
        .unwrap_or_default();
    let (open_territories, saturated_themes) = extract_territory_summary(research_task);
    ContentBriefContext {
        themes: extract_research_themes(research_task),
        open_territories,
        saturated_themes,
        articles,
        valid_link_targets,
    }
}

/// Metric associated with a keyword from research output.
#[derive(Debug, Clone, Copy)]
pub struct KeywordMetric {
    pub difficulty: Option<i64>,
    pub volume: Option<i64>,
}

/// Metadata for a landing page candidate from the research output.
#[derive(Debug, Clone)]
pub struct LandingPageCandidateMeta {
    pub intent: Option<String>,
    pub landing_page_type: Option<String>,
    pub proposed_title: Option<String>,
    pub opportunity_reason: Option<String>,
}

/// Metadata for an article keyword from the research output — the article-side
/// fields of `SelectedKeyword` in the `research_final_selection` artifact's
/// `results` array (mirrors [`LandingPageCandidateMeta`] for landing pages).
#[derive(Debug, Clone)]
pub struct ArticleKeywordMeta {
    pub intent: Option<String>,
    pub recommended_title: Option<String>,
    pub selection_reason: Option<String>,
    pub winnability: Option<String>,
    pub winnability_reason: Option<String>,
}

/// Maximum number of internal-link candidates shipped with each content brief.
pub const MAX_INTERNAL_LINK_CANDIDATES: usize = 15;

/// A valid internal link target (slug + title) for the article writer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InternalLinkCandidate {
    pub slug: String,
    pub title: String,
}

/// Structured brief attached to every content task at creation time (artifact
/// key `content_brief`). Assembled deterministically from the research task's
/// artifacts and the article store — no LLM call. Landing pages and articles
/// share this single construction path; absent fields are omitted from the
/// serialized JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentBrief {
    pub keyword: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Landing pages only (e.g. alternative|use_case|comparison).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opportunity_reason: Option<String>,
    /// Articles only: "target" | "differentiate" | "avoid".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winnability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winnability_reason: Option<String>,
    /// Articles only: why the research pipeline selected this keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_territories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saturated_themes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub internal_link_candidates: Vec<InternalLinkCandidate>,
}

/// Project-side context needed to assemble content briefs at task creation.
/// Kept separate from the DB so brief construction stays pure and testable.
#[derive(Debug, Clone, Default)]
pub struct ContentBriefContext {
    /// Research themes from the seed extraction step.
    pub themes: Vec<String>,
    /// Open territory names from the territory analysis step.
    pub open_territories: Vec<String>,
    /// Saturated theme names from the territory analysis step.
    pub saturated_themes: Vec<String>,
    /// Project articles (candidates for internal links).
    pub articles: Vec<Article>,
    /// Valid internal link targets (normalized slugs, redirects excluded).
    pub valid_link_targets: HashSet<String>,
}

/// Simple title-case: capitalise the first letter of each word.
pub fn to_title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn normalize_keyword(s: &str) -> String {
    s.trim().to_lowercase()
}

fn push_unique_keyword(out: &mut Vec<String>, seen: &mut HashSet<String>, kw: &str) {
    let trimmed = kw.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = normalize_keyword(trimmed);
    if seen.insert(key) {
        out.push(trimmed.to_string());
    }
}

pub fn parse_range_midpoint(raw: &str) -> Option<i64> {
    let nums: Vec<i64> = raw
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| s.replace(',', "").parse::<i64>().ok())
        .collect();

    match nums.len() {
        0 => None,
        1 => Some(nums[0]),
        _ => Some((nums[0] + nums[1]) / 2),
    }
}

pub fn extract_keywords_from_markdown_table(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    for line in raw.lines() {
        if !line.contains('|') {
            continue;
        }
        if line.contains("---") {
            continue;
        }

        let cols: Vec<String> = line
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        // Expected shape: Priority | Keyword | Vol | KD
        if cols.len() < 4 {
            continue;
        }
        if cols[0].eq_ignore_ascii_case("priority") || cols[1].eq_ignore_ascii_case("keyword") {
            continue;
        }

        // Only accept rows that look like keyword research table entries.
        let _vol = parse_range_midpoint(&cols[2]);
        let _kd = parse_range_midpoint(&cols[3]);
        push_unique_keyword(&mut out, &mut seen, &cols[1]);
    }

    out
}

pub fn extract_selectable_keywords(task: &Task) -> Vec<String> {
    use serde_json::Value;

    let Some(raw) = find_research_selection_artifact(task).and_then(|a| a.content.as_ref()) else {
        return Vec::new();
    };

    // Strip markdown code fences if present (e.g., ```json ... ```)
    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());

    let v = match serde_json::from_str::<Value>(&raw_clean) {
        Ok(v) => v,
        Err(_) => {
            // Fallback for agent markdown-table output when JSON normalization
            // did not produce a structured artifact.
            return extract_keywords_from_markdown_table(raw);
        }
    };

    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    // New unified format: landing_page_candidates (from research_final_selection)
    if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                push_unique_keyword(&mut out, &mut seen, kw);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    if let Some(arr) = v.get("difficulty").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                push_unique_keyword(&mut out, &mut seen, kw);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    if let Some(arr) = v
        .get("difficulty")
        .and_then(|x| x.get("results"))
        .and_then(|x| x.as_array())
    {
        for item in arr {
            if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                push_unique_keyword(&mut out, &mut seen, kw);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    if let Some(arr) = v.get("new_keywords").and_then(|x| x.as_array()) {
        for item in arr.iter().take(10) {
            if let Some(kw) = item.as_str() {
                push_unique_keyword(&mut out, &mut seen, kw);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // Support landing_page_candidates from landing_page_analyze step
    if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                push_unique_keyword(&mut out, &mut seen, kw);
            }
        }
    }

    out
}

pub fn extract_keyword_metrics(task: &Task) -> HashMap<String, KeywordMetric> {
    use serde_json::Value;
    let mut out = HashMap::new();

    let Some(raw) = find_research_selection_artifact(task).and_then(|a| a.content.as_ref()) else {
        return out;
    };

    // Strip markdown code fences if present
    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());
    let parsed_json = serde_json::from_str::<Value>(&raw_clean).ok();
    if let Some(v) = parsed_json {
        // Standard keyword research format
        if let Some(arr) = v
            .get("difficulty")
            .and_then(|x| x.get("results"))
            .and_then(|x| x.as_array())
        {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    let kd = item
                        .get("difficulty")
                        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|n| n.round() as i64)));
                    let vol = item.get("volume").and_then(|x| {
                        x.as_i64()
                            .or_else(|| x.as_str().and_then(parse_range_midpoint))
                    });
                    out.insert(
                        normalize_keyword(kw),
                        KeywordMetric {
                            difficulty: kd,
                            volume: vol,
                        },
                    );
                }
            }
            return out;
        }

        // Landing page analyze format
        if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    let kd = item
                        .get("estimated_kd")
                        .and_then(|x| x.as_i64())
                        .or_else(|| item.get("difficulty").and_then(|x| x.as_i64()));
                    let vol = item
                        .get("estimated_volume")
                        .and_then(|x| x.as_i64())
                        .or_else(|| item.get("volume").and_then(|x| x.as_i64()));
                    out.insert(
                        normalize_keyword(kw),
                        KeywordMetric {
                            difficulty: kd,
                            volume: vol,
                        },
                    );
                }
            }
            return out;
        }
    }

    // Markdown fallback: parse summary table rows.
    for line in raw.lines() {
        if !line.contains('|') || line.contains("---") {
            continue;
        }
        let cols: Vec<String> = line
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        if cols.len() < 4 {
            continue;
        }
        if cols[0].eq_ignore_ascii_case("priority") || cols[1].eq_ignore_ascii_case("keyword") {
            continue;
        }
        let kw = cols[1].clone();
        let vol = parse_range_midpoint(&cols[2]);
        let kd = parse_range_midpoint(&cols[3]);
        out.insert(
            normalize_keyword(&kw),
            KeywordMetric {
                difficulty: kd,
                volume: vol,
            },
        );
    }

    out
}

/// Find the research artifact carrying the final keyword / landing-page
/// selection. Single canonical fallback chain shared by every selection
/// extractor (`extract_selectable_keywords`, `extract_keyword_metrics`,
/// `extract_landing_page_meta`, `extract_article_keyword_meta`) so the chain
/// lives in exactly one place: unified `research_final_selection` first, then
/// the legacy artifacts for backward compatibility.
fn find_research_selection_artifact(task: &Task) -> Option<&TaskArtifact> {
    const KEYS: [&str; 7] = [
        "research_final_selection",
        "research_normalize_stage",
        "landing_page_research_agentic",
        "landing_page_analyze",
        "landing_page_research",
        "research_keywords_cli",
        "research_agent_stage",
    ];
    KEYS.iter()
        .find_map(|key| task.artifacts.iter().find(|a| a.key == *key))
}

/// Extract article keyword metadata (intent, recommended title, selection
/// reason, winnability) keyed by normalized keyword. Mirrors
/// [`extract_landing_page_meta`]: reads the `SelectedKeyword` entries from the
/// `research_final_selection` artifact (`difficulty.results` as stored by the
/// deterministic selection step, with a top-level `results` fallback) so the
/// article writer gets the same research context the spec writer gets.
pub fn extract_article_keyword_meta(task: &Task) -> HashMap<String, ArticleKeywordMeta> {
    use serde_json::Value;
    let mut out = HashMap::new();

    let Some(raw) = find_research_selection_artifact(task).and_then(|a| a.content.as_ref()) else {
        return out;
    };

    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());
    let parsed_json = serde_json::from_str::<Value>(&raw_clean).ok();
    if let Some(v) = parsed_json {
        // Deterministic selection stores results wrapped in a difficulty
        // object; the unified ResearchFinalOutput uses a top-level array.
        let arr = v
            .get("difficulty")
            .and_then(|x| x.get("results"))
            .and_then(|x| x.as_array())
            .or_else(|| v.get("results").and_then(|x| x.as_array()));
        if let Some(arr) = arr {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    out.insert(
                        normalize_keyword(kw),
                        ArticleKeywordMeta {
                            intent: item
                                .get("intent")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            recommended_title: item
                                .get("recommended_title")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            selection_reason: item
                                .get("selection_reason")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            winnability: item
                                .get("winnability")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            winnability_reason: item
                                .get("winnability_reason")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                        },
                    );
                }
            }
        }
    }

    out
}

/// Extract the research themes from the `research_seed_extraction` step
/// artifact (`themes`), falling back to the validated-seed themes from
/// `research_seed_validation`. Already computed by the research pipeline —
/// no LLM call.
pub fn extract_research_themes(task: &Task) -> Vec<String> {
    use serde_json::Value;

    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_seed_extraction")
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "research_seed_validation")
        });
    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return Vec::new();
    };

    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());
    let Ok(v) = serde_json::from_str::<Value>(&raw_clean) else {
        return Vec::new();
    };

    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    if let Some(arr) = v.get("themes").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(theme) = item.as_str() {
                push_unique_keyword(&mut out, &mut seen, theme);
            }
        }
    } else if let Some(arr) = v.get("validated_seeds").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(theme) = item.get("theme").and_then(|x| x.as_str()) {
                push_unique_keyword(&mut out, &mut seen, theme);
            }
        }
    }
    out
}

/// Extract territory context (open territory and saturated theme names) from
/// the `research_territory_analysis` step artifact. Names only, to keep the
/// brief compact. Already computed by the research pipeline — no LLM call.
pub fn extract_territory_summary(task: &Task) -> (Vec<String>, Vec<String>) {
    use serde_json::Value;

    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_territory_analysis");
    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return (Vec::new(), Vec::new());
    };

    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());
    let Ok(v) = serde_json::from_str::<Value>(&raw_clean) else {
        return (Vec::new(), Vec::new());
    };

    let names = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("theme").and_then(|t| t.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    (names("open_territories"), names("saturated_themes"))
}

/// Extract landing page candidate metadata (intent, page type, proposed title,
/// opportunity reason) keyed by normalized keyword. Used to enrich the task
/// description for `create_landing_page` tasks so the spec writer has full context.
pub fn extract_landing_page_meta(task: &Task) -> HashMap<String, LandingPageCandidateMeta> {
    use serde_json::Value;
    let mut out = HashMap::new();

    let Some(raw) = find_research_selection_artifact(task).and_then(|a| a.content.as_ref()) else {
        return out;
    };

    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());
    let parsed_json = serde_json::from_str::<Value>(&raw_clean).ok();
    if let Some(v) = parsed_json {
        if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    out.insert(
                        normalize_keyword(kw),
                        LandingPageCandidateMeta {
                            intent: item
                                .get("intent")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            landing_page_type: item
                                .get("landing_page_type")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            proposed_title: item
                                .get("proposed_title")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                            opportunity_reason: item
                                .get("opportunity_reason")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string()),
                        },
                    );
                }
            }
        }
    }

    out
}

pub fn compute_task_priority(metric: Option<&KeywordMetric>) -> Priority {
    match metric.and_then(|m| m.difficulty) {
        Some(kd) if kd <= 30 => Priority::High,
        Some(kd) if kd <= 45 => Priority::Medium,
        Some(_) => Priority::Low,
        None => match metric.and_then(|m| m.volume) {
            Some(v) if v >= 1000 => Priority::High,
            Some(v) if v >= 250 => Priority::Medium,
            _ => Priority::Medium,
        },
    }
}

/// Assemble the structured brief for one content task. Single construction
/// path for articles and landing pages: metrics + selection metadata from the
/// research artifacts, territory/theme context and internal-link candidates
/// from the brief context. Deterministic — no LLM call.
pub fn build_content_brief(
    keyword: &str,
    metric: Option<&KeywordMetric>,
    lp_meta: Option<&LandingPageCandidateMeta>,
    article_meta: Option<&ArticleKeywordMeta>,
    ctx: &ContentBriefContext,
) -> ContentBrief {
    let mut brief = ContentBrief {
        keyword: keyword.to_string(),
        difficulty: metric.and_then(|m| m.difficulty),
        volume: metric.and_then(|m| m.volume),
        themes: ctx.themes.clone(),
        open_territories: ctx.open_territories.clone(),
        saturated_themes: ctx.saturated_themes.clone(),
        internal_link_candidates: select_internal_link_candidates(
            &ctx.articles,
            &ctx.valid_link_targets,
            keyword,
            MAX_INTERNAL_LINK_CANDIDATES,
        ),
        ..Default::default()
    };
    if let Some(lp) = lp_meta {
        brief.intent = lp.intent.clone();
        brief.page_type = lp.landing_page_type.clone();
        brief.proposed_title = lp.proposed_title.clone();
        brief.opportunity_reason = lp.opportunity_reason.clone();
    }
    if let Some(am) = article_meta {
        brief.intent = am.intent.clone();
        brief.proposed_title = am.recommended_title.clone();
        brief.selection_reason = am.selection_reason.clone();
        brief.winnability = am.winnability.clone();
        brief.winnability_reason = am.winnability_reason.clone();
    }
    brief
}

/// Human-readable one-liner summary of the brief, stored as the task
/// description. Omits lines for fields the research did not provide.
pub fn build_content_task_description(brief: &ContentBrief) -> String {
    let mut description = format!("Target keyword: {}", brief.keyword);
    if let Some(kd) = brief.difficulty {
        description.push_str(&format!("\nKD: {}", kd));
    }
    if let Some(vol) = brief.volume {
        description.push_str(&format!("\nVolume: {}", vol));
    }
    if let Some(ref intent) = brief.intent {
        description.push_str(&format!("\nIntent: {}", intent));
    }
    if let Some(ref page_type) = brief.page_type {
        description.push_str(&format!("\nPage type: {}", page_type));
    }
    if let Some(ref proposed_title) = brief.proposed_title {
        description.push_str(&format!("\nProposed title: {}", proposed_title));
    }
    if let Some(ref winnability) = brief.winnability {
        match brief.winnability_reason {
            Some(ref reason) => {
                description.push_str(&format!("\nWinnability: {} — {}", winnability, reason))
            }
            None => description.push_str(&format!("\nWinnability: {}", winnability)),
        }
    }
    if let Some(ref reason) = brief.opportunity_reason {
        description.push_str(&format!("\nOpportunity: {}", reason));
    }
    if let Some(ref reason) = brief.selection_reason {
        description.push_str(&format!("\nWhy selected: {}", reason));
    }
    description
}

/// Serialize the brief as the `content_brief` task artifact. The executor
/// inlines task artifacts into the agent prompt (`exec/agentic.rs`), so the
/// writer receives intent, winnability, territory, and the valid internal-link
/// candidates without any extra wiring.
pub fn build_content_brief_artifact(brief: &ContentBrief, research_task_id: &str) -> TaskArtifact {
    TaskArtifact {
        key: "content_brief".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(research_task_id.to_string()),
        content: serde_json::to_string_pretty(brief).ok(),
    }
}

/// Pick the top-N internal-link candidates for a keyword: articles whose slug
/// is a valid link target (redirected slugs excluded), ranked by token overlap
/// with the keyword (relevance), then word count (substance), then slug
/// (deterministic tie-break).
pub fn select_internal_link_candidates(
    articles: &[Article],
    valid_link_targets: &HashSet<String>,
    keyword: &str,
    limit: usize,
) -> Vec<InternalLinkCandidate> {
    let keyword_tokens = token_set(keyword);
    let mut scored: Vec<(usize, i64, String, String)> = articles
        .iter()
        .map(|a| (crate::content::slug::normalize_url_slug(&a.url_slug), a))
        .filter(|(slug, _)| valid_link_targets.contains(slug))
        .map(|(slug, a)| {
            let haystack = format!("{} {}", a.target_keyword.as_deref().unwrap_or(""), a.title);
            let relevance = token_set(&haystack).intersection(&keyword_tokens).count();
            (relevance, a.word_count, slug, a.title.clone())
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, slug, title)| InternalLinkCandidate { slug, title })
        .collect()
}

/// Lowercase alphanumeric tokens (length > 1) for keyword/article matching.
fn token_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(|s| s.to_lowercase())
        .collect()
}

pub fn build_keyword_provenance_artifact(keyword: &str, research_task_id: &str) -> TaskArtifact {
    TaskArtifact {
        key: "keyword_research".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(research_task_id.to_string()),
        content: Some(format!(
            "{{\"keyword\":\"{}\"}}",
            keyword.replace('"', "\\\"")
        )),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskArtifact, TaskReviewSurface, TaskRun,
        TaskRunPolicy, TaskStatus,
    };

    fn make_task(artifacts: Vec<TaskArtifact>) -> Task {
        Task {
            id: "test-kw".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Review,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: Some("Keyword test".to_string()),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts,
            run: TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            not_before: None,
        }
    }

    fn artifact(key: &str, content: serde_json::Value) -> TaskArtifact {
        TaskArtifact {
            key: key.to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some(content.to_string()),
        }
    }

    // ── parse_range_midpoint ──────────────────────────────────────────────────

    #[test]
    fn range_midpoint_range_string() {
        assert_eq!(parse_range_midpoint("1,000-10,000"), Some(5500));
    }

    #[test]
    fn range_midpoint_single_number_with_comma() {
        assert_eq!(parse_range_midpoint("1,200"), Some(1200));
    }

    #[test]
    fn range_midpoint_single_plain_number() {
        assert_eq!(parse_range_midpoint("1200"), Some(1200));
    }

    #[test]
    fn range_midpoint_empty_string() {
        assert_eq!(parse_range_midpoint(""), None);
    }

    #[test]
    fn range_midpoint_em_dash_placeholder() {
        assert_eq!(parse_range_midpoint("—"), None);
    }

    #[test]
    fn range_midpoint_second_boundary_of_range() {
        assert_eq!(parse_range_midpoint("5,000-10,000"), Some(7500));
    }

    // ── extract_keywords_from_markdown_table ──────────────────────────────────

    #[test]
    fn markdown_table_extracts_keyword_column() {
        let raw = "| Priority | Keyword | Volume | KD |\n\
                   |---|---|---|---|\n\
                   | High | seo tools | 5,000-10,000 | 30 |\n\
                   | Medium | content marketing | 1,000-5,000 | 45 |\n";
        let kws = extract_keywords_from_markdown_table(raw);
        assert!(kws.contains(&"seo tools".to_string()), "got: {kws:?}");
        assert!(
            kws.contains(&"content marketing".to_string()),
            "got: {kws:?}"
        );
    }

    #[test]
    fn markdown_table_skips_header_row() {
        let raw = "| Priority | Keyword | Volume | KD |\n|---|---|---|---|\n";
        assert!(extract_keywords_from_markdown_table(raw).is_empty());
    }

    #[test]
    fn markdown_table_deduplicates_keywords() {
        let raw = "| Priority | Keyword | Volume | KD |\n\
                   |---|---|---|---|\n\
                   | High | seo tools | 1,000 | 30 |\n\
                   | High | seo tools | 2,000 | 35 |\n";
        let kws = extract_keywords_from_markdown_table(raw);
        assert_eq!(kws.iter().filter(|k| k.as_str() == "seo tools").count(), 1);
    }

    // ── extract_selectable_keywords ───────────────────────────────────────────

    #[test]
    fn selectable_reads_difficulty_results_array() {
        let json = serde_json::json!({
            "difficulty": {
                "results": [
                    {"keyword": "seo tools", "difficulty": 30, "volume": "5,000-10,000"},
                    {"keyword": "content strategy", "difficulty": 45, "volume": "1,000-5,000"},
                ]
            }
        });
        let task = make_task(vec![artifact("research_keywords_cli", json)]);
        let kws = extract_selectable_keywords(&task);
        assert!(kws.contains(&"seo tools".to_string()), "got: {kws:?}");
        assert!(
            kws.contains(&"content strategy".to_string()),
            "got: {kws:?}"
        );
    }

    #[test]
    fn selectable_falls_back_to_new_keywords() {
        let json = serde_json::json!({
            "new_keywords": ["keyword a", "keyword b", "keyword c"]
        });
        let task = make_task(vec![artifact("research_keywords_cli", json)]);
        let kws = extract_selectable_keywords(&task);
        assert!(kws.contains(&"keyword a".to_string()), "got: {kws:?}");
        assert_eq!(kws.len(), 3);
    }

    #[test]
    fn selectable_prefers_normalize_stage_over_cli_artifact() {
        let cli_json = serde_json::json!({
            "difficulty": {"results": [{"keyword": "from_cli", "difficulty": 20, "volume": "500"}]}
        });
        let norm_json = serde_json::json!({
            "difficulty": {"results": [{"keyword": "from_normalizer", "difficulty": 15, "volume": "1000"}]}
        });
        let task = make_task(vec![
            artifact("research_keywords_cli", cli_json),
            artifact("research_normalize_stage", norm_json),
        ]);
        let kws = extract_selectable_keywords(&task);
        assert!(kws.contains(&"from_normalizer".to_string()), "got: {kws:?}");
        assert!(!kws.contains(&"from_cli".to_string()), "got: {kws:?}");
    }

    #[test]
    fn selectable_empty_for_no_artifacts() {
        let task = make_task(vec![]);
        assert!(extract_selectable_keywords(&task).is_empty());
    }

    // ── extract_keyword_metrics ───────────────────────────────────────────────

    #[test]
    fn metrics_reads_difficulty_and_volume_midpoint() {
        let json = serde_json::json!({
            "difficulty": {
                "results": [
                    {"keyword": "seo tools", "difficulty": 28, "volume": "5,000-10,000"},
                ]
            }
        });
        let task = make_task(vec![artifact("research_keywords_cli", json)]);
        let metrics = extract_keyword_metrics(&task);
        let m = metrics.get("seo tools").expect("metric not found");
        assert_eq!(m.difficulty, Some(28));
        assert_eq!(m.volume, Some(7500)); // midpoint of 5000–10000
    }

    #[test]
    fn metrics_handles_null_difficulty() {
        let json = serde_json::json!({
            "difficulty": {
                "results": [
                    {"keyword": "hard keyword", "difficulty": null, "volume": "1,000-5,000"},
                ]
            }
        });
        let task = make_task(vec![artifact("research_keywords_cli", json)]);
        let metrics = extract_keyword_metrics(&task);
        let m = metrics.get("hard keyword").expect("metric not found");
        assert_eq!(m.difficulty, None);
        assert_eq!(m.volume, Some(3000)); // midpoint of 1000–5000
    }

    #[test]
    fn metrics_empty_for_no_artifacts() {
        let task = make_task(vec![]);
        assert!(extract_keyword_metrics(&task).is_empty());
    }

    // ── normalize_keyword ─────────────────────────────────────────────────────

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize_keyword("  SEO Tools  "), "seo tools");
        assert_eq!(normalize_keyword("Content Marketing"), "content marketing");
    }

    // ── to_title_case ─────────────────────────────────────────────────────────

    #[test]
    fn debug_research_output_format() {
        // Sample output from research_ahrefs_pipeline to verify format
        let sample = r#"{
            "keywords": [
                {"keyword": "test", "volume": 100, "kd": 25.0, "traffic": 500.0, "has_data": true}
            ],
            "themes": ["test"],
            "total_candidates": 1,
            "with_data_count": 1
        }"#;

        let parsed: serde_json::Value = serde_json::from_str(sample).unwrap();

        // Verify the format
        if let Some(keywords) = parsed.get("keywords").and_then(|k| k.as_array()) {
            if let Some(first) = keywords.first() {
                println!("Keyword: {:?}", first.get("keyword"));
                println!("Volume: {:?}", first.get("volume"));
                println!("KD: {:?}", first.get("kd"));
                println!("Traffic: {:?}", first.get("traffic"));
                println!("Has data: {:?}", first.get("has_data"));
            }
        }
    }

    #[test]
    fn title_case_capitalizes_each_word() {
        assert_eq!(to_title_case("seo tools guide"), "Seo Tools Guide");
        assert_eq!(to_title_case("content"), "Content");
    }

    // ── Article briefs ────────────────────────────────────────────────────────

    fn make_article(slug: &str, title: &str, target_keyword: Option<&str>, word_count: i64) -> Article {
        Article {
            id: 0,
            title: title.to_string(),
            url_slug: slug.to_string(),
            file: format!("{slug}.mdx"),
            target_keyword: target_keyword.map(|s| s.to_string()),
            keyword_difficulty: None,
            target_volume: 0,
            published_date: None,
            word_count,
            status: "published".to_string(),
            review_status: None,
            review_started_at: None,
            last_reviewed_at: None,
            review_count: 0,
            content_gaps_addressed: vec![],
            estimated_traffic_monthly: None,
            project_id: "proj1".to_string(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
            page_type: None,
            content_hash: None,
            last_edited_at: None,
        }
    }

    /// Artifact shaped like the real `research_final_selection` step output for
    /// article research: SelectedKeyword entries under `difficulty.results`.
    fn final_selection_artifact() -> TaskArtifact {
        artifact(
            "research_final_selection",
            serde_json::json!({
                "difficulty": {
                    "total": 1,
                    "successful": 1,
                    "results": [
                        {
                            "keyword": "covered call strategy",
                            "volume": 2400,
                            "difficulty": 22,
                            "selection_reason": "KD 22 with 2400 monthly searches",
                            "recommended_title": "Covered Call Strategy for Small Accounts (2026)",
                            "intent": "informational",
                            "winnability": "target",
                            "winnability_reason": "No AI Overview, weak SERP incumbents"
                        }
                    ]
                },
                "total_candidates": 10,
                "filtered_out": 9
            }),
        )
    }

    #[test]
    fn article_meta_reads_difficulty_results() {
        let task = make_task(vec![final_selection_artifact()]);
        let meta = extract_article_keyword_meta(&task);
        let m = meta.get("covered call strategy").expect("meta not found");
        assert_eq!(m.intent.as_deref(), Some("informational"));
        assert_eq!(
            m.recommended_title.as_deref(),
            Some("Covered Call Strategy for Small Accounts (2026)")
        );
        assert_eq!(m.winnability.as_deref(), Some("target"));
        assert_eq!(
            m.winnability_reason.as_deref(),
            Some("No AI Overview, weak SERP incumbents")
        );
        assert_eq!(
            m.selection_reason.as_deref(),
            Some("KD 22 with 2400 monthly searches")
        );
    }

    #[test]
    fn article_meta_reads_top_level_results_fallback() {
        let json = serde_json::json!({
            "results": [
                {"keyword": "iron condor", "selection_reason": "picked", "recommended_title": "Iron Condor Guide"}
            ]
        });
        let task = make_task(vec![artifact("research_final_selection", json)]);
        let meta = extract_article_keyword_meta(&task);
        assert!(meta.contains_key("iron condor"), "got: {meta:?}");
    }

    #[test]
    fn article_meta_empty_for_landing_page_artifact() {
        let json = serde_json::json!({
            "landing_page_candidates": [{"keyword": "pricing", "intent": "transactional"}]
        });
        let task = make_task(vec![artifact("research_final_selection", json)]);
        assert!(extract_article_keyword_meta(&task).is_empty());
    }

    #[test]
    fn themes_from_seed_extraction() {
        let json = serde_json::json!({"themes": ["options income", "covered calls"], "competitors": []});
        let task = make_task(vec![artifact("research_seed_extraction", json)]);
        assert_eq!(extract_research_themes(&task), vec!["options income", "covered calls"]);
    }

    #[test]
    fn themes_fall_back_to_validated_seeds() {
        let json = serde_json::json!({"validated_seeds": [{"theme": "wheel strategy", "seeds": ["wheel"]}]});
        let task = make_task(vec![artifact("research_seed_validation", json)]);
        assert_eq!(extract_research_themes(&task), vec!["wheel strategy"]);
    }

    #[test]
    fn territory_summary_reads_theme_names() {
        let json = serde_json::json!({
            "open_territories": [{"theme": "cash secured puts", "article_count": 1, "total_impressions": 6000.0, "source_keywords": []}],
            "saturated_themes": [{"theme": "covered calls", "article_count": 7, "total_impressions": 900.0, "source_keywords": []}],
            "total_themes": 4,
            "synced_to_shortlist": 2
        });
        let task = make_task(vec![artifact("research_territory_analysis", json)]);
        let (open, saturated) = extract_territory_summary(&task);
        assert_eq!(open, vec!["cash secured puts"]);
        assert_eq!(saturated, vec!["covered calls"]);
    }

    #[test]
    fn link_candidates_filtered_to_valid_targets_and_ranked_by_relevance() {
        let articles = vec![
            make_article("what-is-a-covered-call", "What Is a Covered Call?", Some("covered call basics"), 900),
            make_article("unrelated-post", "Unrelated Post", Some("dividend aristocrats"), 5000),
            make_article("redirected-slug", "Redirected", Some("covered call strategy"), 3000),
        ];
        let valid: HashSet<String> = ["what-is-a-covered-call", "unrelated-post"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let candidates = select_internal_link_candidates(&articles, &valid, "covered call strategy", 15);
        // Redirected slug is not a valid target and must be excluded.
        assert_eq!(candidates.len(), 2);
        // Relevance (token overlap with the keyword) outranks raw word count.
        assert_eq!(candidates[0].slug, "what-is-a-covered-call");
        assert_eq!(candidates[1].slug, "unrelated-post");
    }

    #[test]
    fn link_candidates_respect_limit() {
        let articles: Vec<Article> = (0..20)
            .map(|i| make_article(&format!("post-{i:02}"), &format!("Post {i}"), None, 100))
            .collect();
        let valid: HashSet<String> = articles.iter().map(|a| a.url_slug.clone()).collect();
        let candidates = select_internal_link_candidates(&articles, &valid, "anything", 15);
        assert_eq!(candidates.len(), 15);
    }

    #[test]
    fn brief_includes_article_fields_territory_and_candidates() {
        let task = make_task(vec![
            final_selection_artifact(),
            artifact("research_seed_extraction", serde_json::json!({"themes": ["options income"]})),
        ]);
        let meta = extract_article_keyword_meta(&task);
        let ctx = ContentBriefContext {
            themes: extract_research_themes(&task),
            articles: vec![make_article("cc-basics", "Covered Call Basics", Some("covered calls"), 800)],
            valid_link_targets: ["cc-basics".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let brief = build_content_brief(
            "covered call strategy",
            None,
            None,
            meta.get("covered call strategy"),
            &ctx,
        );
        assert_eq!(brief.intent.as_deref(), Some("informational"));
        assert_eq!(brief.winnability.as_deref(), Some("target"));
        assert_eq!(
            brief.proposed_title.as_deref(),
            Some("Covered Call Strategy for Small Accounts (2026)")
        );
        assert_eq!(brief.themes, vec!["options income"]);
        assert_eq!(brief.internal_link_candidates.len(), 1);
        assert_eq!(brief.internal_link_candidates[0].slug, "cc-basics");

        let description = build_content_task_description(&brief);
        assert!(description.contains("Target keyword: covered call strategy"));
        assert!(description.contains("Intent: informational"));
        assert!(description.contains("Proposed title: Covered Call Strategy for Small Accounts (2026)"));
        assert!(description.contains("Winnability: target — No AI Overview, weak SERP incumbents"));
        assert!(description.contains("Why selected: KD 22 with 2400 monthly searches"));
    }

    #[test]
    fn brief_omits_missing_fields_gracefully() {
        let brief = build_content_brief(
            "bare keyword",
            None,
            None,
            None,
            &ContentBriefContext::default(),
        );
        let description = build_content_task_description(&brief);
        assert_eq!(description, "Target keyword: bare keyword");

        // Absent optionals must not appear in the serialized artifact either.
        let json = serde_json::to_string(&brief).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("intent").is_none());
        assert!(v.get("winnability").is_none());
        assert!(v.get("proposed_title").is_none());
        assert!(v.get("internal_link_candidates").is_none());
    }

    #[test]
    fn content_tasks_carry_content_brief_artifact() {
        let research_task = make_task(vec![final_selection_artifact()]);
        let ctx = ContentBriefContext {
            articles: vec![make_article("cc-basics", "Covered Call Basics", Some("covered calls"), 800)],
            valid_link_targets: ["cc-basics".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let tasks = build_content_tasks_from_keywords(
            vec!["covered call strategy".to_string()],
            &research_task,
            "research-1",
            "proj1",
            &ctx,
        )
        .expect("tasks");
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.task_type, "write_article");
        let description = task.description.as_deref().unwrap_or_default();
        assert!(description.contains("Intent: informational"));
        assert!(description.contains("Winnability: target"));

        let brief_artifact = task
            .artifacts
            .iter()
            .find(|a| a.key == "content_brief")
            .expect("content_brief artifact");
        let brief: ContentBrief =
            serde_json::from_str(brief_artifact.content.as_deref().unwrap()).unwrap();
        assert_eq!(brief.keyword, "covered call strategy");
        assert_eq!(brief.difficulty, Some(22));
        assert_eq!(brief.volume, Some(2400));
        assert_eq!(brief.internal_link_candidates.len(), 1);
        assert_eq!(brief.internal_link_candidates[0].slug, "cc-basics");
    }
}
