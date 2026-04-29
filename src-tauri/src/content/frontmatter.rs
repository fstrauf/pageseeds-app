/// Canonical frontmatter parsing, validation, and safe editing.
///
/// Design principles (from the sanitizer analysis):
/// - Parse YAML semantically for understanding and validation.
/// - Use raw text surgery only for narrow top-level scalar edits.
/// - Never regenerate the whole frontmatter unless it is missing entirely.
/// - Skip complex YAML (lists, nested objects, comments) during auto-fix.
use serde_yaml::Value;

// ─── Types ───────────────────────────────────────────────────────────────────

/// A top-level scalar field extracted from frontmatter raw text.
#[derive(Debug, Clone)]
pub struct TopLevelField<'a> {
    /// The key as it appears in the source (e.g. "metaDescription").
    pub key: &'a str,
    /// The raw value text after the colon (may include quotes).
    pub raw_value: &'a str,
    /// Zero-based line index within the frontmatter text.
    pub line_idx: usize,
}

/// Parsed frontmatter with both raw text and semantic understanding.
#[derive(Debug)]
pub struct Frontmatter<'a> {
    pub _raw: &'a str,
    pub parsed: Value,
}

// ─── MDX Split ───────────────────────────────────────────────────────────────

/// Split MDX content into frontmatter and body.
///
/// Looks for `---\n` at the start and `\n---\n` as the closing delimiter.
/// Returns `None` if the standard delimiter is not found.
pub fn split_mdx(content: &str) -> Option<(&str, &str)> {
    if !content.starts_with("---\n") {
        return None;
    }
    let after_open = &content[4..];
    let close = after_open.find("\n---\n")?;
    let fm = &after_open[..close];
    let body_start = close + 5; // skip \n---\n
    let body = after_open[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&after_open[body_start..]);
    Some((fm, body))
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

/// Parse raw frontmatter text into a structured representation.
///
/// The raw text is preserved for surgical edits.  The YAML parse is used
/// for structural understanding (complex vs simple, field presence, etc.).
pub fn parse(raw: &str) -> Result<Frontmatter, String> {
    let parsed: Value = serde_yaml::from_str(raw)
        .map_err(|e| format!("Failed to parse frontmatter as YAML: {}", e))?;
    Ok(Frontmatter { _raw: raw, parsed })
}

// ─── Top-level scalar extraction ─────────────────────────────────────────────

/// Extract top-level scalar fields from frontmatter raw text.
///
/// Skips:
/// - comment lines (`# ...`)
/// - indented lines (nested YAML)
/// - list items (`- ...`)
/// - lines without a colon (not key-value)
///
/// This is intentionally conservative.  If frontmatter is complex
/// (contains lists, nested objects, or comments), callers should
/// inspect `is_complex` and avoid auto-fixing.
pub fn top_level_scalars(raw: &str) -> Vec<TopLevelField> {
    let mut fields = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('-') {
            continue;
        }
        // Must not be indented — top-level only
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some((key, val)) = split_field_line(trimmed) {
            fields.push(TopLevelField {
                key,
                raw_value: val,
                line_idx: idx,
            });
        }
    }
    fields
}

// ─── Complexity detection ────────────────────────────────────────────────────

/// Returns true if the frontmatter contains YAML structures that the
/// line-oriented fixer cannot safely edit:
/// - sequences (YAML lists)
/// - nested mappings
/// - comment lines in the raw text
/// - non-string scalar types (numbers, booleans, nulls are OK for top-level)
///
/// When this returns true, auto-fix should be skipped for the file.
pub fn is_complex(raw: &str, parsed: &Value) -> bool {
    // 1. Raw text contains comment lines
    if raw.lines().any(|l| l.trim_start().starts_with('#')) {
        return true;
    }

    // 2. Parsed YAML is not a flat mapping of scalars
    let Some(mapping) = parsed.as_mapping() else {
        // Not even a mapping — definitely complex
        return true;
    };

    for (_key, value) in mapping {
        match value {
            Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {
                // OK — simple scalar
            }
            _ => {
                // Sequence, Mapping, TaggedValue — complex
                return true;
            }
        }
    }

    false
}

// ─── Safe scalar update ──────────────────────────────────────────────────────

