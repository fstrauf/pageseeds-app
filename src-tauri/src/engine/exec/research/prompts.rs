use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

/// Build a deterministic text summary from keyword_coverage.json for the seed
/// extraction prompt.
///
/// Critical SEO nuance: a high article count does NOT mean a topic is exhausted.
/// "Covered calls" with 16 articles still has thousands of long-tail variations
/// (rolldowns, assignment tax, strike selection, dividend capture, etc.). We
/// therefore reframe strong clusters as "expand into new angles, do not repeat
/// primers" rather than "skip these". Thin/moderate clusters remain priority
/// gaps, but the agent must not ignore the site's proven authority clusters.
fn build_coverage_summary(coverage: &serde_json::Value) -> String {
    let empty_clusters: Vec<serde_json::Value> = vec![];
    let clusters = coverage
        .get("clusters")
        .and_then(|c| c.as_array())
        .unwrap_or(&empty_clusters);

    let mut strong: Vec<(String, i64, Vec<String>)> = vec![];
    let mut moderate: Vec<(String, i64, Vec<String>)> = vec![];
    let mut thin: Vec<(String, i64, Vec<String>)> = vec![];

    for c in clusters {
        let name = c
            .get("cluster_name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let count = c.get("article_count").and_then(|n| n.as_i64()).unwrap_or(0);
        let primary: Vec<String> = c
            .get("primary_keywords")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        match count {
            0..=2 => thin.push((name, count, primary)),
            3..=5 => moderate.push((name, count, primary)),
            _ => strong.push((name, count, primary)),
        }
    }

    let mut lines: Vec<String> = vec![
        "Coverage signal: article count shows topical authority, not completion. \
         Strong clusters are where the site already has credibility — the right move \
         is to find new sub-angles, not to avoid the topic entirely."
            .to_string(),
        String::new(),
    ];

    if !strong.is_empty() {
        lines.push(
            "Strong coverage — find NEW angles only; do NOT write another 'what is X' primer:"
                .to_string(),
        );
        for (name, count, primary) in strong {
            let primary_hint = if primary.is_empty() {
                String::new()
            } else {
                format!(" (known angles: {})", primary.join(", "))
            };
            lines.push(format!("- {} ({} articles){}", name, count, primary_hint));
        }
    }

    if !moderate.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Moderate coverage — ok to supplement with adjacent angles:".to_string());
        for (name, count, primary) in moderate {
            let primary_hint = if primary.is_empty() {
                String::new()
            } else {
                format!(" (known angles: {})", primary.join(", "))
            };
            lines.push(format!("- {} ({} articles){}", name, count, primary_hint));
        }
    }

    if !thin.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Thin coverage — good candidates to deepen:".to_string());
        for (name, count, primary) in thin {
            let primary_hint = if primary.is_empty() {
                String::new()
            } else {
                format!(" (known angles: {})", primary.join(", "))
            };
            lines.push(format!("- {} ({} articles){}", name, count, primary_hint));
        }
    }

    if lines.len() <= 2 {
        "No existing content coverage found.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Build a text summary from the research_shortlist SQLite table.
/// Shows pending open territories so the LLM can prioritize them.
/// Depleted themes (health_status = 'depleted') are excluded so the prompt
/// never tells the LLM to prioritize topics that have stopped producing results.
fn build_shortlist_summary(project_id: &str) -> String {
    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => return "(shortlist unavailable)".to_string(),
    };
    let entries = match crate::db::research_shortlist::list_pending_excluding_depleted(&conn, project_id) {
        Ok(e) => e,
        Err(_) => return "(shortlist unavailable)".to_string(),
    };

    if entries.is_empty() {
        return "No pending territory research items.".to_string();
    }

    let mut lines: Vec<String> = vec![
        "The following themes were identified as open territories (low coverage + high impressions). \
         Prioritize these in your theme extraction:".to_string(),
    ];
    for entry in entries {
        let seed_hint = if entry.seeds.is_empty() {
            String::new()
        } else {
            format!(" → suggested seeds: {}", entry.seeds.join(", "))
        };
        lines.push(format!(
            "- {} ({} impressions, {} articles){}",
            entry.theme,
            entry.total_impressions.map(|i| format!("{:.0}", i)).unwrap_or_else(|| "unknown".to_string()),
            entry.article_count.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()),
            seed_hint
        ));
    }
    lines.join("\n")
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
                    None => "(no coverage data available)".to_string(),
                };

            // ── Research shortlist (open territories from GSC analysis) ───────────
            let shortlist_summary = build_shortlist_summary(&task.project_id);

            // Landing page research carries a JSON strategy dialog payload
            // ({"context": ..., "themes": [...]}). Surface it as labeled
            // sections instead of raw JSON so the extractor treats user themes
            // as authoritative seeds and the context as strategy guidance.
            let task_section = match crate::engine::task_store::landing_page_strategy(task) {
                Some((context, user_themes)) => {
                    let mut section = String::new();
                    if !context.is_empty() {
                        section.push_str(&format!("## Strategy Context\n\n{}\n\n", context));
                    }
                    if !user_themes.is_empty() {
                        section.push_str(&format!(
                            "## User-Supplied Themes\n\nThe user explicitly requested these themes — include them verbatim in your extraction:\n\n{}\n\n",
                            user_themes.join("\n")
                        ));
                    }
                    if section.is_empty() {
                        section = format!(
                            "## Task Description\n\n{}\n\n",
                            task.description.as_deref().unwrap_or("(no description)")
                        );
                    }
                    section
                }
                None => format!(
                    "## Task Description\n\n{}\n\n",
                    task.description.as_deref().unwrap_or("(no description)")
                ),
            };

            let user = format!(
                "## Project Context\n\n{}\n\n## Existing Content Coverage\n\n{}\n\n## Research Shortlist (Priority Territories)\n\n{}\n\n{}## Project Path\n\n{}",
                brief_content,
                coverage_summary,
                shortlist_summary,
                task_section,
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
            // Agentic: LLM validates extracted themes for domain relevance and
            // proposes 1-3 sharpened seed phrasings per on-topic theme.
            //
            // Why agentic: "is 'options benefits' relevant to an options income tool?"
            // requires understanding the site's domain and user intent. Hard-coding a
            // relevance rule would silently fail on any input it wasn't tested against.
            //
            // Input: research_seed_extraction artifact — {themes: [string], competitors: [...]}
            // Output contract: {validated_seeds: [{theme: string, seeds: [string]}]}
            // Each on-topic theme should produce 1-3 seeds phrased like real search queries.

            let system = include_str!("../../../prompts/seed_validation.md");

            // Read the seed extraction artifact from the task
            let extraction_json = task
                .artifacts
                .iter()
                .rev()
                .find(|a| a.key == "research_seed_extraction")
                .and_then(|a| a.content.as_deref())
                .unwrap_or_else(|| previous_output.unwrap_or("(no seed extraction data)"));

            // Extract the themes list for a compact prompt
            let themes_compact = crate::models::research::parse_seed_extraction(extraction_json)
                .map(|e| {
                    serde_json::to_string(&serde_json::json!({ "themes": e.themes }))
                        .unwrap_or_else(|_| extraction_json.to_string())
                })
                .unwrap_or_else(|_| extraction_json.to_string());

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

            // Landing page research may carry a user-written strategy context —
            // surface it as a labeled section so validation judges relevance
            // against the user's stated goals, not just the brief.
            let strategy_section = match crate::engine::task_store::landing_page_strategy(task) {
                Some((context, _)) if !context.is_empty() => {
                    format!("## Strategy Context\n\n{}\n\n", context)
                }
                _ => String::new(),
            };

            let user = format!(
                "## Project Context\n\n{}\n\n{}## Extracted Themes\n\n{}\n\n## Task\n\nValidate each theme for domain relevance and propose 1-3 seed phrasings per on-topic theme. Output JSON only.",
                brief_trimmed,
                strategy_section,
                themes_compact,
            );

            Ok((system.to_string(), user))
        }

        _ => Err(format!("Unknown research step: {}", step_name)),
    }
}
