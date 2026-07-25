/// MDX content cleaner.
///
/// Mirrors `packages/seo-content-cli/src/seo_content_mcp/content_cleaner.py`.
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, Serialize)]
pub struct CleaningIssue {
    pub file: String,
    pub issue_type: String,
    pub description: String,
    pub fixed: bool,
}

#[derive(Debug, Serialize)]
pub struct CleaningResult {
    pub files_checked: usize,
    pub issues: Vec<CleaningIssue>,
    pub issues_fixed: usize,
}

/// Parse YAML frontmatter from MDX content — delegates to `frontmatter::split_mdx`.
pub fn parse_frontmatter(content: &str) -> Option<(&str, &str)> {
    crate::content::frontmatter::split_mdx(content)
}

/// Extract a quoted string value from YAML frontmatter.
fn extract_frontmatter_value<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        let prefix = format!("{key}:");
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Words that strongly suggest a title was cut off mid-phrase.
const DANGLING_WORDS: &[&str] = &[
    "to", "a", "an", "the", "and", "or", "for", "of", "in", "on", "at", "with", "by", "from",
    "as", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does",
    "did", "will", "would", "could", "should", "may", "might", "must", "can", "this", "that",
    "these", "those", "your", "my", "how", "what", "why", "when", "where", "who", "which",
    "while", "vs", "versus",
];

const DANGLING_PUNCT: &[char] = &[':', ',', ';', '-', '—'];

/// Returns true if the title looks like it was truncated mid-phrase.
fn is_title_truncated(title: &str) -> bool {
    let t = title.trim().trim_matches('"').trim_matches('\'');
    if t.is_empty() {
        return true;
    }
    if t.ends_with(DANGLING_PUNCT) {
        return true;
    }
    let last_word = t.split_whitespace().last().unwrap_or("").to_lowercase();
    let last_word = last_word.trim_matches('"').trim_matches('\'');
    if DANGLING_WORDS.contains(&last_word) {
        return true;
    }
    // Unmatched double quote
    if t.chars().filter(|&c| c == '"').count() % 2 != 0 {
        return true;
    }
    false
}

/// Scan a single MDX file for cleaning issues. Optionally apply fixes.
///
/// Issues detected:
/// - missing_frontmatter: no `---` block
/// - duplicate_title: body starts with `# <title>` matching the frontmatter title
/// - blank_line_after_frontmatter: no blank line between closing `---` and first body line
/// - truncated_title: frontmatter title ends with a dangling word or punctuation
/// - lowercase_intro: first body paragraph starts with a lowercase letter
fn check_file(path: &Path, dry_run: bool) -> Result<Vec<CleaningIssue>> {
    let content = std::fs::read_to_string(path)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut issues = Vec::new();

    let Some((frontmatter, _body)) = parse_frontmatter(&content) else {
        issues.push(CleaningIssue {
            file: file_name,
            issue_type: "missing_frontmatter".into(),
            description: "No frontmatter block found".into(),
            fixed: false,
        });
        return Ok(issues);
    };

    let title = extract_frontmatter_value(frontmatter, "title");

    // Detect duplicate title heading
    if let Some(title_val) = title {
        // Find where body starts (after closing ---)
        let fm_end = content.find("\n---\n").map(|i| i + 5);
        if let Some(body_start) = fm_end {
            let body = &content[body_start..];
            let body_trimmed = body.trim_start_matches('\n');
            let expected = format!("# {title_val}");
            if body_trimmed.starts_with(&expected) {
                let fixed = if !dry_run {
                    // Remove the duplicate heading line
                    let new_body = body_trimmed
                        .strip_prefix(&expected)
                        .map(|s| s.trim_start_matches('\n'))
                        .unwrap_or(body_trimmed);
                    let _new_content =
                        format!("{}---\n\n{}", &content[..fm_end.unwrap() - 5 + 4], new_body);
                    // Make sure closing --- is correct
                    let rebuilt = rebuild_content(&content, new_body, body_start);
                    std::fs::write(path, rebuilt)?;
                    true
                } else {
                    false
                };
                issues.push(CleaningIssue {
                    file: file_name.clone(),
                    issue_type: "duplicate_title".into(),
                    description: format!("Body starts with duplicate title heading: {title_val}"),
                    fixed,
                });
            }
        }
    }

    // Detect missing blank line after frontmatter close
    let re_no_blank = Regex::new(r"\n---\n[^\n]").unwrap();
    if re_no_blank.is_match(&content) {
        let fixed = if !dry_run {
            let fixed_content = re_no_blank.replace(&content, "\n---\n\n").to_string();
            std::fs::write(path, &fixed_content)?;
            true
        } else {
            false
        };
        issues.push(CleaningIssue {
            file: file_name.clone(),
            issue_type: "missing_blank_line".into(),
            description: "No blank line between frontmatter close and body".into(),
            fixed,
        });
    }

    // Detect truncated / incomplete frontmatter title.
    if let Some(title_val) = title {
        if is_title_truncated(title_val) {
            issues.push(CleaningIssue {
                file: file_name.clone(),
                issue_type: "truncated_title".into(),
                description: format!("Frontmatter title appears truncated: {title_val}"),
                fixed: false,
            });
        }
    }

    // Detect first body paragraph starting with a lowercase letter.
    if let Some((_, body)) = parse_frontmatter(&content) {
        let first_non_empty = body.lines().map(str::trim).find(|l| !l.is_empty());
        if let Some(first_line) = first_non_empty {
            // Strip leading `# ` in case the body starts with an explicit H1.
            let prose_start = first_line.strip_prefix("# ").unwrap_or(first_line);
            if let Some(first_char) = prose_start.chars().next() {
                if first_char.is_lowercase() {
                    issues.push(CleaningIssue {
                        file: file_name,
                        issue_type: "lowercase_intro".into(),
                        description: format!(
                            "First body paragraph starts with lowercase letter: {prose_start}"
                        ),
                        fixed: false,
                    });
                }
            }
        }
    }

    Ok(issues)
}

