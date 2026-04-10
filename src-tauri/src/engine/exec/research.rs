/// Research workflow execution module.
///
/// Contains the execution logic for the 3-step research workflow:
/// 1. research_seed_extraction - LLM extracts themes from project brief (agentic)
/// 2. research_ahrefs_pipeline - Deterministic Rust calls Ahrefs API directly
/// 3. research_final_selection - Deterministic filtering/sorting of results
///
/// Only step 1 uses an LLM. Steps 2 and 3 are pure Rust for reliability.

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{StepResult, WorkflowStep};
use crate::models::research::{KeywordPipelineOutput, SelectedKeyword, LandingPageCandidate};
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

            // Build context from project files - primary: project.md, fallback: seo_content_brief.md
            let brief_content = std::fs::read_to_string(paths.automation_dir.join("project.md"))
                .or_else(|_| {
                    find_file(&paths.automation_dir, "seo_content_brief.md")
                        .and_then(|p| std::fs::read_to_string(&p).ok())
                        .ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, ""))
                })
                .unwrap_or_else(|_| "(no brief found)".to_string());

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

        _ => Err(format!("Unknown research step: {}", step_name)),
    }
}
/// Output format matching what the frontend KeywordPicker expects.
/// 
/// The frontend expects either:
/// - `landing_page_candidates` for landing page research
/// - `difficulty.results` for keyword research (wrapped in difficulty object)
#[derive(Debug, Clone, serde::Serialize)]
struct KeywordPickerOutput {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub landing_page_candidates: Vec<LandingPageCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<DifficultyWrapper>,
    pub total_candidates: usize,
    pub filtered_out: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DifficultyWrapper {
    pub total: usize,
    pub successful: usize,
    pub results: Vec<SelectedKeyword>,
}

/// Deterministic final selection of keywords from pipeline output.
///
/// This replaces the agentic step with pure Rust logic:
/// - Filters to keywords with data and KD <= target (default 10)
/// - Sorts by volume (desc), then difficulty (asc)
/// - Takes top N (default 10)
/// - Generates recommended titles based on keyword
pub fn select_keywords_deterministic(
    pipeline_json: &str,
    is_landing_page: bool,
) -> Result<KeywordPickerOutput, String> {
    // Parse pipeline output
    let pipeline: KeywordPipelineOutput = serde_json::from_str(pipeline_json)
        .map_err(|e| format!("Failed to parse pipeline output: {}", e))?;

    let target_kd = 10i64;
    let max_results = 10usize;
    let total_candidates = pipeline.keywords.len();

    // Filter to keywords with data and acceptable KD
    let mut candidates: Vec<_> = pipeline
        .keywords
        .into_iter()
        .filter(|k| {
            let has_data = k.has_data.unwrap_or(false);
            let kd_ok = k.kd.map(|d| d as i64 <= target_kd).unwrap_or(false);
            has_data && kd_ok
        })
        .collect();

    // Sort by volume desc, then KD asc
    candidates.sort_by(|a, b| {
        let vol_cmp = b.volume.unwrap_or(0).cmp(&a.volume.unwrap_or(0));
        if vol_cmp != std::cmp::Ordering::Equal {
            return vol_cmp;
        }
        let kd_a = a.kd.unwrap_or(100.0) as i64;
        let kd_b = b.kd.unwrap_or(100.0) as i64;
        kd_a.cmp(&kd_b)
    });

    // Take top N
    let selected: Vec<_> = candidates.into_iter().take(max_results).collect();
    let filtered_out = total_candidates.saturating_sub(selected.len());

    if is_landing_page {
        Ok(KeywordPickerOutput {
            landing_page_candidates: selected
                .into_iter()
                .map(|k| LandingPageCandidate {
                    keyword: k.keyword.clone(),
                    estimated_volume: k.volume.unwrap_or(0),
                    estimated_kd: k.kd.unwrap_or(0.0) as i64,
                    intent: k.intent.clone().unwrap_or_else(|| "informational".to_string()),
                    landing_page_type: infer_landing_page_type(&k.keyword),
                    opportunity_score: "high".to_string(),
                    opportunity_reason: format!(
                        "KD {} with {} monthly searches",
                        k.kd.map(|d| d as i64).unwrap_or(0),
                        k.volume.unwrap_or(0)
                    ),
                    proposed_title: generate_title(&k.keyword),
                })
                .collect(),
            difficulty: None,
            total_candidates,
            filtered_out,
        })
    } else {
        let results: Vec<_> = selected
            .into_iter()
            .map(|k| SelectedKeyword {
                keyword: k.keyword.clone(),
                volume: k.volume.unwrap_or(0),
                difficulty: k.kd.unwrap_or(0.0) as i64,
                traffic: k.traffic.map(|t| t as i64),
                selection_reason: format!(
                    "Low difficulty (KD {}) with {} monthly searches",
                    k.kd.map(|d| d as i64).unwrap_or(0),
                    k.volume.unwrap_or(0)
                ),
                recommended_title: generate_title(&k.keyword),
            })
            .collect();

        let successful = results.len();
        Ok(KeywordPickerOutput {
            landing_page_candidates: Vec::new(),
            difficulty: Some(DifficultyWrapper {
                total: successful,
                successful,
                results,
            }),
            total_candidates,
            filtered_out,
        })
    }
}

/// Infer landing page type from keyword patterns
fn infer_landing_page_type(keyword: &str) -> String {
    let lower = keyword.to_lowercase();
    if lower.contains("vs") || lower.contains("compare") || lower.contains("alternative") {
        "comparison".to_string()
    } else if lower.contains("best") || lower.contains("top") || lower.contains("review") {
        "category".to_string()
    } else if lower.contains("how to") || lower.contains("guide") || lower.contains("tutorial") {
        "use_case".to_string()
    } else if lower.contains("software") || lower.contains("tool") || lower.contains("app") {
        "feature".to_string()
    } else {
        "category".to_string()
    }
}

/// Generate a readable title from a keyword
fn generate_title(keyword: &str) -> String {
    // Capitalize first letter of each word
    let words: Vec<String> = keyword
        .split_whitespace()
        .enumerate()
        .map(|(i, word)| {
            if i == 0 || !is_stop_word(word) {
                capitalize_first(word)
            } else {
                word.to_lowercase()
            }
        })
        .collect();
    
    let title = words.join(" ");
    
    // Add suffix based on keyword type
    let lower = keyword.to_lowercase();
    if lower.contains("how to") {
        format!("{}: A Step-by-Step Guide", title)
    } else if lower.contains("best") || lower.contains("top") {
        format!("{} for 2025", title)
    } else if lower.contains("vs") {
        format!("{}: Which is Right for You?", title)
    } else {
        format!("{}: Complete Guide", title)
    }
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "a" | "an" | "the" | "and" | "or" | "but" | "in" | "on" | "at" | "to" | "for" | "of" | "with"
    )
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Execute the deterministic final selection step.
///
/// This is called by the executor when it encounters a step with kind "research_final_selection".
/// It reads the previous step's output (keyword pipeline results) and applies deterministic
/// filtering/sorting to select the best candidates.
pub async fn exec_research_final_selection(
    task: &Task,
    project_path: &str,
    previous_output: Option<&str>,
) -> StepResult {
    let pipeline_json = match previous_output {
        Some(out) => out,
        None => {
            return StepResult {
                success: false,
                message: "No previous step output found — expected keyword pipeline results".to_string(),
                output: None,
            };
        }
    };

    let is_landing_page = task.task_type == "research_landing_pages";

    log::info!(
        "[research_final_selection] Running deterministic selection for {} (landing_page={})",
        task.task_type,
        is_landing_page
    );

    match select_keywords_deterministic(pipeline_json, is_landing_page) {
        Ok(output) => {
            let json = match serde_json::to_string_pretty(&output) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to serialize output: {}", e),
                        output: None,
                    };
                }
            };

            let count = if is_landing_page {
                output.landing_page_candidates.len()
            } else {
                output.difficulty.as_ref().map(|d| d.results.len()).unwrap_or(0)
            };

            StepResult {
                success: true,
                message: format!(
                    "Selected {} keywords deterministically (KD <= 10, sorted by volume)",
                    count
                ),
                output: Some(json),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Keyword selection failed: {}", e),
            output: None,
        },
    }
}
