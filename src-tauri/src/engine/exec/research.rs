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

/// Build a deterministic text summary from keyword_coverage.json for the seed
/// extraction prompt.  Groups clusters by article count so the LLM can avoid
/// over-covered topics and prioritise thin gaps.
fn build_coverage_summary(coverage: &serde_json::Value) -> String {
    let empty_clusters: Vec<serde_json::Value> = vec![];
    let clusters = coverage
        .get("clusters")
        .and_then(|c| c.as_array())
        .unwrap_or(&empty_clusters);

    let mut strong: Vec<(String, i64)> = vec![];
    let mut moderate: Vec<(String, i64)> = vec![];
    let mut thin: Vec<(String, i64)> = vec![];

    for c in clusters {
        let name = c
            .get("cluster_name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let count = c.get("article_count").and_then(|n| n.as_i64()).unwrap_or(0);
        match count {
            0..=2 => thin.push((name, count)),
            3..=5 => moderate.push((name, count)),
            _ => strong.push((name, count)),
        }
    }

    let mut lines: Vec<String> = vec![];

    if !strong.is_empty() {
        lines.push("Strong coverage (skip these):".to_string());
        for (name, count) in strong {
            lines.push(format!("- {} ({} articles)", name, count));
        }
    }

    if !moderate.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Moderate coverage (ok to supplement):".to_string());
        for (name, count) in moderate {
            lines.push(format!("- {} ({} articles)", name, count));
        }
    }

    if !thin.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Thin coverage (good candidates to deepen):".to_string());
        for (name, count) in thin {
            lines.push(format!("- {} ({} articles)", name, count));
        }
    }

    if lines.is_empty() {
        "No existing content coverage found.".to_string()
    } else {
        lines.join("\n")
    }
}

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
            }
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

            // ── Coverage summary for smarter seed generation ──────────────────────
            let coverage_summary = match crate::engine::exec::coverage::read_keyword_coverage(project_path) {
                Some(coverage) => build_coverage_summary(&coverage),
                None => {
                    return Err(
                        "keyword_coverage.json not found. Run 'Analyze Keyword Coverage' first."
                            .to_string(),
                    );
                }
            };

            let user = format!(
                "## Project Context\n\n{}\n\n## Existing Content Coverage\n\n{}\n\n## Task Description\n\n{}\n\n## Project Path\n\n{}",
                brief_content,
                coverage_summary,
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

        "research_seed_validation" => {
            // Agentic: LLM filters autocomplete suggestions for domain relevance.
            //
            // Why agentic: "is 'options benefits' relevant to an options income tool?"
            // requires understanding the site's domain and user intent. Hard-coding a
            // relevance rule would silently fail on any input it wasn't tested against.
            //
            // Input: research_autocomplete artifact — [{theme, suggestions: [...]}]
            // Output contract: {validated_seeds: [{theme: string, seeds: [string]}]}
            // Each theme should produce 1-3 validated seeds that are clearly on-topic.

            let system = include_str!("../../prompts/seed_validation.md");

            // Read the autocomplete artifact from the task
            let autocomplete_json = task
                .artifacts
                .iter()
                .rev()
                .find(|a| a.key == "research_autocomplete")
                .and_then(|a| a.content.as_deref())
                .unwrap_or_else(|| previous_output.unwrap_or("(no autocomplete data)"));

            // Also load the project brief so the LLM has domain context
            let brief_content = std::fs::read_to_string(paths.automation_dir.join("project.md"))
                .unwrap_or_else(|_| "(no brief found)".to_string());

            let user = format!(
                "## Project Context\n\n{}\n\n## Autocomplete Results\n\n{}\n\n## Task\n\nFilter each theme's suggestions to only those clearly relevant to this site. Output JSON only.",
                brief_content,
                autocomplete_json,
            );

            Ok((system.to_string(), user))
        }

        _ => Err(format!("Unknown research step: {}", step_name)),
    }
}

