/// Research workflow execution module.
///
/// Contains the execution logic for the research workflow:
/// 1. research_seed_extraction - LLM extracts themes from project brief (agentic, structured)
/// 2. research_autocomplete - Deterministic Rust fetches Google Autocomplete
/// 3. research_seed_validation - LLM filters suggestions for relevance (agentic, structured)
/// 4. research_ahrefs_pipeline - Deterministic Rust calls Ahrefs API
/// 5. research_final_selection - Deterministic filtering/sorting of results
///
/// Steps 1 and 3 use rig's `Extractor<T>` for type-safe structured output,
/// eliminating the need for a separate normalizer step.

mod autocomplete;
mod landing_page;
mod prompts;

pub(crate) use autocomplete::*;
pub(crate) use landing_page::*;
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
/// used to pass data between steps (e.g., autocomplete results to validation).
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
    }
}

/// Execute keyword research via a rig `Agent` with native tool calling.
///
/// This is an experimental alternative to the 5-step hybrid workflow.
/// The agent is given the `keyword_generator` and `keyword_difficulty` tools
/// and decides autonomously how many calls to make and which keywords to
/// prioritize. The result is returned as raw agent text — callers should
/// parse it as `ResearchFinalOutput` or display it directly.
///
/// # Arguments
/// * `provider` — LLM provider name (`"kimi"`, `"claude"`, etc.)
/// * `prompt` — The research prompt (usually built from project brief + themes)
/// * `preamble` — System instructions for the agent
///
/// # Errors
/// Returns `StepResult` with `success: false` if the agent fails or the
/// backend does not support tool calling.
pub async fn exec_keyword_research_with_tools(
    provider: &str,
    prompt: &str,
    preamble: &str,
) -> StepResult {
    use rig::client::CompletionClient;
    use rig::completion::Prompt;

    let backend = crate::rig::provider::resolve_backend(provider, None, None, None).await;

    // Build agent with tools based on resolved backend and run immediately.
    // Each match arm creates a different Agent<M> type, so we run the prompt
    // inside the arm and return the StepResult directly.
    log::info!(
        "[keyword_research_tools] Running tool-agent with provider '{}'",
        provider
    );

    match &backend {
        crate::rig::provider::LlmBackend::KimiBridge { base_url, model } => {
            let client = match rig::providers::openai::Client::builder()
                .base_url(base_url)
                .api_key("dummy")
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to build OpenAI client for bridge: {}", e),
                        output: None,
                    };
                }
            };
            let agent = client
                .completions_api()
                .agent(model)
                .preamble(preamble)
                .tools(crate::engine::tools::boxed_keyword_tools())
                .build();
            run_tool_agent_prompt(agent, prompt).await
        }
        crate::rig::provider::LlmBackend::Claude { api_key, model } => {
            let client = match rig::providers::anthropic::Client::new(api_key) {
                Ok(c) => c,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to build Claude client: {}", e),
                        output: None,
                    };
                }
            };
            let agent = client
                .agent(model)
                .preamble(preamble)
                .tools(crate::engine::tools::boxed_keyword_tools())
                .build();
            run_tool_agent_prompt(agent, prompt).await
        }
        crate::rig::provider::LlmBackend::OpenAi { api_key, model } => {
            let client = match rig::providers::openai::Client::new(api_key) {
                Ok(c) => c,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to build OpenAI client: {}", e),
                        output: None,
                    };
                }
            };
            let agent = client
                .agent(model)
                .preamble(preamble)
                .tools(crate::engine::tools::boxed_keyword_tools())
                .build();
            run_tool_agent_prompt(agent, prompt).await
        }
        crate::rig::provider::LlmBackend::Ollama { base_url, model } => {
            use rig::client::Nothing;
            let client = match rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to build Ollama client: {}", e),
                        output: None,
                    };
                }
            };
            let agent = client
                .agent(model)
                .preamble(preamble)
                .tools(crate::engine::tools::boxed_keyword_tools())
                .build();
            run_tool_agent_prompt(agent, prompt).await
        }
        crate::rig::provider::LlmBackend::KimiDirect => StepResult {
            success: false,
            message:
                "Tool-based research requires a rig-compatible backend (bridge, Claude, OpenAI, Ollama). \
                 KimiDirect CLI fallback does not support tool calling."
                    .to_string(),
            output: None,
        },
    }
}

/// Helper: run a prompt on an agent and convert the result to a StepResult.
async fn run_tool_agent_prompt<A>(agent: A, prompt: &str) -> StepResult
where
    A: rig::completion::Prompt,
{
    match agent.prompt(prompt).await {
        Ok(response) => {
            log::info!(
                "[keyword_research_tools] Agent complete ({} chars)",
                response.len()
            );
            StepResult {
                success: true,
                message: format!(
                    "Tool-based keyword research complete ({} chars)",
                    response.len()
                ),
                output: Some(response),
            }
        }
        Err(e) => {
            log::error!("[keyword_research_tools] Agent failed: {}", e);
            StepResult {
                success: false,
                message: format!("Tool-based keyword research failed: {}", e),
                output: None,
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
    match crate::rig::extraction::extract_structured::<T>(provider, prompt, Some(preamble)).await {
        Ok(output) => {
            let json = match serde_json::to_string_pretty(&output) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!(
                            "Structured extraction for '{}' succeeded but serialization failed: {}",
                            step_name, e
                        ),
                        output: None,
                    };
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
            StepResult {
                success: false,
                message: format!(
                    "Structured extraction for '{}' failed: {}",
                    step_name, e
                ),
                output: None,
            }
        }
    }
}
