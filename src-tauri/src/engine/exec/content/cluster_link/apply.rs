use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

/// Step 3 for `cluster_and_link`: deterministic apply step that writes the
/// recommended "Related Articles" sections to MDX files.
///
/// Reads `links_to_add.json` produced by the strategy step, groups links by
/// source article, and appends a `## Related Articles` section to each MDX
/// file that does not already have one.
pub(crate) fn exec_cluster_link_apply(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use std::collections::HashMap;
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // --- Load links_to_add.json ---
    let links_path = paths.automation_dir.join("links_to_add.json");
    let links_doc: serde_json::Value =
        match crate::engine::exec::common::read_json(&links_path, "links_to_add.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    let empty_arr: Vec<serde_json::Value> = Vec::new();
    let links_to_add = links_doc["links_to_add"].as_array().unwrap_or(&empty_arr);

    if links_to_add.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No links to add — strategy found no gaps".to_string(),
            output: Some(r#"{"files_modified":0,"links_added":0,"changes":[]}"#.to_string()),
        };
    }

    // Load article_id_to_file.json (written by strategy step) to resolve IDs → files.
    // Falls back to the legacy source_file field if the mapping is missing.
    let id_to_file_path = paths.automation_dir.join("article_id_to_file.json");
    let id_to_file: HashMap<i64, String> = match crate::engine::exec::common::read_json::<
        serde_json::Value,
    >(&id_to_file_path, "article_id_to_file.json")
    {
        Ok(doc) => doc
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|entry| {
                let id = entry["id"].as_i64()?;
                let file = entry["file"].as_str()?.to_string();
                Some((id, file))
            })
            .collect(),
        Err(_) => HashMap::new(),
    };

    // Locate content directory
    let resolution = crate::content::locator::resolve(repo_root, None);
    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return crate::engine::workflows::StepResult::fail("Could not locate content directory".to_string())
        }
    };

    // Build set of valid link targets from the article database, excluding
    // slugs redirected away by a consolidation.
    let valid_slugs: std::collections::HashSet<String> =
        if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
            crate::engine::task_store::load_valid_link_targets(&db, &task.project_id, project_path)
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };

    // Group links by source_file basename: source_file → vec[(title, slug)]
    let mut by_source: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut skipped_missing_source = 0usize;
    let mut skipped_missing_target = 0usize;
    let mut skipped_unknown_slug = 0usize;
    for link in links_to_add {
        let source_file = if let Some(id) = link["source_article_id"].as_i64() {
            id_to_file.get(&id).cloned().unwrap_or_default()
        } else {
            // Legacy fallback: strategy step wrote source_file directly
            link["source_file"].as_str().unwrap_or("").to_string()
        };
        let target_title = link["target_title"].as_str().unwrap_or("").to_string();
        let target_slug = link["target_slug"].as_str().unwrap_or("").to_string();
        if source_file.is_empty() {
            log::warn!(
                "[cluster_link_apply] skipping recommendation — missing source_article_id mapping: {:?}",
                link
            );
            skipped_missing_source += 1;
            continue;
        }
        if target_slug.is_empty() {
            log::warn!(
                "[cluster_link_apply] skipping recommendation — missing target_slug: {:?}",
                link
            );
            skipped_missing_target += 1;
            continue;
        }
        // Exact match first, normalized fallback — a verbatim-existing slug
        // (e.g. with a leading number) is never rewritten, and redirected
        // slugs are not valid targets.
        match crate::content::slug::resolve_slug(&target_slug, &valid_slugs) {
            Some(resolved) => {
                by_source
                    .entry(source_file)
                    .or_default()
                    .push((target_title, resolved));
            }
            None => {
                log::warn!(
                    "[cluster_link_apply] skipping link to non-existent or redirected slug '{}'; valid slug count={}",
                    target_slug,
                    valid_slugs.len()
                );
                skipped_unknown_slug += 1;
                continue;
            }
        }
    }

    // Build basename → full path map from content dir
    let all_files = crate::content::locator::collect_markdown_files(&content_dir);
    let file_map: HashMap<String, std::path::PathBuf> = all_files
        .iter()
        .filter_map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|name| (name.to_string(), p.clone()))
        })
        .collect();

    let mut files_modified = 0usize;
    let mut links_added = 0usize;
    let mut change_log: Vec<serde_json::Value> = Vec::new();
    let mut skipped_source_not_found = 0usize;
    let mut skipped_read_error = 0usize;
    let mut skipped_already_linked = 0usize;

    for (source_basename, new_links) in &by_source {
        let Some(file_path) = file_map.get(source_basename) else {
            log::warn!(
                "[cluster_link_apply] source file not found in content dir: {}",
                source_basename
            );
            skipped_source_not_found += 1;
            continue;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "[cluster_link_apply] cannot read {}: {}",
                    file_path.display(),
                    e
                );
                skipped_read_error += 1;
                continue;
            }
        };

        // Check if a "Related Articles" section already exists
        let related_section_start = content.lines().position(|l| {
            let t = l.trim();
            t.starts_with("##") && t.to_lowercase().contains("related")
        });

        // Build list of new link lines, skipping slugs already present in the file
        let mut new_link_lines: Vec<String> = Vec::new();
        for (title, slug) in new_links {
            let blog_link = crate::content::slug::format_blog_link(&slug);
            if content.contains(&blog_link) {
                log::info!(
                    "[cluster_link_apply] {} already links to {} — skipping",
                    source_basename,
                    blog_link
                );
                skipped_already_linked += 1;
                continue;
            }
            new_link_lines.push(format!("- [{}]({})\n", title, blog_link));
        }

        if new_link_lines.is_empty() {
            continue;
        }

        let (new_content, added_in_file) = if let Some(start_idx) = related_section_start {
            // --- Merge into existing Related Articles section ---
            let lines: Vec<&str> = content.lines().collect();
            // Find where the next heading begins (end of Related Articles section)
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

            // Extract existing slugs from the current section to deduplicate
            // Simple string scan: find "/blog/" and take everything up to ')'
            let existing_slugs: std::collections::HashSet<String> = lines[start_idx..end_idx]
                .iter()
                .filter_map(|l| {
                    let idx = l.find("/blog/")?;
                    let start = idx + "/blog/".len();
                    let end = l[start..].find(')').unwrap_or(l[start..].len());
                    Some(crate::content::slug::normalize_url_slug(&l[start..start + end]))
                })
                .collect();

            let mut merged_lines: Vec<String> = lines[start_idx..end_idx]
                .iter()
                .map(|l| l.to_string())
                .collect();

            for line in &new_link_lines {
                // Extract slug from the new link line to check for duplicates
                let new_slug = line.find("/blog/").and_then(|idx| {
                    let start = idx + "/blog/".len();
                    let end = line[start..].find(')').unwrap_or(line[start..].len());
                    Some(crate::content::slug::normalize_url_slug(&line[start..start + end]))
                });
                if let Some(ref slug) = new_slug {
                    if existing_slugs.contains(slug) {
                        log::info!(
                            "[cluster_link_apply] {} already links to {} in Related Articles — skipping",
                            source_basename,
                            crate::content::slug::format_blog_link(slug)
                        );
                        skipped_already_linked += 1;
                        continue;
                    }
                }
                merged_lines.push(line.trim_end().to_string());
            }

            let original_section_len = end_idx - start_idx;
            if merged_lines.len() <= original_section_len {
                // Nothing new was added
                continue;
            }

            let before = lines[..start_idx].join("\n");
            let after = lines[end_idx..].join("\n");
            let section = merged_lines.join("\n");
            let new_content = if after.is_empty() {
                format!("{}\n{}", before.trim_end(), section)
            } else {
                format!("{}\n{}\n{}", before.trim_end(), section, after)
            };
            (new_content, merged_lines.len() - original_section_len)
        } else {
            // --- Append new Related Articles section ---
            let mut section = String::from("\n\n## Related Articles\n\n");
            for line in &new_link_lines {
                section.push_str(line);
            }
            let new_content = format!("{}{}", content.trim_end(), section);
            (new_content, new_link_lines.len())
        };
        match std::fs::write(file_path, new_content) {
            Ok(_) => {
                files_modified += 1;
                links_added += added_in_file;
                let link_entries: Vec<serde_json::Value> = new_links
                    .iter()
                    .map(|(t, s)| serde_json::json!({"title": t, "slug": s}))
                    .collect();
                change_log.push(serde_json::json!({
                    "file": source_basename,
                    "links_added": added_in_file,
                    "links": link_entries,
                }));
                log::info!(
                    "[cluster_link_apply] {} — added {} Related Articles links",
                    source_basename,
                    added_in_file
                );
            }
            Err(e) => log::warn!(
                "[cluster_link_apply] failed to write {}: {}",
                file_path.display(),
                e
            ),
        }
    }

    // Re-scan the link graph after applying changes so the next drift
    // detection or cluster_and_link run sees the updated state.
    let (orphans_remaining, zero_incoming_remaining) = if files_modified > 0 {
        let mut orphans = 0i32;
        let mut zero_incoming = 0i32;
        if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
            if let Ok(articles) =
                crate::content::article_index::list_articles(&db, &task.project_id)
            {
                let articles: Vec<_> = articles
                    .into_iter()
                    .filter(|a| !a.file.is_empty())
                    .collect();
                let resolution = crate::content::locator::resolve(Path::new(project_path), None);
                if let Some(content_dir) = resolution.selected {
                    match crate::content::linking::scan_links(&content_dir, &articles) {
                        Ok(result) => {
                            let json = serde_json::to_string_pretty(&result).unwrap_or_default();
                            let paths = ProjectPaths::from_path(project_path);
                            let scan_path = paths.automation_dir.join("link_scan.json");
                            if let Err(e) = std::fs::write(&scan_path, &json) {
                                log::warn!("[cluster_link_apply] failed to write updated link_scan.json: {}", e);
                            } else {
                                log::info!(
                                    "[cluster_link_apply] re-scanned and saved link_scan.json: {} articles, {} orphans, {} zero-incoming",
                                    result.total_articles,
                                    result.orphan_ids.len(),
                                    result.zero_incoming_ids.len()
                                );
                            }
                            orphans = result.orphan_ids.len() as i32;
                            zero_incoming = result.zero_incoming_ids.len() as i32;
                        }
                        Err(e) => {
                            log::warn!("[cluster_link_apply] re-scan failed: {}", e);
                        }
                    }
                } else {
                    log::warn!("[cluster_link_apply] could not locate content dir for re-scan");
                }
            } else {
                log::warn!("[cluster_link_apply] failed to load articles for re-scan");
            }
        } else {
            log::warn!("[cluster_link_apply] failed to open DB for re-scan");
        }
        (orphans, zero_incoming)
    } else {
        (0, 0)
    };

    let summary = serde_json::json!({
        "files_modified": files_modified,
        "links_added": links_added,
        "changes": change_log,
        "orphans_remaining": orphans_remaining,
        "zero_incoming_remaining": zero_incoming_remaining,
        "skipped": {
            "missing_source_mapping": skipped_missing_source,
            "missing_target_slug": skipped_missing_target,
            "unknown_target_slug": skipped_unknown_slug,
            "source_file_not_found": skipped_source_not_found,
            "source_file_read_error": skipped_read_error,
            "link_already_exists": skipped_already_linked,
        },
        "recommendations_count": links_to_add.len(),
    });
    crate::engine::workflows::StepResult {
        success: true,
        message: format!("Applied {} links to {} files ({} recommendations, {} skipped)", links_added, files_modified, links_to_add.len(),
            skipped_missing_source + skipped_missing_target + skipped_unknown_slug + skipped_source_not_found + skipped_read_error + skipped_already_linked),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}
