use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::models::task::Task;

/// Run the CTR optimization analysis using an LLM agent.
///
/// Loads the "ctr-optimization" skill, builds a prompt with the skill content
/// and the provided context JSON, and delegates to the agent.
pub(crate) fn exec_ctr_analyze(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    // Quick check: if the context contains zero articles with issues, skip the agent call.
    let context_doc: serde_json::Value = match serde_json::from_str(context_json) {
        Ok(v) => v,
        Err(_) => {
            // If we can't parse the context, still try the agent — it might handle raw text.
            serde_json::Value::Null
        }
    };
    let total_articles = context_doc["total_articles"].as_i64().unwrap_or(-1);
    if total_articles == 0 {
        log::info!("[ctr_audit] No articles with CTR issues detected. Skipping agent analysis.");
        return StepResult {
            success: true,
            message: "All articles look healthy — no CTR issues detected.".to_string(),
            output: Some("{\"recommendations\":[],\"summary\":\"All clear – every article passes the current health checks.\"}".to_string()),
        };
    }

    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "ctr-optimization") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'ctr-optimization' not found in .github/skills/ or app defaults".to_string(),
                output: None,
            };
        }
    };

    // Use string concatenation to avoid format! panics if skill content contains { or }
    let prompt = skill.content
        + "\n\n---\n\n## CTR Audit Context\n\n"
        + context_json
        + "\n\nPlease analyze the above context and provide actionable CTR optimization recommendations."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the Output Contract above."
        + " Do not include markdown prose, summaries, tables, or explanations outside the JSON."
        + " Do not write files. Output the JSON directly in your response.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            // Extract JSON if present so downstream steps receive clean structured data
            let final_output = crate::engine::text::extract_json(&output)
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or(output);
            StepResult {
                success: true,
                message: "CTR analysis completed".to_string(),
                output: Some(final_output),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error during CTR analysis: {}", e),
            output: None,
        },
    }
}
