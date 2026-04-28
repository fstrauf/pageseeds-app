use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::models::task::Task;

/// Maximum articles per batch to stay under the ACP prompt-size boundary.
/// The full CTR context for ~19 articles is ~46KB. Batching by ~4 articles
/// targets ~10-12KB per prompt, which avoids the kimi acp hang.
const CTR_BATCH_SIZE: usize = 4;

/// Run the CTR optimization analysis using an LLM agent.
///
/// Loads the "ctr-optimization" skill, builds a prompt with the skill content
/// and the provided context JSON, and delegates to the agent.
///
/// If the context contains more than CTR_BATCH_SIZE articles, the analysis is
/// split into multiple batched prompts. The resulting recommendations are merged
/// into a single CtrAgentOutput so downstream steps (task spawner, verifier)
/// do not need to change.
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

    let articles = match context_doc["articles"].as_array() {
        Some(arr) if !arr.is_empty() => arr.clone(),
        _ => {
            // Fallback: send the whole context as a single prompt if articles array is missing.
            return run_single_analysis(agent_provider, &skill.content, context_json, repo_root);
        }
    };

    // If the article count is small enough, run a single prompt (no batching overhead).
    if articles.len() <= CTR_BATCH_SIZE {
        return run_single_analysis(agent_provider, &skill.content, context_json, repo_root);
    }

    // ── Batched analysis ──────────────────────────────────────────────────────
    log::info!(
        "[ctr_audit] Batching {} articles into chunks of {} (prompt-size workaround)",
        articles.len(),
        CTR_BATCH_SIZE
    );

    let mut all_recommendations: Vec<serde_json::Value> = Vec::new();
    let mut batch_errors: Vec<String> = Vec::new();

    for (batch_idx, chunk) in articles.chunks(CTR_BATCH_SIZE).enumerate() {
        let batch_context = serde_json::json!({
            "total_articles": chunk.len(),
            "articles": chunk,
        });
        let batch_json = match serde_json::to_string(&batch_context) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ctr_audit] Failed to serialize batch {}: {}", batch_idx + 1, e);
                batch_errors.push(format!("Batch {} serialize error: {}", batch_idx + 1, e));
                continue;
            }
        };

        let batch_size_bytes = batch_json.len();
        log::info!(
            "[ctr_audit] Running batch {}/{} ({} articles, {} bytes)",
            batch_idx + 1,
            (articles.len() + CTR_BATCH_SIZE - 1) / CTR_BATCH_SIZE,
            chunk.len(),
            batch_size_bytes
        );

        match run_single_analysis(agent_provider, &skill.content, &batch_json, repo_root) {
            StepResult { success: true, output: Some(json_str), .. } => {
                match serde_json::from_str::<serde_json::Value>(&json_str) {
                    Ok(parsed) => {
                        if let Some(recs) = parsed["recommendations"].as_array() {
                            all_recommendations.extend(recs.iter().cloned());
                            log::info!(
                                "[ctr_audit] Batch {} returned {} recommendations",
                                batch_idx + 1,
                                recs.len()
                            );
                        } else {
                            batch_errors.push(format!("Batch {}: missing recommendations array", batch_idx + 1));
                        }
                    }
                    Err(e) => {
                        batch_errors.push(format!("Batch {} parse error: {}", batch_idx + 1, e));
                    }
                }
            }
            StepResult { success: false, message, .. } => {
                log::warn!("[ctr_audit] Batch {} failed: {}", batch_idx + 1, message);
                batch_errors.push(format!("Batch {} failed: {}", batch_idx + 1, message));
            }
            StepResult { success: true, output: None, .. } => {
                batch_errors.push(format!("Batch {}: no output", batch_idx + 1));
            }
        }
    }

    let merged = serde_json::json!({
        "recommendations": all_recommendations,
        "summary": format!(
            "Batched CTR analysis: {} total recommendations from {} articles across {} batches. Errors: {}",
            all_recommendations.len(),
            articles.len(),
            (articles.len() + CTR_BATCH_SIZE - 1) / CTR_BATCH_SIZE,
            if batch_errors.is_empty() { "none".to_string() } else { batch_errors.join("; ") }
        ),
    });

    let success = !all_recommendations.is_empty() || batch_errors.is_empty();
    let message = if batch_errors.is_empty() {
        format!("CTR analysis completed: {} recommendations from {} batches", all_recommendations.len(), (articles.len() + CTR_BATCH_SIZE - 1) / CTR_BATCH_SIZE)
    } else {
        format!(
            "CTR analysis partial: {} recommendations, {} batch error(s): {}",
            all_recommendations.len(),
            batch_errors.len(),
            batch_errors.join("; ")
        )
    };

    log::info!("[ctr_audit] {}", message);

    StepResult {
        success,
        message,
        output: Some(serde_json::to_string_pretty(&merged).unwrap_or_default()),
    }
}

/// Run a single (non-batched) CTR analysis prompt.
fn run_single_analysis(
    agent_provider: &str,
    skill_content: &str,
    context_json: &str,
    repo_root: &Path,
) -> StepResult {
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## CTR Audit Context\n\n"
        + context_json
        + "\n\nPlease analyze the above context and provide actionable CTR optimization recommendations."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the Output Contract above."
        + " Do not include markdown prose, summaries, tables, or explanations outside the JSON."
        + " Do not write files. Output the JSON directly in your response.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
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
