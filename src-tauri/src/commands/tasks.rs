use tauri::State;
use crate::engine::task_store;
use crate::models::task::{AgentPolicy, ExecutionMode, Priority, Task, TaskStatus};
use super::AppState;
use std::collections::HashSet;

#[tauri::command]
pub fn list_tasks(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<String>,
    phase: Option<String>,
) -> Result<Vec<Task>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::list_tasks_filtered(&db, &project_id, status.as_deref(), phase.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_task(state: State<'_, AppState>, id: String) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::get_task(&db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_task(
    state: State<'_, AppState>,
    project_id: String,
    task_type: String,
    title: Option<String>,
    description: Option<String>,
    priority: String,
) -> Result<Task, String> {
    use crate::config::{default_execution_mode, default_phase};

    let now = chrono::Utc::now().to_rfc3339();
    let id = format!(
        "task-{}",
        chrono::Utc::now().timestamp_millis().to_string()
    );
    let priority_enum = match priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let task = Task {
        id,
        phase: default_phase(&task_type).to_string(),
        execution_mode: default_execution_mode(&task_type),
        task_type,
        status: TaskStatus::Todo,
        priority: priority_enum,
        agent_policy: AgentPolicy::None,
        title,
        description,
        project_id,
        depends_on: vec![],
        artifacts: vec![],
        run: crate::models::task::TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::create_task(&db, &task).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_task_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let status_enum = match status.as_str() {
        "in_progress" => TaskStatus::InProgress,
        "review" => TaskStatus::Review,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Todo,
    };
    task_store::update_task_status(&db, &id, status_enum).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_task(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    description: Option<String>,
    priority: String,
) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let priority_enum = match priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };
    task_store::update_task(&db, &id, title.as_deref(), description.as_deref(), priority_enum)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::delete_task(&db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_task(state: State<'_, AppState>, id: String) -> Result<Task, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::update_task_status(&db, &id, TaskStatus::Cancelled).map_err(|e| e.to_string())
}

/// Create one `write_article` task per selected keyword and mark the research
/// task as done.  Each keyword string becomes both the task title and the
/// target keyword in the description so the article-writing agent has context.
#[tauri::command]
pub fn create_article_tasks_from_keywords(
    state: State<'_, AppState>,
    project_id: String,
    research_task_id: String,
    keywords: Vec<String>,
) -> Result<Vec<Task>, String> {
    use crate::config::{default_execution_mode, default_phase};
    use crate::models::task::{TaskArtifact, TaskRun};

    let db = state.db.lock().map_err(|e| e.to_string())?;
    let research_task = task_store::get_task(&db, &research_task_id).map_err(|e| e.to_string())?;
    if research_task.project_id != project_id {
        return Err("Research task does not belong to this project".to_string());
    }

    let allowed_keywords = extract_selectable_keywords(&research_task);
    if allowed_keywords.is_empty() {
        return Err("No selectable keywords found on the research task. Re-run keyword research first.".to_string());
    }

    let mut seen_requested = HashSet::new();
    let requested_keywords: Vec<String> = keywords
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

    let mut created = Vec::new();

    let metrics = extract_keyword_metrics(&research_task);

    for keyword in &requested_keywords {
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!("task-{}", chrono::Utc::now().timestamp_millis());
        let title = to_title_case(keyword);
        let metric = metrics.get(&normalize_keyword(keyword));
        let priority_enum = match metric.and_then(|m| m.difficulty) {
            Some(kd) if kd <= 30 => Priority::High,
            Some(kd) if kd <= 45 => Priority::Medium,
            Some(_) => Priority::Low,
            None => {
                match metric.and_then(|m| m.volume) {
                    Some(v) if v >= 1000 => Priority::High,
                    Some(v) if v >= 250 => Priority::Medium,
                    _ => Priority::Medium,
                }
            }
        };

        let mut description = format!("Target keyword: {}", keyword);
        if let Some(m) = metric {
            if let Some(kd) = m.difficulty {
                description.push_str(&format!("\nKD: {}", kd));
            }
            if let Some(vol) = m.volume {
                description.push_str(&format!("\nVolume: {}", vol));
            }
        }

        let provenance = TaskArtifact {
            key: "keyword_research".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some(research_task_id.clone()),
            content: Some(format!("{{\"keyword\":\"{}\"}}", keyword.replace('"', "\\\""))),
        };

        let task = Task {
            id,
            phase: default_phase("write_article").to_string(),
            execution_mode: default_execution_mode("write_article"),
            task_type: "write_article".to_string(),
            status: TaskStatus::Todo,
            priority: priority_enum,
            agent_policy: AgentPolicy::None,
            title: Some(title),
            description: Some(description),
            project_id: project_id.clone(),
            depends_on: vec![research_task_id.clone()],
            artifacts: vec![provenance],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
        };
        task_store::create_task(&db, &task).map_err(|e| e.to_string())?;
        created.push(task);
    }

    // Mark the research task done now that keywords have been dispatched.
    task_store::update_task_status(&db, &research_task_id, TaskStatus::Done)
        .map_err(|e| e.to_string())?;

    Ok(created)
}

/// Simple title-case: capitalise the first letter of each word.
fn to_title_case(s: &str) -> String {
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

fn normalize_keyword(s: &str) -> String {
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

fn parse_range_midpoint(raw: &str) -> Option<i64> {
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

fn extract_keywords_from_markdown_table(raw: &str) -> Vec<String> {
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

fn extract_selectable_keywords(task: &Task) -> Vec<String> {
    use serde_json::Value;

    // Prefer normalized output, then deterministic JSON, then raw agent output.
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_normalize_stage")
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_keywords_cli"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_agent_stage"));
    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return Vec::new();
    };

    let v = match serde_json::from_str::<Value>(raw) {
        Ok(v) => v,
        Err(_) => {
            // Fallback for agent markdown-table output when JSON normalization
            // did not produce a structured artifact.
            return extract_keywords_from_markdown_table(raw);
        }
    };

    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

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
    }

    out
}

#[derive(Debug, Clone, Copy)]
struct KeywordMetric {
    difficulty: Option<i64>,
    volume: Option<i64>,
}

fn extract_keyword_metrics(task: &Task) -> std::collections::HashMap<String, KeywordMetric> {
    use serde_json::Value;
    let mut out = std::collections::HashMap::new();

    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_normalize_stage")
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_keywords_cli"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_agent_stage"));

    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return out;
    };

    let parsed_json = serde_json::from_str::<Value>(raw).ok();
    if let Some(v) = parsed_json {
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
    fn title_case_capitalizes_each_word() {
        assert_eq!(to_title_case("seo tools guide"), "Seo Tools Guide");
        assert_eq!(to_title_case("content"), "Content");
    }
}
