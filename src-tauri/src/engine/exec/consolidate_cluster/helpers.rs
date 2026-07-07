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
pub(crate) fn find_file_by_slug(project_path: &str, slug: &str) -> Option<PathBuf> {
    let repo_root = Path::new(project_path);
    let content_resolution = crate::content::locator::resolve(repo_root, None);
    let content_dir = content_resolution.selected?;

    let files = crate::content::locator::collect_markdown_files(&content_dir);

    // Normalize slug for matching: kebab-case and snake_case should match
    let slug_normalized = slug.replace('-', "_");

    for file in files {
        let name = file.file_stem()?.to_string_lossy().to_string();
        let name_normalized = name.replace('-', "_");

        // Slug might be in filename like "001_best_stocks_csp" → we look for the slug part
        if name == slug
            || name.ends_with(&format!("_{}", slug))
            || name.contains(slug)
            || name_normalized == slug_normalized
            || name_normalized.ends_with(&format!("_{}", slug_normalized))
            || name_normalized.contains(&slug_normalized)
        {
            return Some(file);
        }
        // Also check frontmatter for url_slug match
        if let Ok(content) = std::fs::read_to_string(&file) {
            let scalars = crate::content::frontmatter::top_level_scalars(&content);
            for field in scalars {
                let fm_slug = field.raw_value.trim_matches('"').trim_matches('\'');
                let fm_slug_normalized = fm_slug.replace('-', "_");
                if field.key == "url_slug"
                    && (fm_slug == slug || fm_slug_normalized == slug_normalized)
                {
                    return Some(file);
                }
            }
        }
    }

    None
}

/// Extract headings from markdown body.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

