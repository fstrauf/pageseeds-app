
// ─── Theme string cleaning ──────────────────────────────────────────────────

/// Strip markdown heading markers (`###`), resolve `Cluster N: Topic` → `"Topic"`,
/// and return `None` for bare cluster labels like `"Cluster 7"` or `"### Cluster 9"`
/// that carry no real search topic.
pub(crate) fn clean_theme_str(raw: &str) -> Option<String> {
    let s = raw.trim().trim_start_matches('#').trim();
    if s.is_empty() {
        return None;
    }

    let s: String = if let Some(colon_pos) = s.find(':') {
        // "Cluster 4: SEO Tools (PLANNED)" → "SEO Tools"
        let after = s[colon_pos + 1..].trim();
        after.split('(').next().unwrap_or(after).trim().to_string()
    } else {
        // Strip trailing parenthetical FIRST, then check for bare cluster label.
        // "Cluster 7 (PLANNED)" → "Cluster 7" → bare → None.
        let s_no_paren = s.split('(').next().unwrap_or(s).trim();
        let words: Vec<&str> = s_no_paren.split_whitespace().collect();
        let is_bare_cluster = words.len() <= 2
            && words
                .first()
                .map(|w| w.eq_ignore_ascii_case("cluster"))
                .unwrap_or(false);
        if is_bare_cluster {
            return None;
        }
        s_no_paren.to_string()
    };

    if s.is_empty() || s.len() <= 2 {
        None
    } else {
        Some(s)
    }
}

/// Parse comma-/newline-separated themes from a task description string,
/// applying the same cleaning rules as brief parsing.
///
/// Returns an empty vec when the description contains only junk cluster labels.
///
/// NOTE: No longer used in production — theme extraction is fully agentic now.
/// Kept for tests only.
#[cfg(test)]
pub(crate) fn parse_desc_themes(raw: &str) -> Vec<String> {
    raw.lines()
        .flat_map(|line| line.split(','))
        .filter_map(clean_theme_str)
        .collect()
}

// ─── Theme auto-derivation ────────────────────────────────────────────────────

/// Try to derive keyword themes from existing project configuration files.
///
/// Priority order:
///   1. `project.md` — consolidated project config (PLANNED clusters, Identity)
///   2. `*seo_content_brief*.md` — legacy: PLANNED cluster topics (🎯) and gap cluster names
///   3. `*project_summary*.md`   — legacy: Content Pillar names
///   4. `articles.json`          — unique existing target_keywords (as baseline coverage)
pub(crate) fn derive_themes_from_project(automation_dir: &std::path::Path) -> Vec<String> {
    // Primary: consolidated project.md
    let project_md = automation_dir.join("project.md");
    if project_md.exists() {
        log::info!("[keyword_research] using project.md: {:?}", project_md);
        let themes = extract_from_brief(&project_md);
        if !themes.is_empty() {
            return themes;
        }
        // Also try summary extraction (for Content Clusters & Identity sections)
        let themes = extract_from_summary(&project_md);
        if !themes.is_empty() {
            return themes;
        }
    }

    // Legacy fallbacks
    if let Some(brief) = find_file_by_suffix(automation_dir, "seo_content_brief.md") {
        log::info!("[keyword_research] using brief: {:?}", brief);
        let themes = extract_from_brief(&brief);
        if !themes.is_empty() {
            return themes;
        }
    }

    if let Some(summary) = find_file_by_suffix(automation_dir, "project_summary.md") {
        log::info!("[keyword_research] using summary: {:?}", summary);
        let themes = extract_from_summary(&summary);
        if !themes.is_empty() {
            return themes;
        }
    }

    let articles_json = automation_dir.join("articles.json");
    if articles_json.exists() {
        let themes = extract_from_articles(&articles_json);
        if !themes.is_empty() {
            return themes;
        }
    }

    vec![]
}

/// Find the first file in `dir` whose name contains `suffix` (case-insensitive).
pub(crate) fn find_file_by_suffix(dir: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
    let exact = dir.join(suffix);
    if exact.exists() {
        return Some(exact);
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return None };
    let suffix_lower = suffix.to_lowercase();
    entries
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

/// Extract themes from `seo_content_brief.md`.
pub(crate) fn extract_from_brief(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };

    let planned_items: Vec<String> = content
        .lines()
        .filter(|l| l.contains('🎯'))
        .filter_map(|l| {
            // Strip all decorators first, then delegate to clean_theme_str which:
            // - strips '#' heading markers
            // - resolves "Cluster N: Topic (annotation)" → "Topic"
            // - rejects bare "Cluster N" / "### Cluster N" labels
            let stripped = l.trim()
                .trim_start_matches("- [ ] ")
                .trim_start_matches("- [x] ")
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .replace('🎯', "")
                .replace("**", "")
                .trim()
                .to_string();
            clean_theme_str(&stripped)
        })
        .take(8)
        .collect();

    if !planned_items.is_empty() {
        return planned_items;
    }

    let planned_clusters: Vec<String> = content
        .lines()
        .filter(|l| l.contains("PLANNED") && l.starts_with("###"))
        .filter_map(clean_theme_str)
        .take(8)
        .collect();

    if !planned_clusters.is_empty() {
        return planned_clusters;
    }

    content
        .lines()
        .filter(|l| l.starts_with("### Cluster"))
        .filter_map(clean_theme_str)
        .take(6)
        .collect()
}

/// Extract content pillar topics from `project_summary.md`.
pub(crate) fn extract_from_summary(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };

    let mut themes: Vec<String> = content
        .lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            lower.contains("pillar") && l.starts_with("###")
        })
        .map(|l| {
            let s = l.trim_start_matches('#').trim();
            let s = s.split(':').nth(1).unwrap_or(s).trim();
            s.split('(').next().unwrap_or(s).trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .take(6)
        .collect();

    if themes.is_empty() {
        let mut in_keywords = false;
        for line in content.lines() {
            if line.contains("Search Keywords") {
                in_keywords = true;
                continue;
            }
            if in_keywords {
                if line.trim().starts_with('-') || line.trim().starts_with('*') {
                    let kw = line.trim()
                        .trim_start_matches('-')
                        .trim_start_matches('*')
                        .trim()
                        .trim_matches('"')
                        .to_string();
                    if !kw.is_empty() {
                        themes.push(kw);
                    }
                    if themes.len() >= 8 { break; }
                } else if line.trim().is_empty() || line.starts_with('#') {
                    in_keywords = false;
                }
            }
        }
    }

    themes
}

/// Extract unique target_keywords from `articles.json` as theme seeds.
pub(crate) fn extract_from_articles(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };
    let Ok(articles) = serde_json::from_str::<Vec<serde_json::Value>>(&content) else { return vec![] };

    let mut seen = std::collections::HashSet::new();
    let mut themes = Vec::new();

    for article in &articles {
        if let Some(kw) = article.get("target_keyword").and_then(|v| v.as_str()) {
            if kw.is_empty() { continue; }
            let short: String = kw.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            let lower = short.to_lowercase();
            if seen.insert(lower.clone()) {
                themes.push(short);
            }
        }
        if themes.len() >= 6 { break; }
    }

    themes
}
