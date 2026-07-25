use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::ctr::{CtrAgentOutput, CtrFixType, CtrRecommendation};
use crate::models::task::Task;

/// Drop title_rewrite / meta_description fixes whose recommended string contains
/// a 20xx year not equal to the current calendar year (issue #112 rail A).
fn drop_year_invalid_title_meta_fixes(rec: &mut CtrRecommendation, current_year: i32) {
    let before = rec.fixes.len();
    rec.fixes.retain(|fix| {
        match fix.fix_type {
            CtrFixType::TitleRewrite | CtrFixType::MetaDescription => {
                match fix.recommended.as_str() {
                    Some(s) if !crate::content::year_policy::years_ok(s, current_year) => {
                        log::info!(
                            "[ctr_audit] Dropping {:?} fix for {} — recommended year not equal to {}",
                            fix.fix_type,
                            rec.url_slug,
                            current_year
                        );
                        false
                    }
                    _ => true,
                }
            }
            _ => true,
        }
    });
    if rec.fixes.len() < before {
        log::info!(
            "[ctr_audit] Filtered {} year-invalid title/meta fix(es) for {}",
            before - rec.fixes.len(),
            rec.url_slug
        );
    }
}

/// Run the CTR optimization analysis using an LLM agent.
///
/// Loads the "ctr-optimization" skill, builds a prompt with the skill content
/// and the provided context JSON, and delegates to the agent.
///
/// **Per-article mode:** When called from a `fix_ctr_article` task, `context_json`
/// is empty (`"{}"`) because there is no upstream step output. In that case the
/// function reads the single-article context from the task's `ctr_context` artifact.
/// Missing / empty `ctr_context` fails fast with a clear message (do not call the
/// agent with no article data). If the context contains exactly one article, the
/// returned output is the single `CtrRecommendation` JSON (not wrapped in
/// `CtrAgentOutput`), so downstream `fix_ctr_article_generate` sees the same format
/// it always has.
pub(crate) fn exec_ctr_analyze(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    // Resolve context: prefer explicit context_json, fall back to ctr_context artifact.
    let resolved_from_artifact = context_json.is_empty() || context_json == "{}";
    let context_json = if resolved_from_artifact {
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
            artifact_key: None,
        };
    }

    // Per-article path (fix_ctr_article) requires a real ctr_context artifact.
    // Empty `{}` / missing articles array means spawn forgot to attach context
    // (e.g. bare TaskSpawner create without the shared helper). Fail fast so
    // the agent is not asked for structured JSON with no article data.
    // Site-wide audit passes non-empty context_json from the prior build step;
    // that path is unaffected (resolved_from_artifact is false there).
    let articles_missing_or_empty = match context_doc.get("articles") {
        Some(arr) => arr.as_array().map(|a| a.is_empty()).unwrap_or(true),
        None => true,
    };
    if resolved_from_artifact
        && (context_json == "{}" || articles_missing_or_empty)
        && task.task_type == "fix_ctr_article"
    {
        return StepResult::fail(
            "fix_ctr_article requires a non-empty ctr_context artifact \
             (total_articles=1, articles=[...]). Recreate via \
             create-task -t fix_ctr_article -S <slug> or an audit-spawned child."
                .to_string(),
        );
    }

    let repo_root = Path::new(project_path);

    // The ctr-optimization skill file already contains the canonical Output Contract.
    // Passing a hardcoded contract here duplicates it and is the #1 source of schema drift.
    let output = match crate::engine::agent::run_agent_with_skill(
        "ctr-optimization",
        repo_root,
        &context_json,
        agent_provider,
        None,
    ) {
        Ok(o) => o,
        Err(e) => {
            return StepResult::fail(format!("Agent error: {}", e));
        }
    };

    let extracted = crate::engine::text::extract_json(&output);
    let final_output = match extracted {
        Some(ref val) => {
            let articles = context_doc["articles"].as_array();
            let is_single_article = articles.map(|a| a.len()).unwrap_or(0) == 1;
            let current_year = crate::content::year_policy::current_calendar_year();
            if is_single_article {
                // Prefer first recommendation from a list, else bare recommendation object.
                // Empty list → identity rec with empty fixes (still valid for downstream).
                let mut rec_opt: Option<CtrRecommendation> =
                    if let Some(recs) = val["recommendations"].as_array() {
                        if let Some(first) = recs.first() {
                            serde_json::from_value(first.clone()).ok()
                        } else {
                            let article = context_doc["articles"]
                                .as_array()
                                .and_then(|a| a.first())
                                .unwrap_or(&serde_json::Value::Null);
                            Some(CtrRecommendation {
                                article_id: article["id"].as_i64().unwrap_or(0),
                                url_slug: article["url_slug"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                                file: article["file"].as_str().unwrap_or("").to_string(),
                                target_keyword: article["target_keyword"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                                fixes: vec![],
                                priority: None,
                                expected_ctr_improvement: None,
                            })
                        }
                    } else {
                        serde_json::from_value(val.clone()).ok()
                    };

                if let Some(ref mut rec) = rec_opt {
                    drop_year_invalid_title_meta_fixes(rec, current_year);
                    serde_json::to_string_pretty(rec).unwrap_or_else(|_| output.clone())
                } else {
                    serde_json::to_string_pretty(val).unwrap_or_else(|_| output.clone())
                }
            } else if let Ok(mut agent_out) =
                serde_json::from_value::<CtrAgentOutput>(val.clone())
            {
                for rec in &mut agent_out.recommendations {
                    drop_year_invalid_title_meta_fixes(rec, current_year);
                }
                serde_json::to_string_pretty(&agent_out).unwrap_or_else(|_| output.clone())
            } else {
                serde_json::to_string_pretty(val).unwrap_or_else(|_| output.clone())
            }
        }
        None => {
            let preview = crate::engine::text::char_prefix(&output, 300);
            log::warn!(
                "[ctr_audit] Agent response contained no parseable JSON. Preview: {:?}",
                preview
            );
            return StepResult::fail_with_output(format!(
                    "CTR analysis agent did not return valid JSON. \
                     The prompt asked for a JSON object but the agent responded with non-JSON text. \
                     Preview: {:?}",
                    preview
                ), output);
        }
    };
    StepResult {
        success: true,
        message: "CTR analysis completed".to_string(),
        output: Some(final_output),
        artifact_key: None,
    }
}
