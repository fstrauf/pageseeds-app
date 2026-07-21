//! Step 2 (agentic): generate the structured fix plan.

use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::{IndexingFixContext, IndexingFixPlan};

/// Agentic step: produce a structured `IndexingFixPlan` JSON.
///
/// Cannot be deterministic: the fix depends on intent, content quality, and
/// site-specific conventions. The agent returns JSON only — it does NOT edit
/// files (direct mode has no file I/O on most providers).
///
/// Input contract: `IndexingFixContext` JSON from step 1 (via latest_raw) plus
/// the optional `indexing_target_context` cluster artifact.
/// Output contract: `IndexingFixPlan` JSON (see the indexing-fix skill).
pub(crate) fn exec_indexing_fix_generate(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: Option<&str>,
) -> StepResult {
    let ctx: IndexingFixContext = match context_json {
        Some(j) => match serde_json::from_str(j) {
            Ok(c) => c,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!(
                        "indexing_fix_context output is not valid IndexingFixContext JSON: {}",
                        e
                    ),
                    output: None,
                }
            }
        },
        None => {
            return StepResult {
                success: false,
                message: "No context from indexing_fix_context step. Run the context step first."
                    .to_string(),
                output: None,
            }
        }
    };

    let context_block = format!(
        "\n\n## Page Context (Deterministic)\n\n```json\n{}\n```",
        serde_json::to_string_pretty(&ctx).unwrap_or_default()
    );

    let cluster_context_block = build_cluster_context_block(task);

    // Surface campaign-provided suggestions prominently so the agent uses them
    // instead of rewriting blind.
    let mut suggestions_block = String::new();
    if ctx.suggested_title.is_some() || ctx.suggested_h1.is_some() {
        suggestions_block.push_str("\n\n## Suggested Values (from site-wide audit)\n\n");
        if let Some(ref t) = ctx.suggested_title {
            suggestions_block.push_str(&format!("- Suggested title: {}\n", t));
        }
        if let Some(ref h) = ctx.suggested_h1 {
            suggestions_block.push_str(&format!("- Suggested H1: {}\n", h));
        }
        suggestions_block.push_str(
            "\nUse these suggested values as the basis for your `title` / `h1` changes. \
             Adjust only when they violate the skill rules.",
        );
    }

    let context = format!(
        "Task: Fix Indexing Issue\n\
         - Task ID: {}\n\
         - URL: {}\n\
         - Issue: {}\n\
         - Recommended Action: {}\n\
         - Reason: {}\n\
         - Repo: {}\n\
         {}\n\
         {}\n\
         {}",
        task.id,
        ctx.url,
        ctx.issue.as_deref().unwrap_or("unknown"),
        ctx.recommended_action
            .as_deref()
            .or(ctx.action.as_deref())
            .unwrap_or("unknown"),
        ctx.reason.as_deref().unwrap_or(""),
        project_path,
        suggestions_block,
        context_block,
        cluster_context_block,
    );

    let repo_root = Path::new(project_path);
    // The indexing-fix skill file contains the canonical Output Contract
    // (IndexingFixPlan JSON). The agent returns JSON only — no file edits.
    let raw = match crate::engine::agent::run_agent_with_skill(
        "indexing-fix",
        repo_root,
        &context,
        agent_provider,
        None,
    ) {
        Ok(output) => output,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Agent failed to generate fix plan: {}", e),
                output: None,
            }
        }
    };

    let plan: IndexingFixPlan = match crate::engine::text::extract_json_as(&raw) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!(
                    "Agent output did not contain a valid IndexingFixPlan JSON: {}",
                    crate::engine::text::char_prefix(&raw, 300)
                ),
                output: Some(raw),
            }
        }
    };

    if plan.changes.is_empty() {
        return StepResult {
            success: false,
            message: "Agent returned an IndexingFixPlan with no changes. \
                 Refusing to report success without any planned edit."
                .to_string(),
            output: serde_json::to_string_pretty(&plan).ok(),
        };
    }

    let plan_json = match serde_json::to_string_pretty(&plan) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize IndexingFixPlan: {}", e),
                output: None,
            }
        }
    };

    StepResult {
        success: true,
        message: format!("Generated IndexingFixPlan for {}", ctx.url),
        output: Some(plan_json),
    }
}

/// Load cluster context from task artifacts (set by indexing_health_campaign)
/// and format it as a prompt block.
fn build_cluster_context_block(task: &Task) -> String {
    task.artifacts
        .iter()
        .find(|a| a.key == "indexing_target_context")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<crate::models::indexing_health::IndexingTargetContext>(json).ok())
        .map(|ctx| {
            let siblings = match &ctx.cluster {
                Some(c) => serde_json::to_string_pretty(&c.siblings).unwrap_or_default(),
                None => "[]".to_string(),
            };
            format!(
                "\n\n## Cluster Context (from site-wide audit)\n\nThis page belongs to the '{}' cluster.\n\nSibling articles that may overlap topically:\n```json\n{}```\n\nShared headings detected in cluster: {:?}\n\nWhen planning changes, ensure the title, H1, and opening sections are DISTINCT from these siblings.",
                ctx.cluster.as_ref().map(|c| c.theme.clone()).unwrap_or_default(),
                siblings,
                ctx.cluster.as_ref().and_then(|c| c.shared_headings.clone()).unwrap_or_default()
            )
        })
        .unwrap_or_default()
}