/// Update a top-level scalar field in raw frontmatter text.
///
/// Returns `Some(new_frontmatter_text)` if the field was found and updated,
/// or `None` if the field was not found.
///
/// Preserves:
/// - existing quoting style (adds quotes if the old value was quoted)
/// - line order
/// - all other lines (including comments, lists, nested objects)
///
/// Does NOT touch the field if the line is indented, a list item, or a comment.
#[allow(dead_code)]
pub fn update_scalar(raw_fm: &str, key: &str, new_value: &str) -> Option<String> {
    let mut lines: Vec<String> = raw_fm.lines().map(|s| s.to_string()).collect();
    let mut found = false;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some((k, old_val)) = split_field_line(trimmed) {
            if k == key {
                let needs_quotes = old_val.starts_with('"') || old_val.starts_with('\'');
                let new_val = if needs_quotes {
                    format!("\"{}\"", new_value.replace('"', "\\\""))
                } else {
                    new_value.to_string()
                };
                // Preserve leading whitespace on the original line
                let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                lines[idx] = format!("{}{}: {}", leading_ws, key, new_val);
                found = true;
                break;
            }
        }
    }

    if found {
        Some(lines.join("\n"))
    } else {
        None
    }
}

/// Replace or insert a YAML `faq:` block in raw frontmatter text.
///
/// Behaviour:
/// - Finds the existing `faq:` block (and any immediately preceding comment line)
///   and replaces it with the new block.
/// - If `faq:` does not exist, inserts it after the `title` line (or at the top).
/// - Each question/answer is YAML-quoted and indented as a standard list.
///
/// Returns the updated frontmatter text.
pub fn replace_faq_block(raw_fm: &str, questions: &[(String, String)]) -> String {
    let mut lines: Vec<String> = raw_fm.lines().map(|s| s.to_string()).collect();
    let mut faq_start = None;
    let mut title_idx = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();

        // Track title position for insertion fallback
        if let Some((k, _)) = split_field_line(trimmed) {
            if k == "title" {
                title_idx = Some(i);
            }
        }

        // Find the faq: line
        if trimmed == "faq:" || trimmed.starts_with("faq: ") {
            faq_start = Some(i);
            break;
        }
    }

    let mut new_block = vec!["faq:".to_string()];
    for (q, a) in questions {
        let q_escaped = q.replace('"', "\\\"");
        let a_escaped = a.replace('"', "\\\"");
        new_block.push(format!("  - question: \"{}\"", q_escaped));
        new_block.push(format!("    answer: \"{}\"", a_escaped));
    }

    if let Some(start) = faq_start {
        // Find end of the existing faq block
        let mut end = start + 1;
        while end < lines.len() {
            let line = &lines[end];
            // If line is empty or a comment, keep scanning (it belongs to the block)
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                end += 1;
                continue;
            }
            // If line is indented, it's part of the block
            if line.starts_with(' ') || line.starts_with('\t') {
                end += 1;
                continue;
            }
            // Otherwise it's a new top-level key — block ends before this line
            break;
        }

        // Check if there's a comment line immediately before faq: — keep it
        let actual_start = if start > 0 {
            let prev = lines[start - 1].trim_start();
            if prev.starts_with('#') {
                start - 1
            } else {
                start
            }
        } else {
            start
        };

        // Replace [actual_start, end) with new block
        let mut replacement = Vec::new();
        if actual_start < start {
            replacement.push(lines[actual_start].clone());
        }
        replacement.extend(new_block);
        lines.splice(actual_start..end, replacement);
    } else {
        // Insert after title (or at top)
        let insert_idx = title_idx.map(|i| i + 1).unwrap_or(0);
        lines.insert(insert_idx, String::new());
        for line in new_block.into_iter().rev() {
            lines.insert(insert_idx + 1, line);
        }
    }

    lines.join("\n")
}

