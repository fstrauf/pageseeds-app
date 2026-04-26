use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskRun, TaskStatus};
use std::collections::{HashMap, HashSet};

/// Build content tasks from selected keywords and mark the research task as done.
/// Validates inputs, deduplicates, and constructs task specs.
pub fn build_content_tasks_from_keywords(
    requested_keywords: Vec<String>,
    research_task: &Task,
    research_task_id: &str,
    project_id: &str,
) -> Result<Vec<Task>, String> {
    use crate::config::{default_execution_mode, default_phase};

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
        return Err("No selectable keywords found on the research task. Re-run keyword research first.".to_string());
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

    let mut created = Vec::new();
    for (idx, keyword) in requested_keywords.iter().enumerate() {
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!("task-{}-{}", chrono::Utc::now().timestamp_millis(), idx);
        let title = to_title_case(keyword);
        let metric = metrics.get(&normalize_keyword(keyword));
        let priority_enum = compute_task_priority(metric);
        let description = build_content_task_description(
            keyword,
            metric,
            lp_meta.get(&normalize_keyword(keyword)),
        );
        let provenance = build_keyword_provenance_artifact(keyword, research_task_id);

        let task = Task {
            id,
            phase: default_phase(content_task_type).to_string(),
            execution_mode: default_execution_mode(content_task_type),
            task_type: content_task_type.to_string(),
            status: TaskStatus::Todo,
            priority: priority_enum,
            agent_policy: AgentPolicy::None,
            title: Some(title),
            description: Some(description),
            project_id: project_id.to_string(),
            depends_on: vec![research_task_id.to_string()],
            artifacts: vec![provenance],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
        };
        created.push(task);
    }

    Ok(created)
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

    // New unified workflow: research_final_selection
    // Legacy artifacts for backward compatibility
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_final_selection")
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_normalize_stage"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_research_agentic"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_analyze"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_research"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_keywords_cli"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_agent_stage"));

    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return Vec::new();
    };

    // Strip markdown code fences if present (e.g., ```json ... ```)
    let raw_clean = extract_json_from_markdown(raw);

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

    // New unified workflow: research_final_selection
    // Legacy artifacts for backward compatibility
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_final_selection")
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_normalize_stage"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_research_agentic"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_analyze"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_research"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_keywords_cli"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_agent_stage"));

    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return out;
    };

    // Strip markdown code fences if present
    let raw_clean = extract_json_from_markdown(raw);
    let parsed_json = serde_json::from_str::<Value>(&raw_clean).ok();
    if let Some(v) = parsed_json {
        // Standard keyword research format
        if let Some(arr) = v.get("difficulty").and_then(|x| x.get("results")).and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    let kd = item.get("difficulty").and_then(|x| {
                        x.as_i64().or_else(|| x.as_f64().map(|n| n.round() as i64))
                    });
                    let vol = item.get("volume").and_then(|x| {
                        x.as_i64().or_else(|| x.as_str().and_then(parse_range_midpoint))
                    });
                    out.insert(normalize_keyword(kw), KeywordMetric { difficulty: kd, volume: vol });
                }
            }
            return out;
        }

        // Landing page analyze format
        if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    let kd = item.get("estimated_kd")
                        .and_then(|x| x.as_i64())
                        .or_else(|| item.get("difficulty").and_then(|x| x.as_i64()));
                    let vol = item.get("estimated_volume")
                        .and_then(|x| x.as_i64())
                        .or_else(|| item.get("volume").and_then(|x| x.as_i64()));
                    out.insert(normalize_keyword(kw), KeywordMetric { difficulty: kd, volume: vol });
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
        out.insert(normalize_keyword(&kw), KeywordMetric { difficulty: kd, volume: vol });
    }

    out
}