/// Deterministic Step 2: fetch Google Autocomplete suggestions for all themes.
///
/// Reads the `research_seed_extraction` artifact from the task, calls Google
/// Autocomplete (free, no auth) for each theme, and outputs structured JSON:
/// `[{theme: string, suggestions: [string]}]`
///
/// This output is consumed by the agentic Step 3 (research_seed_validation).
pub fn exec_research_autocomplete(task: &Task, project_path: &str) -> StepResult {
    use crate::engine::exec::keywords::parse_seed_extraction_artifact;

    let seed_artifact = parse_seed_extraction_artifact(task);

    if seed_artifact.themes.is_empty() {
        return StepResult {
            success: false,
            message: "No themes found in research_seed_extraction artifact. Step 1 must run first.".to_string(),
            output: None,
        };
    }

    log::info!(
        "[research_autocomplete] Fetching Google Autocomplete for {} themes",
        seed_artifact.themes.len()
    );

    let themes = seed_artifact.themes.clone();
    let project_path = project_path.to_string();

    let thread_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        rt.block_on(async move {
            let mut results: Vec<serde_json::Value> = Vec::new();

            for theme in &themes {
                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

                let suggestions = match crate::seo::google_autocomplete::fetch_suggestions(theme, "us", "en").await {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("[research_autocomplete] Autocomplete failed for '{}': {}", theme, e);
                        vec![]
                    }
                };

                let suggestion_list: Vec<String> = suggestions
                    .iter()
                    .take(6)
                    .map(|s| s.keyword.clone())
                    .collect();

                log::info!(
                    "[research_autocomplete] theme '{}' → {} suggestions: {:?}",
                    theme,
                    suggestion_list.len(),
                    suggestion_list
                );

                results.push(serde_json::json!({
                    "theme": theme,
                    "suggestions": suggestion_list,
                }));
            }

            Ok::<Vec<serde_json::Value>, String>(results)
        })
    })
    .join()
    .map_err(|_| "Autocomplete thread panicked".to_string());

    match thread_result {
        Ok(Ok(results)) => {
            let json = serde_json::to_string_pretty(&results)
                .unwrap_or_else(|_| "[]".to_string());
            log::info!(
                "[research_autocomplete] Complete — {} theme entries ({} chars)",
                results.len(),
                json.len()
            );
            StepResult {
                success: true,
                message: format!("Google Autocomplete fetched for {} themes", results.len()),
                output: Some(json),
            }
        }
        Ok(Err(e)) => StepResult {
            success: false,
            message: format!("Autocomplete failed: {}", e),
            output: None,
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Autocomplete thread error: {}", e),
            output: None,
        },
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

    let target_kd = 30i64; // 0-100 scale (DataForSEO/Ahrefs unified)
    let max_results = 10usize;
    let total_candidates = pipeline.keywords.len();

    // Filter to keywords with data, acceptable KD, and non-navigational intent
    let mut candidates: Vec<_> = pipeline
        .keywords
        .into_iter()
        .filter(|k| {
            let has_data = k.has_data.unwrap_or(false);
            let kd_ok = k.kd.map(|d| d as i64 <= target_kd).unwrap_or(false);
            // Reject navigational intent (brand searches like "nike air force 1")
            let intent_ok = k.intent.as_deref()
                .map(|i| !i.eq_ignore_ascii_case("navigational"))
                .unwrap_or(true);
            has_data && kd_ok && intent_ok
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

// ─── Landing Page Spec Writer ─────────────────────────────────────────────────

/// Deterministic step: write a structured landing page spec file from the task's
/// keyword metadata. No LLM needed — the spec is a template populated with
/// keyword, page type, intent, volume, and KD from the research output.
///
/// Output: writes `specs/landing_page_spec_{slug}.md` inside the automation dir.
pub fn exec_landing_page_spec_write(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let specs_dir = paths.automation_dir.join("specs");

    if let Err(e) = std::fs::create_dir_all(&specs_dir) {
        return StepResult {
            success: false,
            message: format!("Failed to create specs directory: {}", e),
            output: None,
        };
    }

    // Parse metadata from task description (format: "Target keyword: X\nKD: Y\nVolume: Z\n...")
    let desc = task.description.as_deref().unwrap_or("");
    let meta = parse_landing_page_meta(desc);

    let slug = slugify(&meta.keyword);
    let filename = format!("landing_page_spec_{}.md", slug);
    let spec_path = specs_dir.join(&filename);

    // Don't overwrite an existing spec — it may have been manually edited.
    if spec_path.exists() {
        return StepResult {
            success: true,
            message: format!("Spec already exists: specs/{}", filename),
            output: Some(format!("specs/{}", filename)),
        };
    }

    let spec_content = build_spec_markdown(&meta, task);

    match std::fs::write(&spec_path, &spec_content) {
        Ok(()) => {
            log::info!(
                "[landing_page_spec] wrote spec: {}",
                spec_path.display()
            );
            StepResult {
                success: true,
                message: format!("Landing page spec written: specs/{}", filename),
                output: Some(format!("specs/{}", filename)),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Failed to write spec file: {}", e),
            output: None,
        },
    }
}

struct LandingPageMeta {
    keyword: String,
    kd: Option<i64>,
    volume: Option<i64>,
    intent: Option<String>,
    landing_page_type: Option<String>,
    proposed_title: Option<String>,
    opportunity_reason: Option<String>,
}

/// Parse landing page metadata from the task description.
///
/// Expected format (lines):
///   Target keyword: <keyword>
///   KD: <number>
///   Volume: <number>
///   Intent: <string>
///   Page type: <string>
///   Proposed title: <string>
///   Opportunity: <string>
fn parse_landing_page_meta(desc: &str) -> LandingPageMeta {
    let mut meta = LandingPageMeta {
        keyword: String::new(),
        kd: None,
        volume: None,
        intent: None,
        landing_page_type: None,
        proposed_title: None,
        opportunity_reason: None,
    };

    for line in desc.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("Target keyword:") {
            meta.keyword = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("KD:") {
            meta.kd = val.trim().parse().ok();
        } else if let Some(val) = line.strip_prefix("Volume:") {
            meta.volume = val.trim().parse().ok();
        } else if let Some(val) = line.strip_prefix("Intent:") {
            meta.intent = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Page type:") {
            meta.landing_page_type = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Proposed title:") {
            meta.proposed_title = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Opportunity:") {
            meta.opportunity_reason = Some(val.trim().to_string());
        }
    }

    // Fallback: use task title as keyword if description didn't have it
    if meta.keyword.is_empty() {
        if let Some(title) = task_title_fallback(desc) {
            meta.keyword = title;
        }
    }

    meta
}

fn task_title_fallback(desc: &str) -> Option<String> {
    desc.lines().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string())
}

fn build_spec_markdown(meta: &LandingPageMeta, task: &Task) -> String {
    let title = meta.proposed_title.as_deref()
        .unwrap_or(&meta.keyword);
    let page_type = meta.landing_page_type.as_deref().unwrap_or("category");
    let intent = meta.intent.as_deref().unwrap_or("commercial");

    let mut out = String::with_capacity(2048);

    out.push_str(&format!("# Landing Page Spec: {}\n\n", title));

    out.push_str("## Keyword Research\n\n");
    out.push_str("| Field | Value |\n|---|---|\n");
    out.push_str(&format!("| Target keyword | {} |\n", meta.keyword));
    if let Some(kd) = meta.kd {
        out.push_str(&format!("| Keyword difficulty | {} |\n", kd));
    }
    if let Some(vol) = meta.volume {
        out.push_str(&format!("| Monthly volume | {} |\n", vol));
    }
    out.push_str(&format!("| Search intent | {} |\n", intent));
    out.push_str(&format!("| Page type | {} |\n", page_type));
    if let Some(reason) = &meta.opportunity_reason {
        out.push_str(&format!("| Opportunity | {} |\n", reason));
    }
    out.push('\n');

    out.push_str("## Page Structure\n\n");

    match page_type {
        "comparison" => {
            out.push_str("This is a **comparison** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — headline addressing the comparison query + value prop\n");
            out.push_str("2. Quick comparison table — side-by-side feature matrix\n");
            out.push_str("3. Detailed breakdown — pros/cons for each option\n");
            out.push_str("4. Use case recommendations — \"Choose X if…\" guidance\n");
            out.push_str("5. CTA — clear next step for the reader\n");
        }
        "use_case" => {
            out.push_str("This is a **use case** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — problem statement + how the product solves it\n");
            out.push_str("2. Step-by-step walkthrough — show the workflow\n");
            out.push_str("3. Benefits — concrete outcomes with evidence\n");
            out.push_str("4. Social proof — testimonials or case study snippets\n");
            out.push_str("5. CTA — get started / try it free\n");
        }
        "feature" => {
            out.push_str("This is a **feature** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — feature name + one-line benefit\n");
            out.push_str("2. Problem/solution — what pain this feature eliminates\n");
            out.push_str("3. How it works — visual walkthrough or demo\n");
            out.push_str("4. Integration/compatibility — what it connects with\n");
            out.push_str("5. CTA — try the feature\n");
        }
        _ => {
            // "category" and default
            out.push_str("This is a **category** landing page.\n\n");
            out.push_str("### Recommended Sections\n\n");
            out.push_str("1. Hero — category overview + primary value prop\n");
            out.push_str("2. Key capabilities — 3-5 feature highlights\n");
            out.push_str("3. Who it's for — target audience segments\n");
            out.push_str("4. Social proof — logos, testimonials, or metrics\n");
            out.push_str("5. CTA — primary conversion action\n");
        }
    }

    out.push_str("\n\n## SEO Requirements\n\n");
    out.push_str("- Primary keyword in H1 and meta title\n");
    out.push_str(&format!("- Target keyword: **{}**\n", meta.keyword));
    out.push_str("- Meta description: 150-160 chars, include keyword naturally\n");
    out.push_str("- URL slug should contain the primary keyword\n");
    out.push_str("- Include structured data (FAQ, HowTo, or Product schema as appropriate)\n");

    out.push_str("\n## Implementation Notes\n\n");
    out.push_str("- This spec defines what the landing page should contain.\n");
    out.push_str("- Implement using the repo's landing page framework/templates.\n");
    out.push_str(&format!("- Source task: `{}`\n", task.id));

    out
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}