fn rebuild_content(original: &str, new_body: &str, body_start: usize) -> String {
    let header = &original[..body_start];
    format!("{header}\n{new_body}")
}

/// Replace a frontmatter field value. Returns new frontmatter string.
/// Handles quoted and unquoted values. Preserves field order.
/// If field doesn't exist, inserts it after "title" if present.
/// For `description`, also removes `metaDescription:` and `meta_description:` aliases.
#[allow(dead_code)]
pub fn replace_frontmatter_field(fm: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = fm.lines().map(|s| s.to_string()).collect();
    let mut found = false;
    let mut title_idx = None;

    for (i, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("title:") {
            title_idx = Some(i);
        }
        // For description, also strip metaDescription / meta_description aliases
        if key == "description" {
            if trimmed.starts_with("metaDescription:") || trimmed.starts_with("meta_description:") {
                *line = String::new(); // mark for removal
                continue;
            }
        }
        let prefix = format!("{key}:");
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let old_val = rest.trim();
            let needs_quotes = old_val.starts_with('"') || old_val.starts_with('\'');
            let new_val = if needs_quotes {
                format!("\"{}\"", value.replace('"', "\\\""))
            } else {
                value.to_string()
            };
            // Preserve leading whitespace
            let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            *line = format!("{leading_ws}{key}: {new_val}");
            found = true;
        }
    }

    // Remove blanked-out alias lines
    lines.retain(|l| !l.is_empty());

    if !found {
        let insert_idx = title_idx.map(|i| i + 1).unwrap_or(0);
        let line = format!("{key}: \"{}\"", value.replace('"', "\\\""));
        lines.insert(insert_idx, line);
    }

    lines.join("\n")
}

/// Check whether a line is an MDX import statement or JSX component (not prose).
fn is_mdx_non_prose_line(trimmed: &str) -> bool {
    trimmed.starts_with("import ")
        || trimmed.starts_with("export ")
        || (trimmed.starts_with('<') && trimmed.ends_with('>'))
        || (trimmed.starts_with('<') && trimmed.ends_with("/>"))
}

