use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

/// Step 3 for `cluster_and_link`: deterministic apply step that writes the
/// recommended "Related Articles" sections to MDX files.
///
/// Reads `links_to_add.json` produced by the strategy step, groups links by
/// source article, and appends a `## Related Articles` section to each MDX
/// file that does not already have one.
///
/// Always re-scans residual orphans / zero-incoming after finishing — even when
/// zero files were modified — so post-actions can decide residual follow-ups
/// honestly (issue #196).
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
        let residuals = rescan_link_residuals(task, project_path);
        let summary = serde_json::json!({
            "files_modified": 0,
            "links_added": 0,
            "changes": [],
            "orphans_remaining": residuals.orphans_remaining,
            "zero_incoming_remaining": residuals.zero_incoming_remaining,
            "focus_still_zero_incoming": residuals.focus_still_zero_incoming,
            "skipped": {
                "missing_source_mapping": 0,
                "missing_target_slug": 0,
                "unknown_target_slug": 0,
                "source_file_not_found": 0,
                "source_file_read_error": 0,
                "link_already_exists": 0,
            },
            "recommendations_count": 0,
        });
        return crate::engine::workflows::StepResult {
            success: true,
            message: format!(
                "No links to add — strategy found no gaps (orphans_remaining={}, zero_incoming_remaining={})",
                residuals.orphans_remaining, residuals.zero_incoming_remaining
            ),
            output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
            artifact_key: None,
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

    // Always re-scan residuals (including when files_modified == 0) so follow-up
    // decisions and desk reports stay honest after a no-op apply.
    let residuals = rescan_link_residuals(task, project_path);

    let summary = serde_json::json!({
        "files_modified": files_modified,
        "links_added": links_added,
        "changes": change_log,
        "orphans_remaining": residuals.orphans_remaining,
        "zero_incoming_remaining": residuals.zero_incoming_remaining,
        "focus_still_zero_incoming": residuals.focus_still_zero_incoming,
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
        message: format!("Applied {} links to {} files ({} recommendations, {} skipped); residuals: orphans={}, zero_incoming={}", links_added, files_modified, links_to_add.len(),
            skipped_missing_source + skipped_missing_target + skipped_unknown_slug + skipped_source_not_found + skipped_read_error + skipped_already_linked,
            residuals.orphans_remaining, residuals.zero_incoming_remaining),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
        artifact_key: None,
    }
}

/// Residual counts from a post-apply (or no-op) link graph re-scan.
#[derive(Debug, Clone, Default)]
pub(crate) struct LinkResiduals {
    pub orphans_remaining: i32,
    pub zero_incoming_remaining: i32,
    /// Set only when the task has a `focus_slug` artifact: true if that article
    /// still has zero inbound `/blog/` links after re-scan.
    pub focus_still_zero_incoming: Option<bool>,
}

/// Re-scan the MDX link graph and persist `link_scan.json`.
///
/// Always runs when called — callers use this even after zero file mods so
/// residual debt is honest for post_actions follow-up decisions.
pub(crate) fn rescan_link_residuals(task: &Task, project_path: &str) -> LinkResiduals {
    use std::path::Path;

    let mut residuals = LinkResiduals::default();
    let focus_slug = super::task_focus_slug(task);

    let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) else {
        log::warn!("[cluster_link_apply] failed to open DB for re-scan");
        return residuals;
    };
    let Ok(articles_raw) = crate::content::article_index::list_articles(&db, &task.project_id)
    else {
        log::warn!("[cluster_link_apply] failed to load articles for re-scan");
        return residuals;
    };
    let articles: Vec<_> = articles_raw
        .into_iter()
        .filter(|a| !a.file.is_empty())
        .collect();
    let resolution = crate::content::locator::resolve(Path::new(project_path), None);
    let Some(content_dir) = resolution.selected else {
        log::warn!("[cluster_link_apply] could not locate content dir for re-scan");
        return residuals;
    };

    match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(result) => {
            let json = serde_json::to_string_pretty(&result).unwrap_or_default();
            let paths = ProjectPaths::from_path(project_path);
            let scan_path = paths.automation_dir.join("link_scan.json");
            if let Err(e) = std::fs::write(&scan_path, &json) {
                log::warn!(
                    "[cluster_link_apply] failed to write updated link_scan.json: {}",
                    e
                );
            } else {
                log::info!(
                    "[cluster_link_apply] re-scanned and saved link_scan.json: {} articles, {} orphans, {} zero-incoming",
                    result.total_articles,
                    result.orphan_ids.len(),
                    result.zero_incoming_ids.len()
                );
            }
            residuals.orphans_remaining = result.orphan_ids.len() as i32;
            residuals.zero_incoming_remaining = result.zero_incoming_ids.len() as i32;

            if focus_slug.is_some() {
                let focus_id = super::resolve_focus_article_id(focus_slug.as_deref(), &articles);
                // Focus zero-incoming is graph truth from profiles — not residual
                // discovery lists (which exclude drafts). Post-write focus may
                // still be draft and must still drive must-cover re-rounds.
                residuals.focus_still_zero_incoming = Some(match focus_id {
                    Some(id) => result
                        .profiles
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.incoming_ids.is_empty())
                        .unwrap_or(true),
                    // Unknown focus slug: treat as still-zero when any residual zero-incoming exists
                    None => residuals.zero_incoming_remaining > 0,
                });
            }
        }
        Err(e) => {
            log::warn!("[cluster_link_apply] re-scan failed: {}", e);
        }
    }
    residuals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskReviewSurface, TaskRun,
        TaskRunPolicy, TaskStatus,
    };
    use std::fs;
    use uuid::Uuid;

    fn make_task(project_id: &str, focus_slug: Option<&str>) -> Task {
        let mut artifacts = vec![];
        if let Some(slug) = focus_slug {
            artifacts.push(TaskArtifact {
                key: "focus_slug".to_string(),
                path: None,
                artifact_type: Some("text".to_string()),
                source: Some("write_spawn".to_string()),
                content: Some(slug.to_string()),
            });
        }
        Task {
            id: format!("task-{}", Uuid::new_v4()),
            task_type: "cluster_and_link".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::InProgress,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::BackendAuto,
            agent_policy: AgentPolicy::Required,
            title: Some("Cluster and link: test".to_string()),
            description: None,
            project_id: project_id.to_string(),
            depends_on: vec![],
            artifacts,
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    /// Origin B: apply with empty recommendations / zero file mods must still
    /// report real residual counts when the graph has orphans/zero-incoming.
    #[test]
    fn apply_empty_links_still_reports_honest_residuals() {
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("pageseeds-cl-apply-{}", Uuid::new_v4()));
        let content_dir = dir.join("content").join("blog");
        let automation = dir.join(".github").join("automation");
        fs::create_dir_all(&content_dir).unwrap();
        fs::create_dir_all(&automation).unwrap();

        // Two disconnected articles → 2 orphans, 2 zero-incoming.
        fs::write(
            content_dir.join("1_hub.mdx"),
            "---\ntitle: \"Hub\"\n---\n\n# Hub\n\nBody.\n",
        )
        .unwrap();
        fs::write(
            content_dir.join("2_spoke.mdx"),
            "---\ntitle: \"Spoke\"\n---\n\n# Spoke\n\nBody.\n",
        )
        .unwrap();

        fs::write(
            automation.join("links_to_add.json"),
            r#"{"generated_at":"","links_to_add":[]}"#,
        )
        .unwrap();

        let db_path = dir.join("test.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            crate::db::init_with_conn(&conn).unwrap();
            conn.execute(
                "INSERT INTO projects (id, name, path, active, project_mode)
                 VALUES ('proj1', 'Test', ?1, 1, 'workspace')",
                rusqlite::params![dir.to_string_lossy().as_ref()],
            )
            .unwrap();
            for (id, title, slug, file) in [
                (1i64, "Hub", "hub", "content/blog/1_hub.mdx"),
                (2i64, "Spoke", "spoke", "content/blog/2_spoke.mdx"),
            ] {
                // Published so residual discovery debt lists include them.
                conn.execute(
                    "INSERT INTO articles (
                        id, project_id, title, url_slug, file, status,
                        content_gaps_addressed, target_volume, word_count, review_count
                     ) VALUES (?1, 'proj1', ?2, ?3, ?4, 'published', '[]', 0, 0, 0)",
                    rusqlite::params![id, title, slug, file],
                )
                .unwrap();
            }
        }

        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        std::env::set_var("PAGESEEDS_DB_PATH", &db_path);

        let task = make_task("proj1", Some("spoke"));
        let result = exec_cluster_link_apply(&task, dir.to_string_lossy().as_ref());

        match old_db {
            Some(v) => std::env::set_var("PAGESEEDS_DB_PATH", v),
            None => std::env::remove_var("PAGESEEDS_DB_PATH"),
        }
        let _ = fs::remove_dir_all(&dir);

        assert!(result.success, "apply failed: {}", result.message);
        let out: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(out["files_modified"].as_u64(), Some(0));
        assert_eq!(out["links_added"].as_u64(), Some(0));
        assert_eq!(out["orphans_remaining"].as_i64(), Some(2));
        assert_eq!(out["zero_incoming_remaining"].as_i64(), Some(2));
        assert_eq!(out["focus_still_zero_incoming"].as_bool(), Some(true));
    }

    #[test]
    fn apply_rejects_unknown_target_slug_via_valid_link_targets() {
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("pageseeds-cl-apply-unk-{}", Uuid::new_v4()));
        let content_dir = dir.join("content").join("blog");
        let automation = dir.join(".github").join("automation");
        fs::create_dir_all(&content_dir).unwrap();
        fs::create_dir_all(&automation).unwrap();

        fs::write(
            content_dir.join("1_hub.mdx"),
            "---\ntitle: \"Hub\"\n---\n\n# Hub\n\nBody.\n",
        )
        .unwrap();
        fs::write(
            automation.join("links_to_add.json"),
            r#"{"links_to_add":[{"source_article_id":1,"target_article_id":99,"target_title":"Ghost","target_slug":"ghost-redirected","reason":"test"}]}"#,
        )
        .unwrap();
        fs::write(
            automation.join("article_id_to_file.json"),
            r#"[{"id":1,"file":"1_hub.mdx"}]"#,
        )
        .unwrap();

        let db_path = dir.join("test.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            crate::db::init_with_conn(&conn).unwrap();
            conn.execute(
                "INSERT INTO projects (id, name, path, active, project_mode)
                 VALUES ('proj1', 'Test', ?1, 1, 'workspace')",
                rusqlite::params![dir.to_string_lossy().as_ref()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO articles (
                    id, project_id, title, url_slug, file, status,
                    content_gaps_addressed, target_volume, word_count, review_count
                 ) VALUES (1, 'proj1', 'Hub', 'hub', 'content/blog/1_hub.mdx', 'draft', '[]', 0, 0, 0)",
                [],
            )
            .unwrap();
        }

        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        std::env::set_var("PAGESEEDS_DB_PATH", &db_path);

        let task = make_task("proj1", None);
        let result = exec_cluster_link_apply(&task, dir.to_string_lossy().as_ref());

        match old_db {
            Some(v) => std::env::set_var("PAGESEEDS_DB_PATH", v),
            None => std::env::remove_var("PAGESEEDS_DB_PATH"),
        }
        let _ = fs::remove_dir_all(&dir);

        assert!(result.success, "apply failed: {}", result.message);
        let out: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(out["files_modified"].as_u64(), Some(0));
        assert_eq!(out["links_added"].as_u64(), Some(0));
        assert_eq!(out["skipped"]["unknown_target_slug"].as_u64(), Some(1));
    }
}
