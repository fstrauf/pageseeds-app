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

    let prompt = skill.content
        + "\n\n---\n\n## Merge Context\n\n"
        + context_json
        + "\n\nPlease draft a ContentMergePatch JSON that merges the most valuable unique content from the redirect pages into the keeper."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the ContentMergePatch structure."
        + " Do not include markdown prose, summaries, or explanations outside the JSON.";

    const HARD_PROMPT_LIMIT_BYTES: usize = 20_000;
    let prompt_bytes = prompt.len();
    if prompt_bytes > HARD_PROMPT_LIMIT_BYTES {
        return StepResult {
            success: false,
            message: format!(
                "Merge prompt too large ({} bytes). Limit: {} bytes. \
                 The cluster has too much redirect content to fit the Kimi bridge limit. \
                 Consider splitting the cluster into smaller groups or running merge manually.",
                prompt_bytes, HARD_PROMPT_LIMIT_BYTES
            ),
            output: None,
        };
    }

    // Run the structured extractor inside a fresh runtime because this function
    // is called from within tokio::task::spawn_blocking.
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to create runtime for merge extraction: {}", e),
                output: None,
            };
        }
    };

    let extract_result = rt.block_on(async {
        crate::rig::extraction::extract_structured::<crate::models::merge_patch::ContentMergePatch>(
            agent_provider,
            &prompt,
            Some("You are an expert content editor. Draft a precise ContentMergePatch JSON."),
            Some("direct"),
            None,
        )
        .await
    });

    match extract_result {
        Ok(patch) => {
            let patch_json = match serde_json::to_string_pretty(&patch) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to serialize merge patch: {}", e),
                        output: None,
                    };
                }
            };
            StepResult {
                success: true,
                message: format!(
                    "Merge patch drafted: {} additions, {} transitions",
                    patch.additions.len(),
                    patch.transitions.len()
                ),
                output: Some(patch_json),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Structured extraction failed for merge patch: {}", e),
            output: None,
        },
    }
}

