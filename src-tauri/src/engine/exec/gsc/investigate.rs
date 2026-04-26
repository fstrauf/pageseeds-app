use std::path::Path;

use crate::engine::agent;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{StepResult, WorkflowStep};
use crate::models::task::Task;

// ─── GSC summary (deterministic pre-step for investigate_gsc) ────────────────

/// Deterministic pre-step for `investigate_gsc`.
///
/// Reads gsc_collection.json and produces a compact structured summary grouped
/// by reason_code, with counts, percentages, and up to 5 example URLs per group.
/// Writes gsc_summary.json to the automation dir.
///
/// The agentic investigation step reads this summary rather than raw collection data,
/// so the agent interprets patterns and recommends actions instead of re-doing trivial
/// counting and grouping that a `group_by().count()` handles exactly.
pub(crate) fn exec_gsc_summarise(
    task: &Task,
    project_path: &str,
) -> StepResult {
    use serde_json::{json, Value};
    use std::collections::HashMap;
    let _ = task;

    let paths = ProjectPaths::from_path(project_path);
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let raw = match std::fs::read_to_string(&collection_path) {
        Ok(s) => s,
        Err(_) => return StepResult {
            success: false,
            message: "gsc_collection.json not found — run collect_gsc first".to_string(),
            output: None,
        },
    };

    let collection: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return StepResult {
            success: false,
            message: format!("Failed to parse gsc_collection.json: {}", e),
            output: None,
        },
    };

    let items = match collection.get("items").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => return StepResult {
            success: false,
            message: "gsc_collection.json has no 'items' array".to_string(),
            output: None,
        },
    };

    let total = items.len();
    let mut by_reason: HashMap<String, Vec<String>> = HashMap::new();
    let mut indexed_count = 0usize;

    for item in &items {
        let reason = item.get("reason_code")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let url = item.get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if reason == "indexed_pass" { indexed_count += 1; }
        by_reason.entry(reason).or_default().push(url);
    }

    let non_indexed_count = total - indexed_count;

    let mut groups: Vec<Value> = by_reason.iter().map(|(reason, urls)| {
        let count = urls.len();
        let pct = if total > 0 { (count * 100) / total } else { 0 };
        let examples: Vec<&String> = urls.iter().take(5).collect();
        json!({
            "reason_code": reason,
            "count": count,
            "percentage": pct,
            "example_urls": examples,
        })
    }).collect();

    // Sort by count descending so the most common issues appear first.
    groups.sort_by(|a, b| {
        let ca = a.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        let cb = b.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        cb.cmp(&ca)
    });

    let summary = json!({
        "total_inspected": total,
        "indexed_count": indexed_count,
        "non_indexed_count": non_indexed_count,
        "by_reason": groups,
    });

    let summary_path = paths.automation_dir.join("gsc_summary.json");
    let summary_str = serde_json::to_string_pretty(&summary).unwrap_or_default();
    if let Err(e) = std::fs::write(&summary_path, &summary_str) {
        return StepResult {
            success: false,
            message: format!("Failed to write gsc_summary.json: {}", e),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!(
            "GSC summary: {} total, {} indexed, {} non-indexed ({} reason groups)",
            total, indexed_count, non_indexed_count, by_reason.len()
        ),
        output: Some(summary_str),
    }
}

// ─── GSC investigation ────────────────────────────────────────────────────────

/// Agentic investigation step for `investigate_gsc`.
///
/// Reads gsc_summary.json (produced by the deterministic `gsc_summarise` pre-step)
/// and passes the structured summary to the LLM. The agent interprets *why* certain
/// reason groups are occurring, identifies cross-cutting patterns, and recommends
/// corrective actions — judgment that `group_by().count()` cannot provide.
///
/// Falls back to gsc_collection.json if the summary is not yet written.
pub(crate) fn exec_gsc_investigate(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let _ = step;

    let paths = ProjectPaths::from_path(project_path);

    // Prefer the pre-processed summary; fall back to raw collection if missing.
    let summary_path = paths.automation_dir.join("gsc_summary.json");
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let (context_json, context_label) = if let Ok(s) = std::fs::read_to_string(&summary_path) {
        (s, "GSC Summary (pre-processed)")
    } else if let Ok(s) = std::fs::read_to_string(&collection_path) {
        (s, "GSC Collection (raw)")
    } else {
        return StepResult {
            success: false,
            message: "Neither gsc_summary.json nor gsc_collection.json found — run collect_gsc first".to_string(),
            output: None,
        };
    };

    let prompt = format!(
        "## Task: Investigate GSC Indexing Results\n\n\
         - Task ID: {}\n\
         - Site: {}\n\
         - Repo: {}\n\n\
         ## {}\n\n\
         ```json\n{}\n```\n\n\
         ## Instructions\n\n\
         The data above groups pages by indexing reason code with counts and example URLs.\n\
         Your job is to interpret the patterns — not count or regroup them.\n\n\
         For each non-indexed reason group:\n\
         1. Explain the likely root cause in one sentence\n\
         2. Recommend a specific corrective action\n\
         3. Assign a priority (high/medium/low) based on count and impact\n\n\
         Return a JSON object:\n\
         ```json\n\
         {{\n  \"summary\": \"...\",\n  \"issues_found\": [\n    {{\n      \
         \"reason_code\": \"...\",\n      \"url_count\": 0,\n      \"root_cause\": \"...\",\n      \
         \"recommendation\": \"...\",\n      \"priority\": \"high|medium|low\"\n    \
         }}\n  ]\n}}\n\
         ```",
        task.id,
        project_path,
        project_path,
        context_label,
        context_json,
    );

    match agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => StepResult {
            success: true,
            message: format!("GSC investigation complete ({} chars)", output.len()),
            output: Some(output),
        },
        Err(e) => StepResult {
            success: false,
            message: format!("GSC investigation agent failed: {}", e),
            output: None,
        },
    }
}
