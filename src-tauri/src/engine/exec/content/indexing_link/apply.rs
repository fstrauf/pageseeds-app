use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ─── Step 3: Apply ────────────────────────────────────────────────────────────

/// Deterministic apply: append a Related Articles link or insert a contextual
/// paragraph link to the chosen source file.
pub(crate) fn exec_indexing_link_apply(task: &Task, project_path: &str) -> StepResult {
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Parse target artifact for metadata
    let target_data = match parse_target_artifact(task) {
        Some(t) => t,
        None => {
            return StepResult {
                success: false,
                message: "Missing or invalid indexing_link_target artifact".to_string(),
                output: None,
            }
        }
    };

    let target_article_id = target_data["article_id"].as_i64().unwrap_or(0);
    if target_article_id == 0 {
        return StepResult {
            success: false,
            message: "Target article_id is 0 — no matching article found in DB".to_string(),
            output: None,
        };
    }

    let target_slug = crate::content::slug::normalize_url_slug(target_data["slug"].as_str().unwrap_or(""));
    let target_title = target_data["target_keyword"]
        .as_str()
        .unwrap_or(&target_slug)
        .to_string();

    // Load plan
    let plan_path = paths
        .automation_dir
        .join(format!("indexing_link_plan_{}.json", task.id));
    let plan: serde_json::Value = std::fs::read_to_string(&plan_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "indexing_link_plan")
                .and_then(|a| a.content.as_ref())
                .and_then(|c| serde_json::from_str(c).ok())
        })
        .unwrap_or_default();

    let links = plan["links_to_add"].as_array().cloned().unwrap_or_default();
    if links.is_empty() {
        return StepResult {
            success: true,
            message: "Nothing to apply — plan has no links to add".to_string(),
            output: None,
        };
    }

    // Locate content directory
    let resolution = crate::content::locator::resolve(repo_root, None);
    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return StepResult {
                success: false,
                message: "Could not locate content directory".to_string(),
                output: None,
            }
        }
    };

    // Build basename → full path map
    let all_files = crate::content::locator::collect_markdown_files(&content_dir);
    let file_map: HashMap<String, std::path::PathBuf> = all_files
        .iter()
        .filter_map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|name| (name.to_string(), p.clone()))
        })
        .collect();

    let mut files_modified: Vec<String> = Vec::new();
    let mut links_added = 0usize;
    let mut links_skipped_existing = 0usize;
    let mut links_failed = 0usize;

    for link in &links {
        let source_id = link["source_article_id"].as_i64().unwrap_or(0);
        let anchor_text = link["anchor_text"]
            .as_str()
            .unwrap_or(&target_title)
            .to_string();
        let placement = link["placement"].as_str().unwrap_or("related_section");

        // Find source file from DB
        let source_file = find_file_by_article_id(&task.project_id, source_id);
        let source_basename = match source_file {
            Some(f) => std::path::Path::new(&f)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&f)
                .to_string(),
            None => {
                log::warn!(
                    "[indexing_link_apply] no file found for source article {}",
                    source_id
                );
                links_failed += 1;
                continue;
            }
        };

        let Some(file_path) = file_map.get(&source_basename) else {
            log::warn!(
                "[indexing_link_apply] source file not found in content dir: {}",
                source_basename
            );
            links_failed += 1;
            continue;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "[indexing_link_apply] cannot read {}: {}",
                    file_path.display(),
                    e
                );
                links_failed += 1;
                continue;
            }
        };

        // Skip if already links to target
        if content.contains(&crate::content::slug::format_blog_link(&target_slug)) {
            log::info!(
                "[indexing_link_apply] {} already links to {} — skipping",
                source_basename,
                target_slug
            );
            links_skipped_existing += 1;
            continue;
        }

        let new_content = match placement {
            "contextual_paragraph" => {
                match insert_contextual_link(&content, &anchor_text, &target_slug) {
                    Some(c) => c,
                    None => {
                        log::warn!(
                            "[indexing_link_apply] could not find insertion point for contextual link in {}",
                            source_basename
                        );
                        links_failed += 1;
                        continue;
                    }
                }
            }
            _ => {
                // related_section (default)
                apply_related_section_link(&content, &anchor_text, &target_slug)
            }
        };

        match std::fs::write(file_path, new_content) {
            Ok(_) => {
                files_modified.push(source_basename.clone());
                links_added += 1;
                log::info!(
                    "[indexing_link_apply] {} — added {} link to {}",
                    source_basename,
                    placement,
                    target_slug
                );
            }
            Err(e) => {
                log::warn!(
                    "[indexing_link_apply] failed to write {}: {}",
                    file_path.display(),
                    e
                );
                links_failed += 1;
            }
        }
    }

    let summary = serde_json::json!({
        "target_slug": target_slug,
        "links_added": links_added,
        "links_skipped_existing": links_skipped_existing,
        "links_failed": links_failed,
        "source_files_modified": files_modified,
    });

    // Success if we added links, OR if all planned links already existed.
    // Failure only if there were actual errors (file not found, write failed, etc.)
    let success = links_added > 0
        || (links_failed == 0 && links_skipped_existing > 0);
    let message = if links_added > 0 {
        format!(
            "Applied {} link(s) to {} for target {}",
            links_added,
            files_modified.join(", "),
            target_slug
        )
    } else if links_skipped_existing > 0 && links_failed == 0 {
        format!(
            "Link(s) to {} already exist — no changes needed",
            target_slug
        )
    } else {
        format!(
            "Applied {} link(s) to {} for target {}",
            links_added,
            files_modified.join(", "),
            target_slug
        )
    };

    StepResult {
        success,
        message,
        output: Some(summary.to_string()),
    }
}

