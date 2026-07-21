/// Research workflow execution module.
///
/// Contains the execution logic for the research workflow:
/// 1. research_seed_extraction - LLM extracts themes from project brief (agentic, structured)
/// 2. research_seed_validation - LLM validates themes and proposes seed phrasings (agentic, structured)
/// 3. research_ahrefs_pipeline - Deterministic Rust calls the SEO data provider
/// 4. research_final_selection - Deterministic filtering/sorting of results
///
/// Steps 1 and 2 use rig's `Extractor<T>` for type-safe structured output,
/// eliminating the need for a separate normalizer step.
mod final_selection;
mod prompts;
mod tests;

pub(crate) use final_selection::*;
pub(crate) use prompts::*;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{StepResult, WorkflowStep};
use crate::models::task::Task;

/// Execute a research workflow step.
///
/// For `research_seed_extraction` and `research_seed_validation`, this uses
/// rig's `Extractor<T>` to guarantee structured JSON output directly from the
/// LLM, removing the need for a post-hoc normalizer step.
///
/// For legacy or unexpected step names, it falls back to the standard
/// `agent::run_agent` path.
///
/// The `previous_output` parameter contains the output from the previous step,
/// used to pass data between steps.
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
    let (system_prompt, user_prompt) =
        match build_research_prompts(&step.name, task, project_path, &paths, previous_output) {
            Ok(prompts) => prompts,
            Err(e) => {
                return StepResult::fail(format!("Failed to build prompts for '{}': {}", step.name, e));
            }
        };

    let prompt = format!("{}\n\n---\n\n{}", system_prompt, user_prompt);
    let provider = agent_provider.to_string();
    let step_name = step.name.clone();

    log::info!(
        "[research_workflow] Executing '{}' with provider '{}'",
        step_name,
        provider
    );

    match step_name.as_str() {
        "research_seed_extraction" => {
            run_structured_extraction::<crate::models::research::SeedExtractionOutput>(
                &provider,
                &prompt,
                &system_prompt,
                &step_name,
            )
            .await
        }
        "research_seed_validation" => {
            run_structured_extraction::<crate::models::research::SeedValidationOutput>(
                &provider,
                &prompt,
                &system_prompt,
                &step_name,
            )
            .await
        }
        _ => {
            // Fallback: use standard agent for other research steps
            let repo_root = Path::new(project_path).to_path_buf();
            match tokio::task::spawn_blocking(move || {
                crate::engine::agent::run_agent(&provider, &prompt, &repo_root)
            })
            .await
            {
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

                    StepResult::fail(format!("Research step '{}' failed: {}", step_name, e))
                }
                Err(e) => {
                    log::error!("[research_workflow] '{}' task failed: {}", step_name, e);

                    StepResult::fail(format!("Research step '{}' task failed: {}", step_name, e))
                }
            }
        }
    }
}

/// Helper: run a structured extraction and serialize the result to JSON.
async fn run_structured_extraction<T>(
    provider: &str,
    prompt: &str,
    preamble: &str,
    step_name: &str,
) -> StepResult
where
    T: schemars::JsonSchema
        + for<'a> serde::Deserialize<'a>
        + serde::Serialize
        + Send
        + Sync
        + 'static,
{
    match crate::rig::extraction::extract_structured::<T>(provider, prompt, Some(preamble), Some("direct"), None).await {
        Ok(output) => {
            let json = match serde_json::to_string_pretty(&output) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult::fail(format!(
                            "Structured extraction for '{}' succeeded but serialization failed: {}",
                            step_name, e
                        ));
                }
            };
            log::info!(
                "[research_workflow] '{}' structured extraction complete ({} chars)",
                step_name,
                json.len()
            );
            StepResult {
                success: true,
                message: format!("Structured extraction for '{}' complete", step_name),
                output: Some(json),
            }
        }
        Err(e) => {
            log::error!(
                "[research_workflow] '{}' structured extraction failed: {}",
                step_name,
                e
            );
            StepResult::fail(format!("Structured extraction for '{}' failed: {}", step_name, e))
        }
    }
}