/// Extract landing page candidate metadata (intent, page type, proposed title,
/// opportunity reason) keyed by normalized keyword. Used to enrich the task
/// description for `create_landing_page` tasks so the spec writer has full context.
pub fn extract_landing_page_meta(task: &Task) -> HashMap<String, LandingPageCandidateMeta> {
    use serde_json::Value;
    let mut out = HashMap::new();

    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_final_selection")
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_normalize_stage"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_research_agentic"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_analyze"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "landing_page_research"));

    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return out;
    };

    let raw_clean = extract_json_from_markdown(raw);
    let parsed_json = serde_json::from_str::<Value>(&raw_clean).ok();
    if let Some(v) = parsed_json {
        if let Some(arr) = v.get("landing_page_candidates").and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                    out.insert(normalize_keyword(kw), LandingPageCandidateMeta {
                        intent: item.get("intent").and_then(|x| x.as_str()).map(|s| s.to_string()),
                        landing_page_type: item.get("landing_page_type").and_then(|x| x.as_str()).map(|s| s.to_string()),
                        proposed_title: item.get("proposed_title").and_then(|x| x.as_str()).map(|s| s.to_string()),
                        opportunity_reason: item.get("opportunity_reason").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    });
                }
            }
        }
    }

    out
}

/// Extract JSON from markdown code fences if present.
/// Handles ```json ... ``` and ``` ... ``` wrappers.
pub fn extract_json_from_markdown(content: &str) -> String {
    // Try to find JSON in code fences
    if let Some(start) = content.find("```json") {
        let after_start = start + 7; // len of "```json"
        if let Some(end) = content[after_start..].find("```") {
            return content[after_start..after_start + end].trim().to_string();
        }
    }

    // Try generic code block
    if let Some(start) = content.find("```") {
        let after_start = start + 3; // len of "```"
        if let Some(end) = content[after_start..].find("```") {
            // Strip language identifier if present (e.g., "json\n{...}")
            let block = &content[after_start..after_start + end];
            return block.trim_start_matches("json").trim().to_string();
        }
    }

    // No code fences found, return as-is
    content.trim().to_string()
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

pub fn build_content_task_description(
    keyword: &str,
    metric: Option<&KeywordMetric>,
    lp_meta: Option<&LandingPageCandidateMeta>,
) -> String {
    let mut description = format!("Target keyword: {}", keyword);
    if let Some(m) = metric {
        if let Some(kd) = m.difficulty {
            description.push_str(&format!("\nKD: {}", kd));
        }
        if let Some(vol) = m.volume {
            description.push_str(&format!("\nVolume: {}", vol));
        }
    }
    // Append landing page metadata so the spec writer can use it.
    if let Some(lp) = lp_meta {
        if let Some(ref intent) = lp.intent {
            description.push_str(&format!("\nIntent: {}", intent));
        }
        if let Some(ref page_type) = lp.landing_page_type {
            description.push_str(&format!("\nPage type: {}", page_type));
        }
        if let Some(ref proposed_title) = lp.proposed_title {
            description.push_str(&format!("\nProposed title: {}", proposed_title));
        }
        if let Some(ref reason) = lp.opportunity_reason {
            description.push_str(&format!("\nOpportunity: {}", reason));
        }
    }
    description
}

pub fn build_keyword_provenance_artifact(keyword: &str, research_task_id: &str) -> TaskArtifact {
    TaskArtifact {
        key: "keyword_research".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(research_task_id.to_string()),
        content: Some(format!("{{\"keyword\":\"{}\"}}", keyword.replace('"', "\\\""))),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, ExecutionMode, Priority, Task, TaskArtifact, TaskRun, TaskStatus,
    };

    fn make_task(artifacts: Vec<TaskArtifact>) -> Task {
        Task {
            id: "test-kw".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Review,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Manual,
            agent_policy: AgentPolicy::Optional,
            title: Some("Keyword test".to_string()),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts,
            run: TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
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
        assert!(kws.contains(&"content marketing".to_string()), "got: {kws:?}");
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
        assert!(kws.contains(&"content strategy".to_string()), "got: {kws:?}");
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
}
