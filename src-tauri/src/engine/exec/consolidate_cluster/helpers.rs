use std::path::{Path, PathBuf};

use crate::models::merge_patch::{
    ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
};
use crate::models::task::Task;

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Load the merge plan JSON from the task artifact or automation directory.
///
/// Soft wrapper around the shared [`crate::engine::merge_apply::load_plan_json_from_task`]
/// primitive — returns empty string on miss so desktop steps keep their
/// historical no-op-on-empty behavior. Path B uses the fail-closed variant.
pub(crate) fn load_plan_from_task_or_file(task: &Task, project_path: &str) -> String {
    crate::engine::merge_apply::load_plan_json_from_task_soft(task, project_path)
}

/// Find an MDX file in the content directory by its URL slug.
///
/// Thin wrapper around the canonical [`crate::content::ops::find_file_by_slug`].
pub(crate) fn find_file_by_slug(
    project_path: &str,
    slug: &str,
) -> std::result::Result<Option<PathBuf>, String> {
    crate::content::ops::find_file_by_slug(Path::new(project_path), slug)
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

