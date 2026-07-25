use crate::engine::content_brief::{
    build_content_brief, build_content_brief_artifact, build_content_task_description,
    extract_article_keyword_meta, load_content_brief_context, ContentBriefContext,
};
use crate::engine::spawner::TaskSpec;
use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskStatus};
use std::collections::{HashMap, HashSet};

/// Build content task specs from selected keywords for the research task.
/// Validates inputs, deduplicates, and constructs spawner specs — IDs, phase,
/// status, and lifecycle defaults are resolved by the spawner.
pub fn build_content_tasks_from_keywords(
    requested_keywords: Vec<String>,
    research_task: &Task,
    research_task_id: &str,
    project_id: &str,
    brief_ctx: &ContentBriefContext,
) -> Result<Vec<TaskSpec>, String> {
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

    let mut specs = Vec::new();
    for keyword in requested_keywords.iter() {
        let title = to_title_case(keyword);
        let normalized = normalize_keyword(keyword);
        let metric = metrics.get(&normalized);
        let priority_enum = compute_task_priority(metric);
        let brief = build_content_brief(
            keyword,
            metric,
            lp_meta.get(&normalized),
            article_meta.get(&normalized),
            brief_ctx,
        );
        let description = build_content_task_description(&brief);
        let provenance = build_keyword_provenance_artifact(keyword, research_task_id);
        let brief_artifact = build_content_brief_artifact(&brief, research_task_id);

        // Deterministic idempotency key per keyword: re-selecting a keyword whose
        // write task is still active returns the existing task (SkipIfActive).
        let idempotency_key = format!("{content_task_type}:{project_id}:{normalized}");

        specs.push(TaskSpec {
            project_id: project_id.to_string(),
            task_type: content_task_type.to_string(),
            title: Some(title),
            description: Some(description),
            priority: priority_enum,
            agent_policy: AgentPolicy::None,
            depends_on: vec![research_task_id.to_string()],
            artifacts: vec![provenance, brief_artifact],
            idempotency_key: Some(idempotency_key),
            ..Default::default()
        });
    }

    Ok(specs)
}

