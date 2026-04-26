/// Frontmatter format validator & fix engine.
///
/// Provides deterministic validation and cleanup of YAML frontmatter across
/// all MDX files in a project.  This is the canonical replacement for the
/// legacy `pageseeds content clean` subprocess and the unimplemented
/// `sanitize_article_frontmatter` command.
///
/// Usage:
///   let result = validate_project(repo_root, content_dir, None)?;
///   let fixed  = apply_fixes(&result.issues, repo_root)?;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

// ─── Schema ───────────────────────────────────────────────────────────────────

/// Per-project frontmatter schema, usually loaded from `seo_workspace.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FrontmatterSchema {
    /// Fields that must be present and non-empty.
    #[serde(default = "default_required")]
    pub required: Vec<String>,
    /// Fields that are allowed but not required.
    #[serde(default = "default_optional")]
    pub optional: Vec<String>,
}

fn default_required() -> Vec<String> {
    vec!["title".into(), "date".into(), "description".into()]
}

fn default_optional() -> Vec<String> {
    vec!["status".into()]
}

impl FrontmatterSchema {
    /// Canonical field name for a given key (resolves aliases).
    pub fn canonical(&self, key: &str) -> String {
        let lower = key.to_lowercase();
        // Built-in aliases
        let alias_map: HashMap<&str, &str> = [
            ("publisheddate", "date"),
            ("published_date", "date"),
            ("metadescription", "description"),
            ("meta_description", "description"),
        ]
        .into_iter()
        .collect();
        alias_map.get(lower.as_str()).copied().unwrap_or_else(|| lower.as_str()).to_string()
    }

    /// All known fields (required + optional).
    pub fn known_fields(&self) -> HashSet<String> {
        self.required
            .iter()
            .chain(self.optional.iter())
            .cloned()
            .collect()
    }

    /// Whether a field is required.
    pub fn is_required(&self, field: &str) -> bool {
        self.required.iter().any(|f| f == field)
    }

    /// Default schema used when no project override is provided.
    pub fn default_schema() -> Self {
        Self {
            required: default_required(),
            optional: default_optional(),
        }
    }
}

// ─── Issue types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

/// One discrete format issue found in a single file.
#[derive(Debug, Clone, Serialize)]
pub struct FormatIssue {
    pub file: String,
    pub issue_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub severity: Severity,
    pub message: String,
    pub auto_fixable: bool,
}

