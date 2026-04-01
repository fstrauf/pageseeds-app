/// Research workflow execution module.
///
/// Contains the execution logic for the 3-step research workflow:
/// 1. research_seed_extraction - LLM extracts themes from project brief
/// 2. research_keyword_discovery - ToolCallingAgent fetches keyword data
/// 3. research_final_selection - LLM selects best candidates

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{StepResult, WorkflowStep};
use crate::models::task::Task;

/// Execute a research workflow step using ToolCallingAgent
///
/// This handles the 3-step unified workflow:
/// 1. research_seed_extraction
/// 2. research_keyword_discovery
/// 3. research_final_selection
///
/// The `previous_output` parameter contains the output from the previous step,
/// used to pass data between steps (e.g., themes from step 1 to step 2).
pub async fn exec_research_workflow_step(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    previous_output: Option<&str>,
) -> StepResult {
    use crate::engine::tool_agent::{AgentConfig, ToolCallingAgent};
    use crate::engine::tools::{ToolRegistry, KeywordDifficultyTool, KeywordGeneratorTool};

    let paths = ProjectPaths::from_path(project_path);

    // Create tool registry with keyword tools
    let mut tools = ToolRegistry::new();
    tools.register(KeywordGeneratorTool);
    tools.register(KeywordDifficultyTool);

    // Create agent config (bridge to kimi-acp-openai-bridge)
    let config = AgentConfig {
        base_url: "http://localhost:8080/v1".to_string(),
        model: "kimi-k2.5".to_string(),
        api_key: "not-needed-for-bridge".to_string(),
    };

    let agent = ToolCallingAgent::new(config, tools);

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
            }
        }
    };

    log::info!(
        "[research_workflow] Executing '{}' with ToolCallingAgent",
        step.name
    );

    // Run the agent
    match agent.run(&system_prompt, &user_prompt, 10).await {
        Ok(result) => {
            log::info!(
                "[research_workflow] '{}' complete ({} chars, {} tool calls)",
                step.name,
                result.content.len(),
                result.tool_calls_executed
            );

            StepResult {
                success: true,
                message: format!(
                    "Research step '{}' complete ({} chars, {} tool calls)",
                    step.name,
                    result.content.len(),
                    result.tool_calls_executed
                ),
                output: Some(result.content),
            }
        }
        Err(e) => {
            log::error!("[research_workflow] '{}' failed: {}", step.name, e);

            StepResult {
                success: false,
                message: format!("Research step '{}' failed: {}", step.name, e),
                output: None,
            }
        }
    }
}

/// Build system and user prompts for a research workflow step
///
/// The `previous_output` parameter contains the output from the previous step,
/// allowing data to flow between steps in the workflow.
pub fn build_research_prompts(
    step_name: &str,
    task: &Task,
    project_path: &str,
    paths: &ProjectPaths,
    previous_output: Option<&str>,
) -> Result<(String, String), String> {
    // Helper: find file by suffix pattern
    fn find_file(dir: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
        let suffix_lower = suffix.to_lowercase();
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_lowercase().contains(&suffix_lower))
                    .unwrap_or(false)
            })
    }

    match step_name {
        "research_seed_extraction" => {
            let system = include_str!("../../prompts/seed_extraction.md");

            // Build context from project files - use pattern matching for brief file
            let brief_content = find_file(&paths.automation_dir, "seo_content_brief.md")
                .and_then(|p| std::fs::read_to_string(&p).ok())
                .unwrap_or_else(|| "(no brief found)".to_string());

            let user = format!(
                "## Project Context\n\n{}\n\n## Task Description\n\n{}\n\n## Project Path\n\n{}",
                brief_content,
                task.description.as_deref().unwrap_or("(no description)"),
                project_path
            );

            Ok((system.to_string(), user))
        }

        "research_keyword_discovery" => {
            // This step is now handled by the deterministic keyword_research_native step.
            // The old agentic discovery with ToolCallingAgent has been replaced.
            // This prompt builder remains for backward compatibility.
            
            // Get themes from previous_output (parsed as typed SeedExtractionOutput)
            let themes = if let Some(prev) = previous_output {
                // Try to parse as typed output
                match crate::models::research::parse_seed_extraction(prev) {
                    Ok(extraction) if !extraction.themes.is_empty() => {
                        extraction.themes.join(", ")
                    }
                    _ => {
                        // Fallback: try generic JSON parsing
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(prev) {
                            if let Some(arr) = json.get("themes").and_then(|t| t.as_array()) {
                                arr.iter()
                                    .filter_map(|t| t.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            } else {
                                prev.to_string()
                            }
                        } else {
                            prev.to_string()
                        }
                    }
                }
            } else {
                "(no themes - this should not happen in the hybrid workflow)".to_string()
            };
            
            // Return a placeholder - this step kind is now handled by keyword_research_native
            let system = "You are a placeholder. The actual keyword discovery now runs via deterministic Rust code.";
            let user = format!("Themes that would be researched: {}", themes);
            
            Ok((system.to_string(), user))
        }

        "research_final_selection" => {
            // Choose prompt based on task type
            let is_landing_page = task.task_type == "research_landing_pages";

            let system = if is_landing_page {
                include_str!("../../prompts/final_selection_landing_pages.md")
            } else {
                include_str!("../../prompts/final_selection_keywords.md")
            };

            // Get keyword data from previous step output (Step 2 -> Step 3)
            // Parse as typed KeywordPipelineOutput for structured access
            let keyword_data = if let Some(prev) = previous_output {
                // Try to parse as typed output first
                match crate::models::research::parse_keyword_pipeline(prev) {
                    Ok(pipeline) => {
                        // Format as pretty JSON for the LLM prompt
                        serde_json::to_string_pretty(&pipeline)
                            .unwrap_or_else(|_| prev.to_string())
                    }
                    Err(_) => {
                        // Fallback to raw if parsing fails
                        prev.to_string()
                    }
                }
            } else {
                task.description
                    .as_deref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "{\"keywords\": [], \"themes\": [], \"total_candidates\": 0, \"with_data_count\": 0}".to_string())
            };

            let user = format!(
                "## Keyword Research Data\n\n{}\n\nSelect the best candidates based on the criteria above. \
                 Return ONLY valid JSON matching the output contract specified in the system prompt.",
                keyword_data
            );

            Ok((system.to_string(), user))
        }

        _ => Err(format!("Unknown research step: {}", step_name)),
    }
}