/// Create content tasks from selected keywords and mark the research task done.
///
/// Single creation path shared by the Tauri command and the pageseeds-cli
/// binary: validates the keywords against the research task's selection
/// artifact, spawns one content task per keyword via the TaskSpawner
/// (idempotent per keyword), then transitions the research task to done
/// (user-selection lifecycle complete).
pub fn create_article_tasks_from_keywords(
    conn: &rusqlite::Connection,
    project_id: &str,
    research_task_id: &str,
    keywords: Vec<String>,
) -> Result<Vec<Task>, String> {
    let research_task = crate::engine::task_store::get_task(conn, research_task_id)
        .map_err(|e| e.to_string())?;

    let brief_ctx = load_content_brief_context(conn, project_id, &research_task);

    let picked_keywords = keywords.clone();
    let specs = build_content_tasks_from_keywords(
        keywords,
        &research_task,
        research_task_id,
        project_id,
        &brief_ctx,
    )?;

    let mut tasks = Vec::with_capacity(specs.len());
    for spec in specs {
        let task = crate::engine::spawner::TaskSpawner::spawn(conn, spec)
            .map_err(|e| e.to_string())?;
        tasks.push(task);
    }

    // Best-effort coverage feedback (issue #23): mark research_shortlist rows
    // whose theme/seeds match a picked keyword as covered. A keyword that
    // matches nothing is a no-op — this must never fail the selection.
    match crate::db::research_shortlist::mark_covered_for_keywords(conn, project_id, &picked_keywords)
    {
        Ok(n) if n > 0 => log::info!(
            "[keyword_selection] marked {} research_shortlist entrie(s) covered",
            n
        ),
        Ok(_) => {}
        Err(e) => log::warn!(
            "[keyword_selection] mark_covered_for_keywords failed (non-fatal): {}",
            e
        ),
    }

    // Mark the research task done now that keywords have been dispatched.
    crate::engine::task_store::update_task_status(conn, research_task_id, TaskStatus::Done)
        .map_err(|e| e.to_string())?;

    Ok(tasks)
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

/// Normalize a keyword for dedup and idempotency keys. Delegates to the
/// canonical normalizer (strips quotes, collapses whitespace, lowercases).
pub fn normalize_keyword(s: &str) -> String {
    crate::content::keyword_match::normalize_keyword(s)
}

pub(crate) fn push_unique_keyword(out: &mut Vec<String>, seen: &mut HashSet<String>, kw: &str) {
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
    let mut out = HashMap::new();

    let Some(raw) = find_research_selection_artifact(task).and_then(|a| a.content.as_ref()) else {
        return out;
    };

    if let Some(v) = parse_artifact_json(task, find_research_selection_artifact(task)) {
        // Standard keyword research format
        if let Some(arr) = v
            .get("difficulty")
            .and_then(|x| x.get("results"))
            .and_then(|x| x.as_array())
        {
            return meta_by_keyword(arr, |item| {
                let kw = item.get("keyword")?.as_str()?;
                let kd = item
                    .get("difficulty")
                    .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|n| n.round() as i64)));
                let vol = item.get("volume").and_then(|x| {
                    x.as_i64()
                        .or_else(|| x.as_str().and_then(parse_range_midpoint))
                });
                Some((normalize_keyword(kw), KeywordMetric { difficulty: kd, volume: vol }))
            });
        }

        // Landing page analyze format
        if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
            return meta_by_keyword(arr, |item| {
                let kw = item.get("keyword")?.as_str()?;
                let kd = item
                    .get("estimated_kd")
                    .and_then(|x| x.as_i64())
                    .or_else(|| item.get("difficulty").and_then(|x| x.as_i64()));
                let vol = item
                    .get("estimated_volume")
                    .and_then(|x| x.as_i64())
                    .or_else(|| item.get("volume").and_then(|x| x.as_i64()));
                Some((normalize_keyword(kw), KeywordMetric { difficulty: kd, volume: vol }))
            });
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
/// `extract_landing_page_meta`, `content_brief::extract_article_keyword_meta`)
/// so the chain lives in exactly one place: unified `research_final_selection`
/// first, then the legacy artifacts for backward compatibility.
pub(crate) fn find_research_selection_artifact(task: &Task) -> Option<&TaskArtifact> {
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

/// Clean (strip markdown code fences) and parse an artifact's JSON content.
/// Shared preamble for all artifact-metadata extractors; returns `None` when
/// the artifact is missing, has no inline content, or does not parse.
pub(crate) fn parse_artifact_json(
    _task: &Task,
    artifact: Option<&TaskArtifact>,
) -> Option<serde_json::Value> {
    let raw = artifact.and_then(|a| a.content.as_ref())?;
    let raw_clean =
        crate::engine::text::extract_json_string(raw).unwrap_or_else(|| raw.trim().to_string());
    serde_json::from_str::<serde_json::Value>(&raw_clean).ok()
}

/// Collect per-keyword metadata from a JSON array into a map keyed by
/// normalized keyword. Shared loop for all artifact-metadata extractors;
/// `map` returns `None` for entries without a usable keyword.
pub(crate) fn meta_by_keyword<T>(
    arr: &[serde_json::Value],
    map: impl Fn(&serde_json::Value) -> Option<(String, T)>,
) -> HashMap<String, T> {
    arr.iter().filter_map(map).collect()
}

/// Extract landing page candidate metadata (intent, page type, proposed title,
/// opportunity reason) keyed by normalized keyword. Used to enrich the task
/// description for `create_landing_page` tasks so the landing page writer has
/// full context.
pub fn extract_landing_page_meta(task: &Task) -> HashMap<String, LandingPageCandidateMeta> {
    let Some(v) = parse_artifact_json(task, find_research_selection_artifact(task)) else {
        return HashMap::new();
    };
    let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) else {
        return HashMap::new();
    };
    meta_by_keyword(arr, |item| {
        let kw = item.get("keyword")?.as_str()?;
        let meta = LandingPageCandidateMeta {
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
        };
        Some((normalize_keyword(kw), meta))
    })
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
mod tests;