// ─── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FormatValidationResult {
    pub files_checked: usize,
    pub issues: Vec<FormatIssue>,
    pub error_count: usize,
    pub warn_count: usize,
    pub info_count: usize,
    pub auto_fixable_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatFixResult {
    pub files_checked: usize,
    pub files_fixed: usize,
    pub issues_remaining: Vec<FormatIssue>,
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// Validate all MDX files under `content_dir` against the schema.
///
/// If `schema` is `None`, the default schema is used.
pub fn validate_project(
    _repo_root: &Path,
    content_dir: &Path,
    schema: Option<&FrontmatterSchema>,
) -> std::result::Result<FormatValidationResult, String> {
    let schema = schema.map(|s| s.clone()).unwrap_or_else(FrontmatterSchema::default_schema);
    let files = crate::content::locator::collect_markdown_files(content_dir);

    let mut issues = Vec::new();
    let mut files_checked = 0usize;

    for path in &files {
        let basename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let file_issues = validate_file(path, &schema)?;
        if !file_issues.is_empty() {
            for mut issue in file_issues {
                issue.file = basename.clone();
                issues.push(issue);
            }
        }
        files_checked += 1;
    }

    // Format drift: detect files whose frontmatter keys differ from the majority pattern
    issues.extend(detect_format_drift(&files, &schema)?);

    let error_count = issues.iter().filter(|i| i.severity == Severity::Error).count();
    let warn_count = issues.iter().filter(|i| i.severity == Severity::Warn).count();
    let info_count = issues.iter().filter(|i| i.severity == Severity::Info).count();
    let auto_fixable_count = issues.iter().filter(|i| i.auto_fixable).count();

    Ok(FormatValidationResult {
        files_checked,
        issues,
        error_count,
        warn_count,
        info_count,
        auto_fixable_count,
    })
}

fn validate_file(
    path: &Path,
    schema: &FrontmatterSchema,
) -> std::result::Result<Vec<FormatIssue>, String> {
    let mut issues = Vec::new();
    let content = std::fs::read_to_string(path).unwrap_or_default();

    // 1. Frontmatter block exists
    let Some((fm, _body)) = crate::content::cleaner::parse_frontmatter(&content) else {
        issues.push(FormatIssue {
            file: String::new(),
            issue_type: "missing_frontmatter".into(),
            field: None,
            severity: Severity::Error,
            message: "File has no YAML frontmatter block".into(),
            auto_fixable: true,
        });
        return Ok(issues);
    };

    // Parse frontmatter into a map of canonical_key -> (original_key, value)
    let mut fields: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for line in fm.lines() {
        let trimmed = line.trim();
        if let Some((key, val)) = split_field_line(trimmed) {
            let canonical = schema.canonical(&key);
            fields
                .entry(canonical)
                .or_default()
                .push((key, val));
        }
    }

    // 2. Required fields present
    for req in &schema.required {
        if !fields.contains_key(req) || fields[req].iter().all(|(_, v)| v.is_empty()) {
            issues.push(FormatIssue {
                file: String::new(),
                issue_type: "missing_field".into(),
                field: Some(req.clone()),
                severity: Severity::Error,
                message: format!("Required frontmatter field '{}' is missing or empty", req),
                auto_fixable: true,
            });
        }
    }

    // 3. Unknown aliases (non-canonical key names for known fields)
    for (canonical, occurrences) in &fields {
        for (original, _) in occurrences {
            if original.to_lowercase() != *canonical {
                issues.push(FormatIssue {
                    file: String::new(),
                    issue_type: "unknown_alias".into(),
                    field: Some(original.clone()),
                    severity: Severity::Warn,
                    message: format!(
                        "Frontmatter uses alias '{}'; canonical name is '{}'",
                        original, canonical
                    ),
                    auto_fixable: true,
                });
            }
        }
    }

    // 4. Date format valid
    if let Some(date_occurrences) = fields.get("date") {
        if let Some((_, date_val)) = date_occurrences.first() {
            if !date_val.is_empty() && !is_valid_iso_date(date_val) {
                issues.push(FormatIssue {
                    file: String::new(),
                    issue_type: "invalid_date".into(),
                    field: Some("date".into()),
                    severity: Severity::Error,
                    message: format!("Date '{}' is not a valid YYYY-MM-DD format", date_val),
                    auto_fixable: false,
                });
            }
        }
    }

    // 5. Description length
    if let Some(desc_occurrences) = fields.get("description") {
        if let Some((_, desc_val)) = desc_occurrences.first() {
            if !desc_val.is_empty() {
                if desc_val.len() < 50 {
                    issues.push(FormatIssue {
                        file: String::new(),
                        issue_type: "description_too_short".into(),
                        field: Some("description".into()),
                        severity: Severity::Warn,
                        message: format!("Description is {} chars (minimum 50)", desc_val.len()),
                        auto_fixable: false,
                    });
                } else if desc_val.len() > 160 {
                    issues.push(FormatIssue {
                        file: String::new(),
                        issue_type: "description_too_long".into(),
                        field: Some("description".into()),
                        severity: Severity::Warn,
                        message: format!("Description is {} chars (maximum 160)", desc_val.len()),
                        auto_fixable: false,
                    });
                }
            }
        }
    }

    // 6. Title length
    if let Some(title_occurrences) = fields.get("title") {
        if let Some((_, title_val)) = title_occurrences.first() {
            if !title_val.is_empty() && title_val.len() > 100 {
                issues.push(FormatIssue {
                    file: String::new(),
                    issue_type: "title_too_long".into(),
                    field: Some("title".into()),
                    severity: Severity::Warn,
                    message: format!("Title is {} chars (maximum 100)", title_val.len()),
                    auto_fixable: false,
                });
            }
        }
    }

    // 7. Duplicate fields
    for (canonical, occurrences) in &fields {
        if occurrences.len() > 1 {
            issues.push(FormatIssue {
                file: String::new(),
                issue_type: "duplicate_field".into(),
                field: Some(canonical.clone()),
                severity: Severity::Warn,
                message: format!("Field '{}' appears {} times in frontmatter", canonical, occurrences.len()),
                auto_fixable: true,
            });
        }
    }

    // 8. Unquoted values (values that contain spaces or special chars but aren't quoted)
    for line in fm.lines() {
        let trimmed = line.trim();
        if let Some((key, val)) = split_field_line(trimmed) {
            if !val.is_empty() && !is_quoted(&val) && needs_quoting(&val) {
                issues.push(FormatIssue {
                    file: String::new(),
                    issue_type: "unquoted_value".into(),
                    field: Some(key),
                    severity: Severity::Info,
                    message: format!("Value '{}' should be quoted to avoid YAML parsing issues", val),
                    auto_fixable: true,
                });
            }
        }
    }

    Ok(issues)
}

/// Detect format drift: files whose frontmatter keys differ from the majority pattern.
/// This is an Info-level issue meant to surface inconsistency across the project.
fn detect_format_drift(
    files: &[PathBuf],
    schema: &FrontmatterSchema,
) -> std::result::Result<Vec<FormatIssue>, String> {
    if files.len() < 3 {
        return Ok(Vec::new());
    }

    // Build a frequency map of key sets
    let mut key_set_counts: HashMap<Vec<String>, usize> = HashMap::new();
    for path in files {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if let Some((fm, _)) = crate::content::cleaner::parse_frontmatter(&content) {
            let mut keys: Vec<String> = Vec::new();
            for line in fm.lines() {
                if let Some((key, _)) = split_field_line(line.trim()) {
                    let canonical = schema.canonical(&key);
                    if !keys.contains(&canonical) {
                        keys.push(canonical);
                    }
                }
            }
            keys.sort();
            *key_set_counts.entry(keys).or_insert(0) += 1;
        }
    }

    if key_set_counts.len() <= 1 {
        return Ok(Vec::new());
    }

    // Find the majority pattern
    let majority = key_set_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(keys, _)| keys.clone())
        .unwrap_or_default();

    let majority_set: HashSet<String> = majority.iter().cloned().collect();

    let mut issues = Vec::new();
    for path in files {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if let Some((fm, _)) = crate::content::cleaner::parse_frontmatter(&content) {
            let mut keys: Vec<String> = Vec::new();
            for line in fm.lines() {
                if let Some((key, _)) = split_field_line(line.trim()) {
                    let canonical = schema.canonical(&key);
                    if !keys.contains(&canonical) {
                        keys.push(canonical);
                    }
                }
            }
            keys.sort();
            if keys != majority {
                let basename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let extra: Vec<String> = keys
                    .iter()
                    .filter(|k| !majority_set.contains(*k))
                    .cloned()
                    .collect();
                let msg = if extra.is_empty() {
                    "Frontmatter key set differs from the majority pattern in this project".into()
                } else {
                    format!(
                        "Frontmatter has extra keys [{}] not in the majority pattern",
                        extra.join(", ")
                    )
                };
                issues.push(FormatIssue {
                    file: basename,
                    issue_type: "format_drift".into(),
                    field: None,
                    severity: Severity::Info,
                    message: msg,
                    auto_fixable: false,
                });
            }
        }
    }

    Ok(issues)
}