/// Find byte range of first paragraph in MDX body (after H1, before first blank line or heading).
/// Skips MDX import/export statements and JSX components.
pub fn find_first_paragraph_range(body: &str) -> Option<(usize, usize)> {
    let mut in_h1 = false;
    let mut lines = body.lines().peekable();
    let mut byte_offset = 0usize;

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if !in_h1 && trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
            in_h1 = true;
            byte_offset += line.len() + 1; // +1 for newline
            continue;
        }
        if in_h1 {
            if trimmed.is_empty() {
                byte_offset += line.len() + 1;
                continue;
            }
            if trimmed.starts_with('#') {
                break;
            }
            if is_mdx_non_prose_line(trimmed) {
                byte_offset += line.len() + 1;
                continue;
            }
        } else if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("---")
            || is_mdx_non_prose_line(trimmed)
        {
            byte_offset += line.len() + 1;
            continue;
        }
        // Found first paragraph start (after H1, or at body start if no H1)
        let paragraph_start = byte_offset;
        // Find end of contiguous paragraph (no blank lines, no headings)
        let mut end_offset = byte_offset + line.len();
        while let Some(next) = lines.peek() {
            let next_trimmed = next.trim();
            if next_trimmed.is_empty()
                || next_trimmed.starts_with('#')
                || next_trimmed.starts_with("---")
                || is_mdx_non_prose_line(next_trimmed)
            {
                break;
            }
            end_offset += 1 + next.len(); // +1 for newline before
            lines.next();
        }
        return Some((paragraph_start, end_offset));
    }
    None
}

/// Ensure the body has a first paragraph containing `new_text`.
///
/// If a first paragraph already exists (after the H1 or at the top of the body),
/// it is replaced. If no paragraph exists — for example, the H1 is immediately
/// followed by an H2 heading — the new text is inserted after the first H1.
/// If there is no H1, the text is inserted at the start of the body.
pub fn ensure_first_paragraph(body: &str, new_text: &str) -> String {
    match find_first_paragraph_range(body) {
        Some((start, end)) => {
            let before = &body[..start];
            let after = &body[end..];
            format!("{before}{new_text}{after}")
        }
        None => insert_after_first_h1(body, new_text),
    }
}

/// Insert `new_text` after the first H1 line, or at the top of the body if no H1 exists.
fn insert_after_first_h1(body: &str, new_text: &str) -> String {
    let h1_pattern = Regex::new(r"(?m)^# .+$").unwrap();
    if let Some(m) = h1_pattern.find(body) {
        let insert_pos = m.end();
        let before = &body[..insert_pos];
        let after = &body[insert_pos..];
        return format!("{before}\n\n{new_text}{after}");
    }

    // No H1: prepend the paragraph to the body.
    if body.trim().is_empty() {
        new_text.to_string()
    } else {
        format!("{new_text}\n\n{body}")
    }
}

/// Replace first paragraph with new text.
///
/// Deprecated: this is a thin wrapper around `ensure_first_paragraph` so that
/// callers get insertion behavior when no paragraph exists. Prefer calling
/// `ensure_first_paragraph` directly for clarity.
pub fn replace_first_paragraph(body: &str, new_text: &str) -> String {
    ensure_first_paragraph(body, new_text)
}

/// Insert JSON-LD FAQPage schema before last `---` separator or at end of body.
/// If an FAQPage schema already exists, replaces it rather than appending a duplicate.
#[allow(dead_code)] // Kept for potential Phase 3 inline JSON-LD cleanup/migration
pub fn insert_faq_schema(body: &str, questions: &[(String, String)]) -> String {
    let mut entity_parts = Vec::new();
    for (i, (q, a)) in questions.iter().enumerate() {
        let comma = if i < questions.len() - 1 { "," } else { "" };
        let q_json = serde_json::to_string(q).unwrap();
        let a_json = serde_json::to_string(a).unwrap();
        entity_parts.push(format!(
            "    {{\n      \"@type\": \"Question\",\n      \"name\": {q_json},\n      \"acceptedAnswer\": {{\n        \"@type\": \"Answer\",\n        \"text\": {a_json}\n      }}\n    }}{comma}"
        ));
    }
    let entities = entity_parts.join("\n");

    let schema = format!(
        "<script type=\"application/ld+json\">\n{{\n  \"@context\": \"https://schema.org\",\n  \"@type\": \"FAQPage\",\n  \"mainEntity\": [\n{entities}\n  ]\n}}\n</script>"
    );

    // If an existing FAQPage schema is present, replace it
    let body_lower = body.to_lowercase();
    if let Some(start) = body_lower.find("<script type=\"application/ld+json\">") {
        if let Some(end) = body_lower[start..].find("</script>") {
            let end_abs = start + end + "</script>".len();
            // Verify it's actually an FAQPage schema
            let block = &body[start..end_abs];
            if block.to_lowercase().contains("faqpage") {
                let before = &body[..start];
                let after = &body[end_abs..];
                return format!("{before}{schema}{after}");
            }
        }
    }

    // No existing FAQ schema: insert before last `---` separator (end of article marker)
    if let Some(pos) = body.rfind("\n---\n") {
        let before = &body[..pos];
        let after = &body[pos..];
        format!("{before}\n\n{schema}\n{after}")
    } else {
        format!("{body}\n\n{schema}\n")
    }
}

