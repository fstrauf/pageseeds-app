/// Shared file-reading helpers used by content and content_audit modules.
///
/// Also includes artifact diffing utilities for the Health dashboard.

/// Read an article source file. Returns None if not found or unreadable.
pub fn read_source_file(repo_root: &std::path::Path, file_ref: &str) -> Option<String> {
    if file_ref.is_empty() {
        return None;
    }
    let p = std::path::Path::new(file_ref);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };
    std::fs::read_to_string(&full).ok()
}

/// Parse YAML frontmatter from an MDX/markdown source string.
///
/// Delegates to `frontmatter::split_mdx` + `frontmatter::parse` — the
/// single source of truth for frontmatter parsing.
/// Returns (frontmatter_map, body_string).
pub fn parse_frontmatter(
    source: &str,
) -> (std::collections::HashMap<String, String>, String) {
    let mut fm = std::collections::HashMap::new();

    let (fm_text, body) = match crate::content::frontmatter::split_mdx(source) {
        Some((t, b)) => (t, b.to_string()),
        None => return (fm, source.to_string()),
    };

    if let Ok(parsed) = crate::content::frontmatter::parse(fm_text) {
        if let Some(mapping) = parsed.parsed.as_mapping() {
            for (key, value) in mapping {
                if let Some(k) = key.as_str() {
                    if let Some(v) = value.as_str() {
                        fm.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
    }

    (fm, body)
}

/// Diff two JSON artifact values and return a summary of changes.
///
/// Returns (added, removed, changed) counts based on comparing items
/// by their `id` or `url_slug` field. Used by the Health dashboard
/// to show "+3 new, -2 resolved" style diffs.
pub(crate) fn diff_artifacts(
    old: &serde_json::Value,
    new: &serde_json::Value,
    item_key: &str, // e.g. "articles", "duplicate_groups"
    id_field: &str, // e.g. "id", "url_slug", "hash"
) -> (usize, usize, usize) {
    let old_items: Vec<&serde_json::Value> = old.get(item_key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let new_items: Vec<&serde_json::Value> = new.get(item_key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    let old_ids: std::collections::HashSet<String> = old_items
        .iter()
        .filter_map(|item| item.get(id_field).and_then(|v| v.as_str()).map(String::from))
        .collect();
    let new_ids: std::collections::HashSet<String> = new_items
        .iter()
        .filter_map(|item| item.get(id_field).and_then(|v| v.as_str()).map(String::from))
        .collect();

    let added = new_ids.difference(&old_ids).count();
    let removed = old_ids.difference(&new_ids).count();

    // Changed = items present in both but with different check failure status
    let mut changed = 0usize;
    for old_item in &old_items {
        let id = match old_item.get(id_field).and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        if !new_ids.contains(id) {
            continue;
        }
        if let Some(new_item) = new_items.iter().find(|i| {
            i.get(id_field).and_then(|v| v.as_str()) == Some(id)
        }) {
            // Simple heuristic: compare health_score or health fields
            let old_health = old_item.get("health_score").and_then(|v| v.as_f64());
            let new_health = new_item.get("health_score").and_then(|v| v.as_f64());
            if old_health != new_health {
                changed += 1;
            }
        }
    }

    (added, removed, changed)
}