// ─── Fix engine ───────────────────────────────────────────────────────────────

/// Apply auto-fixes for all auto-fixable issues.
pub fn apply_fixes(
    issues: &[FormatIssue],
    repo_root: &Path,
) -> std::result::Result<FormatFixResult, String> {
    // Group issues by file
    let mut by_file: HashMap<String, Vec<&FormatIssue>> = HashMap::new();
    for issue in issues.iter().filter(|i| i.auto_fixable) {
        by_file
            .entry(issue.file.clone())
            .or_default()
            .push(issue);
    }

    let mut files_fixed = 0usize;
    let mut remaining = Vec::new();

    for (basename, file_issues) in by_file {
        let content_dir = crate::content::ops::resolve_content_dir(
            &repo_root.join(".github").join("automation"),
            repo_root,
        )
        .map_err(|e| e.to_string())?;
        let path = content_dir.join(&basename);

        if !path.exists() {
            remaining.extend(file_issues.into_iter().cloned());
            continue;
        }

        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let new_content = match apply_file_fixes(&content, &file_issues) {
            Some(c) => c,
            None => {
                remaining.extend(file_issues.into_iter().cloned());
                continue;
            }
        };

        if new_content != content {
            if let Err(_) = std::fs::write(&path, new_content) {
                remaining.extend(file_issues.into_iter().cloned());
                continue;
            }
            files_fixed += 1;
        }
    }

    // Add back all non-auto-fixable issues
    for issue in issues.iter().filter(|i| !i.auto_fixable) {
        remaining.push(issue.clone());
    }

    let unique_files: HashSet<String> = issues.iter().map(|i| i.file.clone()).collect();

    Ok(FormatFixResult {
        files_checked: unique_files.len(),
        files_fixed,
        issues_remaining: remaining,
    })
}