pub(crate) fn apply_related_section_link(content: &str, anchor_text: &str, target_slug: &str) -> String {
    let related_section_start = content.lines().position(|l| {
        let t = l.trim();
        t.starts_with("##") && t.to_lowercase().contains("related")
    });

    let new_link_line = format!("- [{}]({})\n", anchor_text, crate::content::slug::format_blog_link(target_slug));

    if let Some(start_idx) = related_section_start {
        let lines: Vec<&str> = content.lines().collect();
        let end_idx = lines
            .iter()
            .enumerate()
            .skip(start_idx + 1)
            .find(|(_, l)| {
                let t = l.trim();
                t.starts_with("##") && !t.to_lowercase().contains("related")
            })
            .map(|(i, _)| i)
            .unwrap_or(lines.len());

        let before = lines[..start_idx].join("\n");
        let section = lines[start_idx..end_idx].join("\n");
        let after = lines[end_idx..].join("\n");

        let new_section = format!("{}\n{}", section.trim_end(), new_link_line.trim_end());
        if after.is_empty() {
            format!("{}\n{}", before.trim_end(), new_section)
        } else {
            format!("{}\n{}\n{}", before.trim_end(), new_section, after)
        }
    } else {
        format!(
            "{}\n\n## Related Articles\n\n{}",
            content.trim_end(),
            new_link_line
        )
    }
}

/// Insert a contextual link into a relevant paragraph.
///
/// Finds a paragraph that contains the target keyword or a related term,
/// and appends a natural sentence with the link at the end of that paragraph.
/// Returns None if no suitable paragraph is found.
pub(crate) fn insert_contextual_link(content: &str, anchor_text: &str, target_slug: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut best_paragraph_idx: Option<usize> = None;
    let mut best_score = 0;

    // Simple heuristic: find a paragraph (non-empty, non-heading, non-list, non-code)
    // that contains words related to the anchor text
    let anchor_words: Vec<String> = anchor_text
        .to_lowercase()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // Skip headings, lists, code blocks, frontmatter, empty lines
        if trimmed.starts_with('#')
            || trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.starts_with("```")
            || trimmed.starts_with("---")
            || trimmed.is_empty()
        {
            continue;
        }

        let line_lower = trimmed.to_lowercase();
        let score = anchor_words
            .iter()
            .filter(|word| line_lower.contains(*word))
            .count();

        if score > best_score {
            best_score = score;
            best_paragraph_idx = Some(idx);
        }
    }

    // If no paragraph contains related words, try to find any substantial paragraph
    let target_idx = best_paragraph_idx.or_else(|| {
        lines.iter().enumerate().find_map(|(idx, line)| {
            let trimmed = line.trim();
            if trimmed.len() > 80
                && !trimmed.starts_with('#')
                && !trimmed.starts_with('-')
                && !trimmed.starts_with('*')
                && !trimmed.starts_with("```")
                && !trimmed.starts_with("---")
            {
                Some(idx)
            } else {
                None
            }
        })
    })?;

    let insertion_sentence = format!(
        " For more on this, see [{}]({}).",
        anchor_text, crate::content::slug::format_blog_link(target_slug)
    );

    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let original_line = &new_lines[target_idx];
    new_lines[target_idx] = format!("{}{}", original_line.trim_end(), insertion_sentence);

    Some(new_lines.join("\n"))
}