/// Insert a snippet section (H2 + answer + optional list/table) near the top of the body.
/// Places the section after the first H1 if present, otherwise after the first paragraph.
pub fn insert_snippet_section(
    body: &str,
    heading: &str,
    answer: &str,
    list: Option<&[String]>,
    table: Option<&[Vec<String>]>,
) -> String {
    let mut section = format!("\n\n## {}\n\n{}", heading, answer);

    if let Some(rows) = table {
        if rows.len() >= 2 {
            // Markdown table with header row
            let mut table_md = String::new();
            for (i, row) in rows.iter().enumerate() {
                let cells = row.join(" | ");
                table_md.push_str(&format!("| {} |\n", cells));
                if i == 0 {
                    let separators: Vec<&str> = row.iter().map(|_| "---").collect();
                    table_md.push_str(&format!("| {} |\n", separators.join(" | ")));
                }
            }
            section.push_str("\n");
            section.push_str(&table_md);
        }
    }

    if let Some(items) = list {
        if !items.is_empty() {
            section.push_str("\n");
            for (i, item) in items.iter().enumerate() {
                section.push_str(&format!("{}. {}\n", i + 1, item));
            }
        }
    }

    // Find insertion point: after first H1, or after first paragraph
    let h1_pattern = regex::Regex::new(r"(?m)^# .+$").unwrap();
    if let Some(m) = h1_pattern.find(body) {
        let insert_pos = m.end();
        let before = &body[..insert_pos];
        let after = &body[insert_pos..];
        return format!("{}{}{}", before, section, after);
    }

    // Fallback: after first paragraph
    match find_first_paragraph_range(body) {
        Some((_, end)) => {
            let before = &body[..end];
            let after = &body[end..];
            format!("{}{}{}", before, section, after)
        }
        None => format!("{}{}", body, section),
    }
}

/// Reconstruct MDX file from frontmatter and body.
pub fn rebuild_mdx(fm: &str, body: &str) -> String {
    format!("---\n{fm}\n---\n{body}")
}

/// Validate MDX structure after edits. Returns Ok(()) or descriptive error.
/// Checks:
/// - Starts with ---\n
/// - Has closing \n---\n
/// - Body is not empty
/// - Frontmatter length is NOT checked (removed — large inline FAQ YAML is legitimate)
pub fn validate_mdx_structure(content: &str) -> std::result::Result<(), String> {
    if !content.starts_with("---\n") {
        return Err("MDX does not start with frontmatter delimiter '---\\n'".to_string());
    }
    let after_open = &content[4..];
    if !after_open.contains("\n---\n") {
        return Err("MDX frontmatter not properly closed with '\\n---\\n'".to_string());
    }
    // Body must not be empty
    let body_start = after_open.find("\n---\n").unwrap() + 5;
    let body = &after_open[body_start..].trim();
    if body.is_empty() {
        return Err("MDX body is empty after frontmatter".to_string());
    }
    Ok(())
}

/// Scan all MDX files in `content_dir` for issues. Apply fixes unless `dry_run`.
pub fn scan_and_clean(content_dir: &Path, dry_run: bool) -> Result<CleaningResult> {
    let files = crate::content::locator::collect_markdown_files(content_dir);
    let files_checked = files.len();
    let mut all_issues = Vec::new();

    for path in &files {
        let mut file_issues = check_file(path, dry_run)?;
        all_issues.append(&mut file_issues);
    }

    let issues_fixed = all_issues.iter().filter(|i| i.fixed).count();

    Ok(CleaningResult {
        files_checked,
        issues: all_issues,
        issues_fixed,
    })
}

