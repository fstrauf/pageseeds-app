use std::path::Path;

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
pub(crate) fn exec_gsc_summarise(task: &Task, project_path: &str) -> StepResult {
    use serde_json::{json, Value};
    use std::collections::HashMap;
    let _ = task;

    let paths = ProjectPaths::from_path(project_path);
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let collection: Value =
        match crate::engine::exec::common::read_json(&collection_path, "gsc_collection.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    let items = match collection.get("items").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => {
            return StepResult::fail("gsc_collection.json has no 'items' array".to_string())
        }
    };

    let total = items.len();
    let mut by_reason: HashMap<String, Vec<String>> = HashMap::new();
    let mut indexed_count = 0usize;

    for item in &items {
        let reason = item
            .get("reason_code")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if reason == "indexed_pass" {
            indexed_count += 1;
        }
        by_reason.entry(reason).or_default().push(url);
    }

    let non_indexed_count = total - indexed_count;

    let mut groups: Vec<Value> = by_reason
        .iter()
        .map(|(reason, urls)| {
            let count = urls.len();
            let pct = if total > 0 { (count * 100) / total } else { 0 };
            let examples: Vec<&String> = urls.iter().take(5).collect();
            json!({
                "reason_code": reason,
                "count": count,
                "percentage": pct,
                "example_urls": examples,
            })
        })
        .collect();

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
    if let Err(e) =
        crate::engine::exec::common::write_json(&summary_path, &summary, "gsc_summary.json")
    {
        return e;
    }

    StepResult {
        success: true,
        message: format!(
            "GSC summary: {} total, {} indexed, {} non-indexed ({} reason groups)",
            total,
            indexed_count,
            non_indexed_count,
            by_reason.len()
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
        return StepResult::fail("Neither gsc_summary.json nor gsc_collection.json found — run collect_gsc first"
                    .to_string());
    };

    let context = format!(
        "Task ID: {}\nSite: {}\nRepo: {}\n\n## {}\n\n```json\n{}\n```",
        task.id, project_path, project_path, context_label, context_json,
    );

    let repo_root = Path::new(project_path);
    // The gsc-investigate skill file already contains the canonical Output Contract.
    match crate::engine::agent::run_agent_with_skill(
        "gsc-investigate",
        repo_root,
        &context,
        agent_provider,
        None,
    ) {
        Ok(output) => StepResult {
            success: true,
            message: format!("GSC investigation complete ({} chars)", output.len()),
            output: Some(output),
        },
        Err(e) => StepResult::fail(format!("GSC investigation agent failed: {}", e)),
    }
}
