use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ─── Step 2: Plan ─────────────────────────────────────────────────────────────

/// Agentic step: choose the best source and anchor text from the shortlist.
///
/// V1 uses the existing prompt-based agent pattern (not Rig extraction)
/// to keep the implementation simple and proven.
pub(crate) fn exec_indexing_link_plan(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    use std::path::Path;
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Parse target artifact
    let target_data = match parse_target_artifact(task) {
        Some(t) => t,
        None => {
            return StepResult::fail("Missing or invalid indexing_link_target artifact".to_string())
        }
    };

    let target_slug = crate::content::slug::normalize_url_slug(target_data["slug"].as_str().unwrap_or(""));
    let target_url = target_data["url"].as_str().unwrap_or("").to_string();
    let target_keyword = target_data["target_keyword"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let reason_code = target_data["reason_code"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Load context from previous step (or rebuild from artifact)
    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_link_context")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: re-run context logic
            let ctx_result = exec_indexing_link_context(task, project_path);
            ctx_result.output.clone()
        });

    let context: serde_json::Value = match context_json {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => serde_json::json!({}),
    };

    let sources = context["sources"].as_array().cloned().unwrap_or_default();
    if sources.is_empty() {
        return StepResult {
            success: true,
            message: "Nothing to do — no source candidates available for this target".to_string(),
            output: Some(serde_json::json!({ "links_to_add": [] }).to_string()),
            artifact_key: None,
        };
    }

    // Build compact prompt
    let sources_json = serde_json::to_string(&sources).unwrap_or_default();
    let prompt = format!(
        r#"You are an SEO specialist choosing the best internal link to add.

## Target page
- URL: {target_url}
- Slug: {target_slug}
- Keyword: {target_keyword}
- Issue: {reason_code}

## Candidate source pages (already filtered for relevance)
{sources_json}

## Task
Choose exactly ONE source page from the candidate list above and decide:
1. Which source page should link to the target.
2. What anchor text to use (should naturally include or relate to the target keyword).

Return ONLY a valid JSON object — no markdown fences, no commentary.

Output schema:
{{
  "links_to_add": [
    {{
      "source_article_id": <number>,
      "target_article_id": <number>,
      "anchor_text": "<natural anchor text>",
      "target_slug": "{target_slug}",
      "placement": "related_section",
      "reason": "<one sentence explaining why this source and anchor were chosen>"
    }}
  ]
}}

Requirements:
- Only ONE link in links_to_add.
- Choose from the candidate sources above.
- Do NOT pick a source where already_links_to_target is true.
- placement must be "related_section" in V1.
"#,
    );

    let raw_output = match crate::engine::agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(out) => out,
        Err(e) => {
            return StepResult::fail(format!("Agent failed: {}", e))
        }
    };

    let plan_json = crate::engine::text::extract_json(&raw_output).unwrap_or_else(|| {
        serde_json::json!({
            "links_to_add": [],
        })
    });

    // Validate: ensure we got exactly one link
    let link_count = plan_json["links_to_add"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    if link_count == 0 {
        return StepResult::fail("Agent returned no link recommendations".to_string());
    }

    // Persist plan for apply step
    let plan_path = paths
        .automation_dir
        .join(format!("indexing_link_plan_{}.json", task.id));
    let _ = std::fs::write(
        &plan_path,
        serde_json::to_string_pretty(&plan_json).unwrap_or_default(),
    );

    StepResult {
        success: true,
        message: format!(
            "Link plan: {} link recommended for {}",
            link_count, target_slug
        ),
        output: Some(plan_json.to_string()),
        artifact_key: None,
    }
}

