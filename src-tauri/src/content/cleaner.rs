/// MDX content cleaner.
///
/// Mirrors `packages/seo-content-cli/src/seo_content_mcp/content_cleaner.py`.
use std::path::Path;

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

/// Parse YAML frontmatter from MDX content.
/// Returns (frontmatter_str, body_str) or None if no frontmatter found.
pub fn parse_frontmatter(content: &str) -> Option<(&str, &str)> {
    if !content.starts_with("---") {
        return None;
    }
    // Find closing ---
    let after_open = &content[3..];
    // Skip the newline after opening ---
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    let close = after_open.find("\n---")?;
    let fm = &after_open[..close];
    let body_start = close + 4; // skip \n---
    let body = &after_open[body_start..].strip_prefix('\n').unwrap_or(&after_open[body_start..]);
    Some((fm, body))
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

/// Scan a single MDX file for cleaning issues. Optionally apply fixes.
///
/// Issues detected:
/// - missing_frontmatter: no `---` block
/// - duplicate_title: body starts with `# <title>` matching the frontmatter title
/// - blank_line_after_frontmatter: no blank line between closing `---` and first body line
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
                    let _new_content = format!("{}---\n\n{}", &content[..fm_end.unwrap() - 5 + 4], new_body);
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
            file: file_name,
            issue_type: "missing_blank_line".into(),
            description: "No blank line between frontmatter close and body".into(),
            fixed,
        });
    }

    Ok(issues)
}

fn rebuild_content(original: &str, new_body: &str, body_start: usize) -> String {
    let header = &original[..body_start];
    format!("{header}\n{new_body}")
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
