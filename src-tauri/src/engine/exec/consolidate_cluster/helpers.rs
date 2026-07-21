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
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Load the merge plan JSON from the task artifact or automation directory.
pub(crate) fn load_plan_from_task_or_file(task: &Task, project_path: &str) -> String {
    let paths = ProjectPaths::from_path(project_path);

    // Extract cluster_id from task title
    let cluster_id = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .unwrap_or("")
        .trim();

    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() || cluster_id.is_empty() {
        return String::new();
    }

    let strategy: serde_json::Value = serde_json::from_str(&strategy_json).unwrap_or_default();
    let rec = strategy["merge_recommendations"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|r| r["cluster_id"].as_str().unwrap_or("") == cluster_id)
        });

    match rec {
        Some(r) => serde_json::to_string(r).unwrap_or_default(),
        None => String::new(),
    }
}

/// Find an MDX file in the content directory by its URL slug.
///
/// Matching is exact/normalized only — never substring — so a bad match can
/// never patch the wrong live article. Resolution order:
///   1. Exact filename stem match, or numeric-prefixed stem (`001_{slug}`)
///   2. Normalized stem match via `content::slug::normalize_url_slug`
///   3. Frontmatter `url_slug` match (exact or normalized)
///
/// Returns `Ok(None)` when nothing matches. Returns `Err` when more than one
/// file matches — ambiguous cases must fail loudly rather than guess.
pub(crate) fn find_file_by_slug(
    project_path: &str,
    slug: &str,
) -> std::result::Result<Option<PathBuf>, String> {
    let repo_root = Path::new(project_path);
    let content_resolution = crate::content::locator::resolve(repo_root, None);
    let Some(content_dir) = content_resolution.selected else {
        return Ok(None);
    };

    let files = crate::content::locator::collect_markdown_files(&content_dir);
    let slug_normalized = crate::content::slug::normalize_url_slug(slug);

    // Pass 1: filename stem matching (exact, numeric-prefix suffix, normalized).
    let mut matches: Vec<PathBuf> = Vec::new();
    for file in &files {
        let Some(stem) = file.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        let stem_matches = stem == slug
            || stem.ends_with(&format!("_{}", slug))
            || (!slug_normalized.is_empty()
                && crate::content::slug::normalize_url_slug(&stem) == slug_normalized);
        if stem_matches {
            matches.push(file.clone());
        }
    }

    // Pass 2 (fallback): frontmatter `url_slug` matching.
    if matches.is_empty() {
        for file in &files {
            let Ok(content) = std::fs::read_to_string(file) else {
                continue;
            };
            let scalars = crate::content::frontmatter::top_level_scalars(&content);
            for field in scalars {
                if field.key != "url_slug" {
                    continue;
                }
                let fm_slug = field.raw_value.trim_matches('"').trim_matches('\'');
                let fm_matches = fm_slug == slug
                    || (!slug_normalized.is_empty()
                        && crate::content::slug::normalize_url_slug(fm_slug) == slug_normalized);
                if fm_matches {
                    matches.push(file.clone());
                    break;
                }
            }
        }
    }

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => Err(format!(
            "Ambiguous slug '{}': matches multiple files: {}",
            slug,
            matches
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Extract headings from markdown body.
pub(crate) fn extract_headings(body: &str) -> Vec<ExtractedHeading> {
    const MAX_BODY_LINES: usize = 30;
    let mut headings = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        if line.starts_with("## ") || line.starts_with("### ") || line.starts_with("#### ") {
            let level = line.chars().take_while(|&c| c == '#').count() as u8;
            let text = line.trim_start_matches('#').trim().to_string();
            let mut body_lines = Vec::new();
            i += 1;
            while i < lines.len() && body_lines.len() < MAX_BODY_LINES {
                let next = lines[i].trim_start();
                if next.starts_with("## ") || next.starts_with("# ") {
                    break;
                }
                body_lines.push(lines[i]);
                i += 1;
            }
            // Skip remaining lines of this section if truncated
            while i < lines.len() {
                let next = lines[i].trim_start();
                if next.starts_with("## ") || next.starts_with("# ") {
                    break;
                }
                i += 1;
            }
            headings.push(ExtractedHeading {
                level,
                text,
                body: body_lines.join("\n"),
            });
            continue;
        }
        i += 1;
    }
    headings
}

/// Extract markdown tables from body.
pub(crate) fn extract_tables(body: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with('|') {
            let mut table_lines = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                table_lines.push(lines[i]);
                i += 1;
            }
            let markdown = table_lines.join("\n");
            tables.push(ExtractedTable {
                caption: None,
                markdown,
            });
            continue;
        }
        i += 1;
    }
    tables
}

/// Extract code block examples from body.
pub(crate) fn extract_examples(body: &str) -> Vec<ExtractedExample> {
    const MAX_CODE_LINES: usize = 40;
    let mut examples = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("```") {
            let fence = lines[i].trim_start();
            let lang = fence
                .strip_prefix("```")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                if code_lines.len() < MAX_CODE_LINES {
                    code_lines.push(lines[i]);
                }
                i += 1;
            }
            i += 1; // skip closing fence
            examples.push(ExtractedExample {
                caption: None,
                code: code_lines.join("\n"),
                language: lang,
            });
            continue;
        }
        i += 1;
    }
    examples
}

/// Extract FAQ-style Q&A from body (lines matching "Q:" / "A:" or "**Q:**" patterns).
pub(crate) fn extract_faqs(body: &str) -> Vec<ExtractedFaq> {
    const MAX_ANSWER_LINES: usize = 20;
    let mut faqs = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let q_match =
            line.starts_with("Q:") || line.starts_with("**Q:**") || line.starts_with("**Q:**");
        if q_match {
            let question = line
                .strip_prefix("Q:")
                .or_else(|| line.strip_prefix("**Q:**"))
                .unwrap_or(line)
                .trim()
                .to_string();
            i += 1;
            let mut answer_lines = Vec::new();
            while i < lines.len() {
                let next = lines[i].trim();
                if next.starts_with("Q:")
                    || next.starts_with("**Q:**")
                    || next.starts_with("A:")
                    || next.starts_with("**A:**")
                {
                    break;
                }
                if answer_lines.len() < MAX_ANSWER_LINES {
                    answer_lines.push(lines[i]);
                }
                i += 1;
            }
            // Check if next line is "A:"
            if i < lines.len() {
                let a_line = lines[i].trim();
                if a_line.starts_with("A:") || a_line.starts_with("**A:**") {
                    if answer_lines.len() < MAX_ANSWER_LINES {
                        answer_lines.push(
                            a_line
                                .strip_prefix("A:")
                                .or_else(|| a_line.strip_prefix("**A:**"))
                                .unwrap_or(a_line)
                                .trim(),
                        );
                    }
                    i += 1;
                }
            }
            faqs.push(ExtractedFaq {
                question,
                answer: answer_lines.join("\n").trim().to_string(),
            });
            continue;
        }
        i += 1;
    }
    faqs
}

