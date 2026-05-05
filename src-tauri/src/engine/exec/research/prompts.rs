use crate::engine::project_paths::ProjectPaths;
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
            let system = include_str!("../../../prompts/seed_extraction.md");

            // Build context from project files - primary: project.md, fallback: seo_content_brief.md
            let brief_content = std::fs::read_to_string(paths.automation_dir.join("project.md"))
                .or_else(|_| {
                    find_file(&paths.automation_dir, "seo_content_brief.md")
                        .and_then(|p| std::fs::read_to_string(&p).ok())
                        .ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, ""))
                })
                .unwrap_or_else(|_| "(no brief found)".to_string());

            // ── Coverage summary for smarter seed generation ──────────────────────
            let coverage_summary =
                match crate::engine::exec::coverage::read_keyword_coverage(project_path) {
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
                    Ok(extraction) if !extraction.themes.is_empty() => extraction.themes.join(", "),
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

            let system = include_str!("../../../prompts/seed_validation.md");

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

            // Truncate very long briefs to keep prompts manageable
            const MAX_BRIEF_LEN: usize = 15_000;
            let brief_trimmed = if brief_content.len() > MAX_BRIEF_LEN {
                format!("{}… [truncated]", &brief_content[..MAX_BRIEF_LEN])
            } else {
                brief_content
            };

            // Compact the autocomplete JSON to shave whitespace bytes
            let autocomplete_compact = serde_json::from_str::<serde_json::Value>(autocomplete_json)
                .map(|v| {
                    serde_json::to_string(&v).unwrap_or_else(|_| autocomplete_json.to_string())
                })
                .unwrap_or_else(|_| autocomplete_json.to_string());

            let user = format!(
                "## Project Context\n\n{}\n\n## Autocomplete Results\n\n{}\n\n## Task\n\nFilter each theme's suggestions to only those clearly relevant to this site. Output JSON only.",
                brief_trimmed,
                autocomplete_compact,
            );

            Ok((system.to_string(), user))
        }

        _ => Err(format!("Unknown research step: {}", step_name)),
    }
}
