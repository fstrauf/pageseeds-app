use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::indexing_health::{
    DistinctivenessVerdict, IndexingCampaignPlan, IndexingCampaignSummary, IndexingTargetContext,
    IndexingTargetPlan, PrerequisiteCheck, PrerequisiteReport, TargetDiagnosis,
};
use crate::models::task::Task;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Distinctiveness Review (agentic)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic distinctiveness review.
/// For each target with cluster siblings, ask the agent to judge whether the
/// target's title, H1, and focus are sufficiently distinct from its siblings.
pub(crate) fn exec_ihc_distinctiveness_review(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    _context_json: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load target contexts
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &contexts_path,
        "indexing_target_contexts.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let targets: Vec<IndexingTargetContext> = match contexts_doc["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect::<Vec<IndexingTargetContext>>()
        .into_iter()
        .filter(|t| t.diagnosis.has_cluster_siblings)
        .collect::<Vec<_>>()
    {
        t if t.is_empty() => {
            return StepResult {
                success: true,
                message: "No targets with cluster siblings — distinctiveness review skipped."
                    .to_string(),
                output: Some("[]".to_string()),
            }
        }
        t => t,
    };

    // Load skill
    let repo_root = Path::new(project_path);
    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "indexing-distinctiveness") {
        Ok(s) => s.content,
        Err(msg) => {
            return StepResult { success: false, message: msg, output: None }
        }
    };

    let mut verdicts: Vec<DistinctivenessVerdict> = Vec::new();

    // Process one target at a time to stay within prompt budget
    for target_ctx in &targets {
        let prompt = build_distinctiveness_prompt(&skill, target_ctx);

        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Failed to create runtime for extraction: {}", e),
                    output: None,
                }
            }
        };

        let extract_result = rt.block_on(async {
            crate::rig::extraction::extract_structured::<DistinctivenessVerdict>(
                agent_provider,
                &prompt,
                Some("You are an expert SEO content strategist. Judge article distinctiveness precisely."),
                Some("direct"),
                None,
            )
            .await
        });

        match extract_result {
            Ok(v) => {
                log::info!(
                    "[ihc_distinctiveness] {} → {} ({})",
                    target_ctx.target.url,
                    v.verdict,
                    v.confidence
                );
                verdicts.push(v);
            }
            Err(e) => {
                log::warn!(
                    "[ihc_distinctiveness] failed for {}: {}",
                    target_ctx.target.url,
                    e
                );
                // Push a fallback verdict so the reduce step can still proceed
                verdicts.push(DistinctivenessVerdict {
                    target_url: target_ctx.target.url.clone(),
                    verdict: "DISTINCT".to_string(),
                    confidence: "low".to_string(),
                    recommendation: "NO_ACTION".to_string(),
                    keep_url: None,
                    redirect_url: None,
                    reason: format!("Extraction failed: {}. Defaulting to no action.", e),
                    suggested_title: None,
                    suggested_h1: None,
                });
            }
        }
    }

    // Write verdicts to disk
    let verdicts_path = paths
        .automation_dir
        .join("indexing_distinctiveness_verdicts.json");
    let verdicts_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "verdicts": verdicts,
    });
    let verdicts_json = match serde_json::to_string_pretty(&verdicts_doc) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize verdicts: {}", e),
                output: None,
            }
        }
    };
    let _ = std::fs::write(&verdicts_path, &verdicts_json);

    StepResult {
        success: true,
        message: format!(
            "Distinctiveness review: {} verdict(s), {} OVERLAP",
            verdicts.len(),
            verdicts.iter().filter(|v| v.verdict == "OVERLAP").count()
        ),
        output: Some(verdicts_json),
    }
}

fn build_distinctiveness_prompt(skill: &str, target: &IndexingTargetContext) -> String {
    let siblings_json = match &target.cluster {
        Some(c) => serde_json::to_string_pretty(&c.siblings).unwrap_or_default(),
        None => "[]".to_string(),
    };

    format!(
        "{skill}\n\n---\n\n## Target Article\n\n- URL: {url}\n- Title: {title}\n- H1: {h1}\n- Word count: {wc}\n- Reason not indexed: {reason}\n\n## Cluster Siblings\n\n{siblings}\n\nReturn a single JSON object matching the DistinctivenessVerdict structure.",
        skill = skill,
        url = target.target.url,
        title = target.target.title,
        h1 = target.target.h1,
        wc = target.target.word_count,
        reason = target.target.reason_code,
        siblings = siblings_json,
    )
}
