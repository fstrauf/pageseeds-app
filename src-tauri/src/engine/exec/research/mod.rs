/// Research workflow execution module.
///
/// Contains the execution logic for the 3-step research workflow:
/// 1. research_seed_extraction - LLM extracts themes from project brief (agentic)
/// 2. research_ahrefs_pipeline - Deterministic Rust calls Ahrefs API directly
/// 3. research_final_selection - Deterministic filtering/sorting of results
///
/// Only step 1 uses an LLM. Steps 2 and 3 are pure Rust for reliability.

mod autocomplete;
mod landing_page;
mod prompts;

pub(crate) use autocomplete::*;
pub(crate) use landing_page::*;
pub(crate) use prompts::*;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{StepResult, WorkflowStep};
use crate::models::task::Task;

/// Execute a research workflow step using the configured CLI agent.
///
/// This handles the research steps that need an LLM (currently only
/// `research_seed_extraction`). It builds the prompt and delegates to
/// `agent::run_agent` — the same path used by every other agentic step.
///
/// The `previous_output` parameter contains the output from the previous step,
/// used to pass data between steps (e.g., themes from step 1 to step 2).
pub async fn exec_research_workflow_step(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    previous_output: Option<&str>,
) -> StepResult {
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);

    // Build prompts based on step name, passing previous step's output
    let (system_prompt, user_prompt) = match build_research_prompts(
        &step.name,
        task,
        project_path,
        &paths,
        previous_output,
    ) {
        Ok(prompts) => prompts,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to build prompts for '{}': {}", step.name, e),
                output: None,
            };
        }
    };

    // Combine system and user prompts for the CLI agent
    let prompt = format!("{}\n\n---\n\n{}", system_prompt, user_prompt);

    log::info!(
        "[research_workflow] Executing '{}' with provider '{}'",
        step.name,
        agent_provider
    );

    let provider = agent_provider.to_string();
    let repo_root = Path::new(project_path).to_path_buf();
    let step_name = step.name.clone();

    // Run the agent via the standard CLI wrapper (same as all other agentic steps)
    match tokio::task::spawn_blocking(move || {
        crate::engine::agent::run_agent(&provider, &prompt, &repo_root)
    }).await {
        Ok(Ok(output)) => {
            log::info!(
                "[research_workflow] '{}' complete ({} chars)",
                step_name,
                output.len()
            );

            StepResult {
                success: true,
                message: format!(
                    "Research step '{}' complete ({} chars)",
                    step_name,
                    output.len()
                ),
                output: Some(output),
            }
        }
        Ok(Err(e)) => {
            log::error!("[research_workflow] '{}' failed: {}", step_name, e);

            StepResult {
                success: false,
                message: format!("Research step '{}' failed: {}", step_name, e),
                output: None,
            }
        }
        Err(e) => {
            log::error!("[research_workflow] '{}' task failed: {}", step_name, e);

            StepResult {
                success: false,
                message: format!("Research step '{}' task failed: {}", step_name, e),
                output: None,
            }
        }
    }
}
