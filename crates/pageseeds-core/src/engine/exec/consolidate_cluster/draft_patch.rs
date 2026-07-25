use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Draft Patch (agentic)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic: draft a ContentMergePatch JSON that merges unique valuable content
/// from redirect pages into the keeper page.
///
/// Why not deterministic: merging overlapping articles requires editorial judgment
/// about which sections are redundant, which contain unique value worth preserving,
/// and where in the keeper's structure they best fit. A deterministic algorithm
/// cannot evaluate content quality, relevance, or narrative flow. The output is a
/// structured `ContentMergePatch` with precise insertion points, extracted via
/// Rig's `extract_structured`.
pub(crate) fn exec_merge_draft_patch(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "merge-content") {
        Ok(s) => s,
        Err(msg) => {
            return StepResult::fail(msg);
        }
    };

    // When the extract step produced multiple batches (clusters with >5
    // redirect pages), run one draft round per batch against the same keeper
    // instead of dropping pages. The rounds are applied sequentially by
    // `merge_apply_patch`.
    let context: Option<MergeContext> = serde_json::from_str(context_json).ok();
    let batch_contexts: Vec<String> = match &context {
        Some(context) if !context.batches.is_empty() => context
            .batches
            .iter()
            .map(|b| {
                serde_json::to_string(&MergeRoundContext {
                    keeper_file: &context.keeper_file,
                    keeper_outline: &context.keeper_outline,
                    keeper_excerpt: &context.keeper_excerpt,
                    total_redirects: context.total_redirects,
                    batch_count: context.batch_count,
                    batch_index: b.batch_index,
                    redirect_pages: &b.redirect_pages,
                })
                .unwrap_or_default()
            })
            .collect(),
        // Backward-compatible fallback: no batch structure → single round with
        // the raw context as-is.
        _ => vec![context_json.to_string()],
    };

    // Run the structured extractor inside a fresh runtime because this function
    // is called from within tokio::task::spawn_blocking.
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return StepResult::fail(format!("Failed to create runtime for merge extraction: {}", e));
        }
    };

    let mut patches: Vec<crate::models::merge_patch::ContentMergePatch> = Vec::new();
    for (i, batch_context) in batch_contexts.iter().enumerate() {
        let prompt = assemble_merge_prompt(&skill.content, batch_context);

        let budget = crate::config::prompt_budget::default_prompt_budget();
        let prompt_bytes = prompt.len();
        if prompt_bytes > budget.hard {
            return StepResult::fail(format!(
                    "Merge prompt too large ({} bytes) for batch {}/{}. Hard budget: {} bytes. \
                     Batching and per-page truncation in merge_extract_sections should have \
                     kept every round under this budget — treat this as a bug there, not a \
                     provider limit.",
                    prompt_bytes, i + 1, batch_contexts.len(), budget.hard
                ));
        }

        let extract_result = rt.block_on(async {
            crate::rig::extraction::extract_structured::<crate::models::merge_patch::ContentMergePatch>(
                agent_provider,
                &prompt,
                Some("You are an expert content editor. Draft a precise ContentMergePatch JSON."),
                // Backend preference is only honored by the legacy Kimi bridge;
                // the default CLI backend ignores it.
                None,
                None,
            )
            .await
        });

        match extract_result {
            Ok(patch) => patches.push(patch),
            Err(e) => {
                return StepResult::fail(format!(
                        "Structured extraction failed for merge patch (batch {}/{}): {}",
                        i + 1,
                        batch_contexts.len(),
                        e
                    ));
            }
        }
    }

    let total_additions: usize = patches.iter().map(|p| p.additions.len()).sum();
    let total_transitions: usize = patches.iter().map(|p| p.transitions.len()).sum();

    // Single round keeps the original output shape (a bare ContentMergePatch);
    // multiple rounds are wrapped so `merge_apply_patch` can apply them in order.
    let output_json = if patches.len() == 1 {
        serde_json::to_string_pretty(&patches[0])
    } else {
        serde_json::to_string_pretty(&serde_json::json!({ "patches": patches }))
    };

    match output_json {
        Ok(j) => StepResult {
            success: true,
            message: format!(
                "Merge patch drafted: {} round(s), {} additions, {} transitions",
                patches.len(),
                total_additions,
                total_transitions
            ),
            output: Some(j),
            artifact_key: None,
        },
        Err(e) => StepResult::fail(format!("Failed to serialize merge patch: {}", e)),
    }
}

/// Assemble the full merge prompt for one draft round: skill content +
/// merge-context header + serialized round context + instruction tail. Kept
/// as one function so tests can assert the assembled size against the shared
/// prompt budget exactly as the step produces it.
pub(crate) fn assemble_merge_prompt(skill_content: &str, batch_context_json: &str) -> String {
    skill_content.to_string()
        + "\n\n---\n\n## Merge Context\n\n"
        + batch_context_json
        + "\n\nPlease draft a ContentMergePatch JSON that merges the most valuable unique content from the redirect pages into the keeper."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the ContentMergePatch structure."
        + " Do not include markdown prose, summaries, or explanations outside the JSON."
}