/// Apply fixes to a single file's content. Returns None if no frontmatter could be parsed
/// and the issue type isn't `missing_frontmatter`.
fn apply_file_fixes(content: &str, issues: &[&FormatIssue]) -> Option<String> {
    let has_missing_fm = issues.iter().any(|i| i.issue_type == "missing_frontmatter");

    if has_missing_fm {
        // Create frontmatter block with any missing required fields we can infer
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut fm_lines = vec![format!("date: \"{}\"", today)];
        for issue in issues {
            if issue.issue_type == "missing_field" {
                if let Some(field) = &issue.field {
                    if !fm_lines.iter().any(|l| l.starts_with(&format!("{}:", field))) {
                        fm_lines.push(format!("{}: \"\"", field));
                    }
                }
            }
        }
        return Some(format!("---\n{}\n---\n\n{}", fm_lines.join("\n"), content));
    }

    let (fm, body) = crate::content::cleaner::parse_frontmatter(content)?;

    // Build a map of canonical -> list of (original_key, value)
    let mut field_map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut lines_info: Vec<LineInfo> = Vec::new();

    for (idx, line) in fm.lines().enumerate() {
        let trimmed = line.trim();
        if let Some((key, val)) = split_field_line(trimmed) {
            let canonical = canonical_key(&key);
            field_map
                .entry(canonical.clone())
                .or_default()
                .push((key.clone(), val.clone()));
            lines_info.push(LineInfo {
                idx,
                key,
                canonical,
                value: val,
            });
        } else {
            lines_info.push(LineInfo {
                idx,
                key: String::new(),
                canonical: String::new(),
                value: String::new(),
            });
        }
    }

    let mut new_fm_lines: Vec<String> = fm.lines().map(|s| s.to_string()).collect();
    let mut lines_to_remove: HashSet<usize> = HashSet::new();

    for issue in issues {
        match issue.issue_type.as_str() {
            "unknown_alias" => {
                if let Some(field) = &issue.field {
                    let canonical = canonical_key(field);
                    for info in &lines_info {
                        if info.key == *field {
                            new_fm_lines[info.idx] = format!("{}: \"{}\"", canonical, info.value);
                        }
                    }
                }
            }
            "duplicate_field" => {
                if let Some(field) = &issue.field {
                    let mut seen = false;
                    for info in &lines_info {
                        if info.canonical == *field && !info.key.is_empty() {
                            if seen {
                                lines_to_remove.insert(info.idx);
                            } else {
                                seen = true;
                            }
                        }
                    }
                }
            }
            "missing_field" => {
                if let Some(field) = &issue.field {
                    if !field_map.contains_key(field) {
                        // Insert after title if possible, otherwise at end
                        let insert_idx = lines_info
                            .iter()
                            .rfind(|i| i.canonical == "title")
                            .map(|i| i.idx + 1)
                            .unwrap_or(new_fm_lines.len());
                        new_fm_lines.insert(insert_idx, format!("{}: \"\"", field));
                        // Update indices for subsequent operations
                        for info in &mut lines_info {
                            if info.idx >= insert_idx {
                                info.idx += 1;
                            }
                        }
                    }
                }
            }
            "unquoted_value" => {
                if let Some(field) = &issue.field {
                    for info in &lines_info {
                        if info.key == *field && !is_quoted(&info.value) && needs_quoting(&info.value) {
                            new_fm_lines[info.idx] = format!("{}: \"{}\"", info.key, info.value);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Remove duplicate lines
    let mut filtered: Vec<String> = Vec::new();
    for (idx, line) in new_fm_lines.into_iter().enumerate() {
        if !lines_to_remove.contains(&idx) {
            filtered.push(line);
        }
    }

    Some(format!("---\n{}\n---\n\n{}", filtered.join("\n"), body))
}

struct LineInfo {
    idx: usize,
    key: String,
    canonical: String,
    value: String,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Split a frontmatter line like `title: "Foo"` into ("title", "Foo").
fn split_field_line(line: &str) -> Option<(String, String)> {
    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim().to_string();
    let val = line[colon_pos + 1..].trim().to_string();
    let val = val.trim_matches('"').trim_matches('\'').to_string();
    if key.is_empty() {
        return None;
    }
    Some((key, val))
}

/// Check if a string is a valid ISO date (YYYY-MM-DD).
fn is_valid_iso_date(s: &str) -> bool {
    let re = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
    if !re.is_match(s) {
        return false;
    }
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

/// Check if a value is already quoted.
fn is_quoted(s: &str) -> bool {
    (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))
}

/// Check if a value should be quoted for YAML safety.
fn needs_quoting(s: &str) -> bool {
    s.contains(' ') || s.contains(':') || s.contains('#') || s.contains('[') || s.contains('{')
}

/// Canonical key name (lowercase, no aliases).
fn canonical_key(key: &str) -> String {
    let lower = key.to_lowercase();
    let alias_map: HashMap<&str, &str> = [
        ("publisheddate", "date"),
        ("published_date", "date"),
        ("metadescription", "description"),
        ("meta_description", "description"),
    ]
    .into_iter()
    .collect();
    alias_map.get(lower.as_str()).copied().unwrap_or_else(|| lower.as_str()).to_string()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_missing_frontmatter() {
        let schema = FrontmatterSchema::default_schema();
        let content = "# Title\n\nBody.";
        let issues = validate_file_content(content, &schema);
        assert!(issues.iter().any(|i| i.issue_type == "missing_frontmatter"));
    }

    #[test]
    fn validate_missing_field() {
        let schema = FrontmatterSchema::default_schema();
        let content = "---\ntitle: \"Foo\"\n---\n\nBody.";
        let issues = validate_file_content(content, &schema);
        assert!(issues.iter().any(|i| i.issue_type == "missing_field" && i.field == Some("date".into())));
        assert!(issues.iter().any(|i| i.issue_type == "missing_field" && i.field == Some("description".into())));
    }

    #[test]
    fn validate_unknown_alias() {
        let schema = FrontmatterSchema::default_schema();
        let content = "---\ntitle: \"Foo\"\nmetaDescription: \"Bar\"\ndate: \"2024-01-01\"\ndescription: \"Baz\"\n---\n\nBody.";
        let issues = validate_file_content(content, &schema);
        assert!(issues.iter().any(|i| i.issue_type == "unknown_alias" && i.field == Some("metaDescription".into())));
    }

    #[test]
    fn validate_invalid_date() {
        let schema = FrontmatterSchema::default_schema();
        let content = "---\ntitle: \"Foo\"\ndate: \"not-a-date\"\ndescription: \"Bar\"\n---\n\nBody.";
        let issues = validate_file_content(content, &schema);
        assert!(issues.iter().any(|i| i.issue_type == "invalid_date"));
    }

    #[test]
    fn validate_duplicate_field() {
        let schema = FrontmatterSchema::default_schema();
        let content = "---\ntitle: \"Foo\"\ntitle: \"Bar\"\ndate: \"2024-01-01\"\ndescription: \"Baz\"\n---\n\nBody.";
        let issues = validate_file_content(content, &schema);
        assert!(issues.iter().any(|i| i.issue_type == "duplicate_field" && i.field == Some("title".into())));
    }

    #[test]
    fn fix_missing_frontmatter() {
        let content = "# Title\n\nBody.";
        let issues = vec![FormatIssue {
            file: "test.mdx".into(),
            issue_type: "missing_frontmatter".into(),
            field: None,
            severity: Severity::Error,
            message: "no frontmatter".into(),
            auto_fixable: true,
        }];
        let fixed = apply_file_fixes(content, &issues.iter().collect::<Vec<_>>()).unwrap();
        assert!(fixed.starts_with("---\n"));
        assert!(fixed.contains("# Title"));
    }

    #[test]
    fn fix_unknown_alias() {
        let content = "---\ntitle: \"Foo\"\nmetaDescription: \"Bar\"\ndate: \"2024-01-01\"\ndescription: \"Baz\"\n---\n\nBody.";
        let issues = vec![FormatIssue {
            file: "test.mdx".into(),
            issue_type: "unknown_alias".into(),
            field: Some("metaDescription".into()),
            severity: Severity::Warn,
            message: "alias".into(),
            auto_fixable: true,
        }];
        let fixed = apply_file_fixes(content, &issues.iter().collect::<Vec<_>>()).unwrap();
        assert!(fixed.contains("description: \"Bar\""));
        assert!(!fixed.contains("metaDescription:"));
    }

    #[test]
    fn fix_duplicate_field() {
        let content = "---\ntitle: \"Foo\"\ntitle: \"Bar\"\ndate: \"2024-01-01\"\ndescription: \"Baz\"\n---\n\nBody.";
        let issues = vec![FormatIssue {
            file: "test.mdx".into(),
            issue_type: "duplicate_field".into(),
            field: Some("title".into()),
            severity: Severity::Warn,
            message: "dup".into(),
            auto_fixable: true,
        }];
        let fixed = apply_file_fixes(content, &issues.iter().collect::<Vec<_>>()).unwrap();
        let title_count = fixed.matches("title:").count();
        assert_eq!(title_count, 1);
    }

    #[test]
    fn canonical_key_resolution() {
        let schema = FrontmatterSchema::default_schema();
        assert_eq!(schema.canonical("publishedDate"), "date");
        assert_eq!(schema.canonical("meta_description"), "description");
        assert_eq!(schema.canonical("title"), "title");
    }

    #[test]
    fn date_validation() {
        assert!(is_valid_iso_date("2024-01-15"));
        assert!(!is_valid_iso_date("2024-13-01")); // invalid month
        assert!(!is_valid_iso_date("not-a-date"));
        assert!(!is_valid_iso_date("2024-01-01T00:00:00"));
    }

    // Helper: validate content string directly (no file I/O)
    fn validate_file_content(content: &str, schema: &FrontmatterSchema) -> Vec<FormatIssue> {
        let mut issues = Vec::new();

        let Some((fm, _body)) = crate::content::cleaner::parse_frontmatter(content) else {
            issues.push(FormatIssue {
                file: String::new(),
                issue_type: "missing_frontmatter".into(),
                field: None,
                severity: Severity::Error,
                message: "no frontmatter".into(),
                auto_fixable: true,
            });
            return issues;
        };

        let mut fields: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for line in fm.lines() {
            if let Some((key, val)) = split_field_line(line.trim()) {
                let canonical = schema.canonical(&key);
                fields.entry(canonical).or_default().push((key, val));
            }
        }

        for req in &schema.required {
            if !fields.contains_key(req) || fields[req].iter().all(|(_, v)| v.is_empty()) {
                issues.push(FormatIssue {
                    file: String::new(),
                    issue_type: "missing_field".into(),
                    field: Some(req.clone()),
                    severity: Severity::Error,
                    message: format!("missing {}", req),
                    auto_fixable: true,
                });
            }
        }

        for (canonical, occurrences) in &fields {
            for (original, _) in occurrences {
                if original.to_lowercase() != *canonical {
                    issues.push(FormatIssue {
                        file: String::new(),
                        issue_type: "unknown_alias".into(),
                        field: Some(original.clone()),
                        severity: Severity::Warn,
                        message: format!("alias {} -> {}", original, canonical),
                        auto_fixable: true,
                    });
                }
            }
        }

        if let Some(date_occurrences) = fields.get("date") {
            if let Some((_, date_val)) = date_occurrences.first() {
                if !date_val.is_empty() && !is_valid_iso_date(date_val) {
                    issues.push(FormatIssue {
                        file: String::new(),
                        issue_type: "invalid_date".into(),
                        field: Some("date".into()),
                        severity: Severity::Error,
                        message: format!("bad date {}", date_val),
                        auto_fixable: false,
                    });
                }
            }
        }

        for (canonical, occurrences) in &fields {
            if occurrences.len() > 1 {
                issues.push(FormatIssue {
                    file: String::new(),
                    issue_type: "duplicate_field".into(),
                    field: Some(canonical.clone()),
                    severity: Severity::Warn,
                    message: format!("dup {}", canonical),
                    auto_fixable: true,
                });
            }
        }

        issues
    }
}