/// Fix malformed frontmatter closers where `---` is appended to the last field line
/// instead of being on its own line.
///
/// Pattern: `...field: "value"---\n` → `...field: "value"\n---\n`
///
/// Returns the list of files that were fixed.
pub fn fix_malformed_frontmatter_closers(
    content_dir: &Path,
) -> std::result::Result<Vec<PathBuf>, String> {
    let files = crate::content::locator::collect_markdown_files(content_dir);
    let mut fixed = Vec::new();

    for path in files {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[sanitize] Could not read {}: {}", path.display(), e);
                continue;
            }
        };

        // Only touch files that start with frontmatter
        if !content.starts_with("---\n") {
            continue;
        }

        // Check if the standard closing delimiter exists
        let after_open = &content[4..];
        if after_open.contains("\n---\n") {
            continue; // already well-formed
        }

        // Look for the malformed pattern: a line ending with text followed immediately by ---\n
        // We search for the first occurrence of `"---\n` or `'---\n` after the opening
        let mut rewritten = content.clone();
        let mut made_change = false;

        for pattern in ["\"---\n", "'---\n"] {
            if let Some(pos) = rewritten.find(pattern) {
                // Replace "---\n with "\n---\n (or '---\n with '\n---\n)
                let quote = &pattern[..1];
                rewritten.replace_range(pos..pos + 4, &format!("{}\n---", quote));
                made_change = true;
                break; // only fix the first occurrence (the closing delimiter)
            }
        }

        if made_change {
            if let Err(e) = std::fs::write(&path, &rewritten) {
                log::warn!("[sanitize] Could not write {}: {}", path.display(), e);
                continue;
            }
            fixed.push(path);
        }
    }

    Ok(fixed)
}

