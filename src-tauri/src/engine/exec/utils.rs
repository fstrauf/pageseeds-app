/// Shared file-reading helpers used by content and content_audit modules.

/// Read an article source file. Returns None if not found or unreadable.
pub(crate) fn read_source_file(repo_root: &std::path::Path, file_ref: &str) -> Option<String> {
    if file_ref.is_empty() { return None; }
    let p = std::path::Path::new(file_ref);
    let full = if p.is_absolute() { p.to_path_buf() } else { repo_root.join(p) };
    std::fs::read_to_string(&full).ok()
}

/// Parse YAML frontmatter from an MDX/markdown source string.
/// Returns (frontmatter_map, body_string).
pub(crate) fn parse_frontmatter(source: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut fm = std::collections::HashMap::new();
    if !source.starts_with("---") {
        return (fm, source.to_string());
    }
    let end = match source[3..].find("\n---") {
        Some(i) => i + 3,
        None => return (fm, source.to_string()),
    };
    let fm_text = &source[3..end];
    let body = source[end + 4..].trim_start().to_string();
    for line in fm_text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            fm.insert(k.trim().to_string(), val);
        }
    }
    (fm, body)
}
