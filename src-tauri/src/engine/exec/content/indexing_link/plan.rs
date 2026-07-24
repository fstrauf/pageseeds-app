use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::indexing_health::IndexingLinkOutcome;
use crate::models::task::Task;

use super::*;
// ─── Step 2: Plan ─────────────────────────────────────────────────────────────

/// Agentic step: choose the best source and anchor text from the shortlist.
///
/// V1 uses the existing prompt-based agent pattern (not Rig extraction)
/// to keep the implementation simple and proven.
///
/// Intentional no-ops write `outcome` so verify can pass without inbound growth:
/// - empty shortlist → `no_candidates`
/// - all candidates already link to target → `already_linked`
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
            return StepResult::fail(MISSING_INDEXING_LINK_TARGET_MSG.to_string())
        }
    };

    let target_slug = crate::content::slug::normalize_url_slug(&target_data.slug);
    let target_url = target_data.url.clone();
    let target_keyword = target_data.target_keyword.clone();
    let reason_code = target_data.reason_code.clone();
    let target_article_id = target_data.article_id;

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
        return noop_plan_result(
            &paths,
            task,
            IndexingLinkOutcome::NoCandidates,
            "Nothing to do — no source candidates available for this target",
        );
    }

    // Drop sources that already link to the target before calling the agent.
    let usable: Vec<serde_json::Value> = sources
        .iter()
        .filter(|s| s["already_links_to_target"].as_bool() != Some(true))
        .cloned()
        .collect();
    if usable.is_empty() {
        return noop_plan_result(
            &paths,
            task,
            IndexingLinkOutcome::AlreadyLinked,
            "Nothing to do — all candidate sources already link to the target",
        );
    }

    // Build compact prompt
    let sources_json = serde_json::to_string(&usable).unwrap_or_default();
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
      "source_file": "<file path from the candidate>",
      "target_article_id": {target_article_id},
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
- Include source_file from the chosen candidate.
"#,
    );

    let raw_output = match crate::engine::agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(out) => out,
        Err(e) => {
            return StepResult::fail(format!("Agent failed: {}", e))
        }
    };

    let mut plan_json = crate::engine::text::extract_json(&raw_output).unwrap_or_else(|| {
        serde_json::json!({
            "links_to_add": [],
        })
    });

    // Enrich plan rows with source_file from candidates when the agent omits it.
    if let Some(links) = plan_json["links_to_add"].as_array_mut() {
        for link in links.iter_mut() {
            let source_id = link["source_article_id"].as_i64().unwrap_or(0);
            let has_file = link["source_file"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !has_file {
                if let Some(src) = usable
                    .iter()
                    .chain(sources.iter())
                    .find(|s| s["article_id"].as_i64() == Some(source_id))
                {
                    if let Some(f) = src["file"].as_str() {
                        link["source_file"] = serde_json::json!(f);
                    }
                }
            }
            // V1 hard-codes related_section; ignore any other placement.
            link["placement"] = serde_json::json!("related_section");
            if link["target_article_id"].as_i64().unwrap_or(0) == 0 {
                link["target_article_id"] = serde_json::json!(target_article_id);
            }
        }
    }

    // Validate: ensure we got exactly one link
    let link_count = plan_json["links_to_add"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    if link_count == 0 {
        return StepResult::fail("Agent returned no link recommendations".to_string());
    }

    // Planned links pending apply — outcome finalized by apply.
    plan_json["outcome"] = serde_json::json!(IndexingLinkOutcome::Applied.as_str());

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

fn noop_plan_result(
    paths: &ProjectPaths,
    task: &Task,
    outcome: IndexingLinkOutcome,
    message: &str,
) -> StepResult {
    let plan_json = serde_json::json!({
        "links_to_add": [],
        "outcome": outcome.as_str(),
    });
    let plan_path = paths
        .automation_dir
        .join(format!("indexing_link_plan_{}.json", task.id));
    let _ = std::fs::write(
        &plan_path,
        serde_json::to_string_pretty(&plan_json).unwrap_or_default(),
    );
    StepResult {
        success: true,
        message: message.to_string(),
        output: Some(plan_json.to_string()),
        artifact_key: None,
    }
}