/// Rename all `.md` files in `content_dir` to `.mdx`.
///
/// Returns a list of `(old_path, new_path)` for each file renamed.
/// Skips files where the `.mdx` counterpart already exists.
pub fn rename_md_to_mdx(
    content_dir: &Path,
) -> std::result::Result<Vec<(PathBuf, PathBuf)>, String> {
    let files = crate::content::locator::collect_markdown_files(content_dir);
    let mut renamed = Vec::new();

    for path in files {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("md") && !ext.eq_ignore_ascii_case("mdx") {
                let stem = path.file_stem().unwrap_or_default();
                let new_name = format!("{}.mdx", stem.to_string_lossy());
                let new_path = path.with_file_name(&new_name);

                if new_path.exists() {
                    log::warn!(
                        "[sanitize] Skipping rename: {} already exists",
                        new_path.display()
                    );
                    continue;
                }

                std::fs::rename(&path, &new_path).map_err(|e| {
                    format!(
                        "Failed to rename {} to {}: {}",
                        path.display(),
                        new_path.display(),
                        e
                    )
                })?;

                renamed.push((path, new_path));
            }
        }
    }

    Ok(renamed)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_frontmatter_field_basic() {
        let fm = r#"title: "Old Title"
description: "Old desc"
date: "2024-01-01""#;
        let result = replace_frontmatter_field(fm, "description", "New description here");
        assert!(result.contains("description: \"New description here\""));
        assert!(result.contains("title: \"Old Title\""));
        assert!(result.contains("date: \"2024-01-01\""));
    }

    #[test]
    fn replace_frontmatter_field_insert() {
        let fm = r#"title: "Old Title"
date: "2024-01-01""#;
        let result = replace_frontmatter_field(fm, "description", "Inserted desc");
        assert!(result.contains("description: \"Inserted desc\""));
        // Should be inserted after title
        let title_pos = result.find("title:").unwrap();
        let desc_pos = result.find("description:").unwrap();
        assert!(desc_pos > title_pos);
    }

    #[test]
    fn replace_frontmatter_field_canonicalizes_meta() {
        let fm = r#"title: "Old Title"
description: "Old desc"
metaDescription: "Should be removed"
date: "2024-01-01""#;
        let result = replace_frontmatter_field(fm, "description", "New desc");
        assert!(result.contains("description: \"New desc\""));
        assert!(!result.contains("metaDescription"));
    }

    #[test]
    fn replace_first_paragraph_basic() {
        let body = r#"# Heading

First paragraph here.

Second paragraph."#;
        let result = replace_first_paragraph(body, "Replaced first paragraph.");
        assert!(result.contains("Replaced first paragraph."));
        assert!(!result.contains("First paragraph here."));
        assert!(result.contains("Second paragraph."));
    }

    #[test]
    fn ensure_first_paragraph_inserts_after_h1_when_missing() {
        // Regression case: H1 is immediately followed by H2 with no intro paragraph.
        let body = "# SPX Options Tax Treatment\n## SPX Options 60/40 Tax Treatment Explained\n\nBody content.";
        let result = ensure_first_paragraph(body, "SPX options 60/40 tax treatment gives index traders a major edge.");
        assert!(
            result.contains("# SPX Options Tax Treatment\n\nSPX options 60/40 tax treatment gives index traders a major edge."),
            "intro should be inserted between H1 and H2, got:\n{}",
            result
        );
        assert!(result.contains("## SPX Options 60/40 Tax Treatment Explained"));
        assert!(result.contains("Body content."));
    }

    #[test]
    fn ensure_first_paragraph_replaces_existing_paragraph() {
        let body = "# Heading\n\nOld paragraph.\n\n## Next\nBody.";
        let result = ensure_first_paragraph(body, "New paragraph text.");
        assert!(result.contains("New paragraph text."));
        assert!(!result.contains("Old paragraph."));
        assert!(result.contains("## Next"));
    }

    #[test]
    fn ensure_first_paragraph_inserts_at_top_when_no_h1_and_no_paragraph() {
        // No H1 and no prose paragraph exists (only headings / separators).
        let body = "## First H2\n\n---\n";
        let result = ensure_first_paragraph(body, "Intro paragraph.");
        assert!(result.starts_with("Intro paragraph.\n\n## First H2"));
        assert!(result.contains("---"));
    }

    #[test]
    fn insert_faq_schema_basic() {
        let body = "# Article\n\nSome content.\n\n---\n";
        let questions = vec![
            ("What is X?".to_string(), "X is a test.".to_string()),
            ("How does Y work?".to_string(), "Y works well.".to_string()),
        ];
        let result = insert_faq_schema(body, &questions);
        assert!(result.contains("FAQPage"));
        assert!(result.contains("What is X?"));
        assert!(result.contains("Y works well."));
        // Should be before the closing ---
        let faq_pos = result.find("FAQPage").unwrap();
        let close_pos = result.rfind("---").unwrap();
        assert!(faq_pos < close_pos);
    }

    #[test]
    fn validate_mdx_structure_pass() {
        // Large frontmatter (simulating 23K inline FAQ YAML) should still pass
        let large_fm = "a: x\n".repeat(5000);
        let content = format!("---\n{}---\n\nBody here.\n", large_fm);
        assert!(validate_mdx_structure(&content).is_ok());
    }

    #[test]
    fn validate_mdx_structure_missing_close() {
        let content = "---\ntitle: test\n\nBody here.\n";
        assert!(validate_mdx_structure(content).is_err());
    }

    #[test]
    fn is_title_truncated_detects_dangling_words_and_punct() {
        assert!(is_title_truncated("A Beginner's Guide to"));
        assert!(is_title_truncated("What Are the Greeks in Options: "));
        assert!(!is_title_truncated("Cash Secured Puts: The Complete Income"));
        assert!(!is_title_truncated("Cash Secured Puts: The Complete Income Guide"));
        assert!(!is_title_truncated("Options Greeks Cheat Sheet (2026)"));
    }

    #[test]
    fn check_file_flags_truncated_title_and_lowercase_intro() {
        let dir = std::env::temp_dir().join(format!(
            "ps_cleaner_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.mdx");
        std::fs::write(
            &path,
            "---\ntitle: \"A Beginner's Guide to\"\n---\n\nash secured puts are great.\n",
        )
        .unwrap();

        let issues = check_file(&path, true).unwrap();
        let types: Vec<_> = issues.iter().map(|i| i.issue_type.as_str()).collect();
        assert!(types.contains(&"truncated_title"), "issues: {:?}", types);
        assert!(types.contains(&"lowercase_intro"), "issues: {:?}", types);

        // cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