/// Replace a top-level scalar field, with alias handling and insertion fallback.
///
/// This is the safe replacement for `cleaner::replace_frontmatter_field`.
///
/// Behaviour:
/// - Only matches top-level scalar keys (skips indented, list items, comments).
/// - For `description`, also removes `metaDescription` and `meta_description` aliases.
/// - Preserves existing quoting style.
/// - If the key does not exist, inserts it after the `title` line (or at the top).
///
/// Returns the updated frontmatter text.
pub fn replace_scalar(raw_fm: &str, key: &str, new_value: &str) -> String {
    let mut lines: Vec<String> = raw_fm.lines().map(|s| s.to_string()).collect();
    let mut found = false;
    let mut title_idx = None;

    for (i, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }

        // Track title position for insertion fallback
        if let Some((k, _)) = split_field_line(trimmed) {
            if k == "title" {
                title_idx = Some(i);
            }
        }

        // Remove description aliases when updating description
        if key == "description" {
            if trimmed.starts_with("metaDescription:") || trimmed.starts_with("meta_description:") {
                *line = String::new(); // mark for removal
                continue;
            }
        }

        if let Some((k, old_val)) = split_field_line(trimmed) {
            if k == key {
                let needs_quotes = old_val.starts_with('"') || old_val.starts_with('\'');
                let new_val = if needs_quotes {
                    format!("\"{}\"", new_value.replace('"', "\\\""))
                } else {
                    new_value.to_string()
                };
                let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                *line = format!("{}{}: {}", leading_ws, key, new_val);
                found = true;
            }
        }
    }

    // Remove blanked-out alias lines
    lines.retain(|l| !l.is_empty());

    if !found {
        let insert_idx = title_idx.map(|i| i + 1).unwrap_or(0);
        let line = format!("{}: \"{}\"", key, new_value.replace('"', "\\\""));
        lines.insert(insert_idx, line);
    }

    lines.join("\n")
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Split a frontmatter line like `title: "Foo"` into ("title", "Foo").
///
/// The value is returned raw — quotes are NOT stripped, so callers can
/// inspect whether the original was quoted.
pub fn split_field_line(line: &str) -> Option<(&str, &str)> {
    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim();
    let val = line[colon_pos + 1..].trim();
    if key.is_empty() {
        return None;
    }
    Some((key, val))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mdx_standard() {
        let content = "---\ntitle: Test\n---\n\nBody here.\n";
        let (fm, body) = split_mdx(content).unwrap();
        assert_eq!(fm, "title: Test");
        assert_eq!(body, "Body here.\n");
    }

    #[test]
    fn split_mdx_no_frontmatter() {
        assert!(split_mdx("# Just a heading\n\nBody.").is_none());
    }

    #[test]
    fn parse_simple_mapping() {
        let raw = "title: \"Hello\"\ndescription: World\n";
        let fm = parse(raw).unwrap();
        assert_eq!(fm.parsed["title"].as_str(), Some("Hello"));
        assert_eq!(fm.parsed["description"].as_str(), Some("World"));
    }

    #[test]
    fn top_level_scalars_skips_comments_and_lists() {
        let raw = r#"title: "A"
# AI SEO: FAQ Schema
faq:
  - question: "Q1?"
    answer: "A1"
description: "B"
"#;
        let fields = top_level_scalars(raw);
        let keys: Vec<_> = fields.iter().map(|f| f.key).collect();
        // `faq` itself is a top-level key (with empty raw value), but its children are skipped
        assert_eq!(keys, vec!["title", "faq", "description"]);
    }

    #[test]
    fn is_complex_detects_comments() {
        let raw = "title: A\n# comment\n";
        let parsed = parse(raw).unwrap();
        assert!(is_complex(raw, &parsed.parsed));
    }

    #[test]
    fn is_complex_detects_lists() {
        let raw = "title: A\ntags:\n  - one\n  - two\n";
        let parsed = parse(raw).unwrap();
        assert!(is_complex(raw, &parsed.parsed));
    }

    #[test]
    fn is_complex_flat_scalars_ok() {
        let raw = "title: A\ndescription: B\ndate: 2024-01-01\n";
        let parsed = parse(raw).unwrap();
        assert!(!is_complex(raw, &parsed.parsed));
    }

    #[test]
    fn update_scalar_preserves_quotes() {
        let raw = r#"title: "Old Title"
description: Old desc"#;
        let updated = update_scalar(raw, "title", "New Title").unwrap();
        assert!(updated.contains("title: \"New Title\""));
        assert!(updated.contains("description: Old desc"));
    }

    #[test]
    fn update_scalar_adds_quotes_when_unquoted() {
        let raw = "title: Old\ndescription: Old desc";
        let updated = update_scalar(raw, "description", "New desc").unwrap();
        assert!(updated.contains("description: New desc"));
    }

    #[test]
    fn update_scalar_not_found() {
        let raw = "title: A";
        assert!(update_scalar(raw, "missing", "X").is_none());
    }

    #[test]
    fn update_scalar_skips_indented_lines() {
        let raw = "faq:\n  - question: \"Q?\"\n    answer: \"A\"\n";
        assert!(update_scalar(raw, "answer", "New").is_none());
    }

    #[test]
    fn replace_scalar_updates_existing_field() {
        let raw = "title: \"Old\"\ndescription: Old desc\n";
        let updated = replace_scalar(raw, "title", "New Title");
        assert!(updated.contains("title: \"New Title\""));
        assert!(updated.contains("description: Old desc"));
    }

    #[test]
    fn replace_scalar_inserts_after_title() {
        let raw = "title: \"Hello\"\n";
        let updated = replace_scalar(raw, "description", "New desc");
        assert!(updated.contains("title: \"Hello\""));
        assert!(updated.contains("description: \"New desc\""));
        // description should come after title
        let title_pos = updated.find("title:").unwrap();
        let desc_pos = updated.find("description:").unwrap();
        assert!(desc_pos > title_pos);
    }

    #[test]
    fn replace_scalar_removes_description_aliases() {
        let raw = "title: \"Hello\"\nmetaDescription: \"Old alias\"\ndescription: \"Old desc\"\n";
        let updated = replace_scalar(raw, "description", "New desc");
        assert!(!updated.contains("metaDescription:"));
        assert!(updated.contains("description: \"New desc\""));
    }

    #[test]
    fn replace_scalar_preserves_complex_yaml() {
        let raw = r#"title: "Hello"
date: "2024-01-01"
description: "Old"
# AI SEO: FAQ Schema
faq:
  - question: "Q1?"
    answer: "A1"
  - question: "Q2?"
    answer: "A2"
citations:
  - source: "S1"
    url: "http://example.com"
"#;
        let updated = replace_scalar(raw, "description", "New desc");
        // FAQ list preserved
        assert!(updated.contains("faq:"));
        assert!(updated.contains("  - question: \"Q1?\""));
        assert!(updated.contains("  - question: \"Q2?\""));
        // Comment preserved
        assert!(updated.contains("# AI SEO: FAQ Schema"));
        // Citations preserved
        assert!(updated.contains("citations:"));
        assert!(updated.contains("  - source: \"S1\""));
        // Description updated
        assert!(updated.contains("description: \"New desc\""));
    }

    #[test]
    fn replace_scalar_skips_indented_nested_keys() {
        let raw = r#"faq:
  - question: "Q?"
    answer: "A"
title: "Hello"
"#;
        let updated = replace_scalar(raw, "title", "New Title");
        // title should be updated
        assert!(updated.contains("title: \"New Title\""));
        // nested answer should NOT be touched
        assert!(updated.contains("    answer: \"A\""));
    }

    #[test]
    fn replace_faq_block_replaces_existing() {
        let raw = r#"title: "Hello"
# AI SEO: FAQ Schema
faq:
  - question: "Old Q?"
    answer: "Old A"
description: "Desc"
"#;
        let updated = replace_faq_block(raw, &[("New Q?".to_string(), "New A".to_string())]);
        assert!(updated.contains("title: \"Hello\""));
        assert!(updated.contains("description: \"Desc\""));
        assert!(updated.contains("# AI SEO: FAQ Schema"));
        assert!(updated.contains("  - question: \"New Q?\""));
        assert!(updated.contains("    answer: \"New A\""));
        assert!(!updated.contains("Old Q?"));
    }

    #[test]
    fn replace_faq_block_inserts_after_title() {
        let raw = r#"title: "Hello"
description: "Desc"
"#;
        let updated = replace_faq_block(raw, &[("Q1?".to_string(), "A1".to_string())]);
        assert!(updated.contains("title: \"Hello\""));
        assert!(updated.contains("description: \"Desc\""));
        assert!(updated.contains("faq:"));
        assert!(updated.contains("  - question: \"Q1?\""));
        // faq should come after title
        let title_pos = updated.find("title:").unwrap();
        let faq_pos = updated.find("faq:").unwrap();
        assert!(faq_pos > title_pos);
    }

    #[test]
    fn replace_faq_block_escapes_quotes() {
        let updated = replace_faq_block(
            "title: \"Hello\"\n",
            &[(
                "Q with \"quotes\"?".to_string(),
                "A with \"quotes\"".to_string(),
            )],
        );
        assert!(updated.contains("  - question: \"Q with \\\"quotes\\\"?\""));
        assert!(updated.contains("    answer: \"A with \\\"quotes\\\"\""));
    }

    #[test]
    fn replace_faq_block_multiple_questions() {
        let raw = "title: \"Hello\"\n";
        let updated = replace_faq_block(
            raw,
            &[
                ("Q1?".to_string(), "A1".to_string()),
                ("Q2?".to_string(), "A2".to_string()),
            ],
        );
        let q1_pos = updated.find("Q1?").unwrap();
        let q2_pos = updated.find("Q2?").unwrap();
        assert!(q2_pos > q1_pos);
        assert!(updated.contains("  - question: \"Q1?\""));
        assert!(updated.contains("  - question: \"Q2?\""));
    }
}
