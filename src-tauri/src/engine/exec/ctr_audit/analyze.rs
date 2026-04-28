use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::models::task::Task;

/// Run the CTR optimization analysis using an LLM agent.
///
/// Loads the "ctr-optimization" skill, builds a prompt with the skill content
/// and the provided context JSON, and delegates to the agent.
///
/// **Per-article mode:** When called from a `fix_ctr_article` task, `context_json`
/// is empty (`"{}"`) because there is no upstream step output. In that case the
/// function reads the single-article context from the task's `ctr_context` artifact.
/// If the context contains exactly one article, the returned output is the single
/// `CtrRecommendation` JSON (not wrapped in `CtrAgentOutput`), so downstream
/// `fix_ctr_article_generate` sees the same format it always has.
pub(crate) fn exec_ctr_analyze(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    // Resolve context: prefer explicit context_json, fall back to ctr_context artifact.
    let context_json = if context_json.is_empty() || context_json == "{}" {
        task.artifacts
            .iter()
            .find(|a| a.key == "ctr_context")
            .and_then(|a| a.content.clone())
            .unwrap_or_else(|| "{}".to_string())
    } else {
        context_json.to_string()
    };

    // Quick check: if the context contains zero articles with issues, skip the agent call.
    let context_doc: serde_json::Value = match serde_json::from_str(&context_json) {
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

    let prompt = skill.content
        + "\n\n---\n\n## CTR Audit Context\n\n"
        + &context_json
        + "\n\nPlease analyze the above context and provide actionable CTR optimization recommendations."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the Output Contract above."
        + " Do not include markdown prose, summaries, tables, or explanations outside the JSON."
        + " Do not write files. Output the JSON directly in your response.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            let extracted = crate::engine::text::extract_json(&output);
            let final_output = match extracted {
                Some(ref val) => {
                    // Per-article mode: if the context has exactly one article,
                    // extract the single recommendation so downstream fix steps
                    // receive a plain CtrRecommendation instead of CtrAgentOutput.
                    let articles = context_doc["articles"].as_array();
                    let is_single_article = articles.map(|a| a.len()).unwrap_or(0) == 1;
                    if is_single_article {
                        if let Some(recs) = val["recommendations"].as_array() {
                            if let Some(first) = recs.first() {
                                serde_json::to_string_pretty(first).unwrap_or_else(|_| output.clone())
                            } else {
                                serde_json::to_string_pretty(val).unwrap_or_else(|_| output.clone())
                            }
                        } else {
                            serde_json::to_string_pretty(val).unwrap_or_else(|_| output.clone())
                        }
                    } else {
                        serde_json::to_string_pretty(val).unwrap_or_else(|_| output.clone())
                    }
                }
                None => output,
            };
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
