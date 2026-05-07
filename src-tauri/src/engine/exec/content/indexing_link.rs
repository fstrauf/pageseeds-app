/// Per-target internal link fix execution module.
///
/// Each `fix_indexing_internal_links` task carries a single target in its
/// `indexing_link_target` artifact. The four steps are:
///   1. indexing_link_context  — deterministic: build target + source shortlist
///   2. indexing_link_plan     — agentic: choose source and anchor from shortlist
///   3. indexing_link_apply    — deterministic: append Related Articles link
///   4. indexing_link_verify   — deterministic: prove target gained inbound links
use std::collections::HashMap;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

// ─── Step 1: Context ──────────────────────────────────────────────────────────

/// Build a compact per-target context artifact from the task's target data,
/// current link scan, and source file excerpts.
pub(crate) fn exec_indexing_link_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Parse target artifact
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
    let target_keyword = target_data["target_keyword"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Load link scan — trigger fresh scan if missing or stale (>1 hour)
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_scan: Option<serde_json::Value> = {
        let stale = match std::fs::metadata(&link_scan_path) {
            Ok(m) => m
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs() > 3600)
                .unwrap_or(true),
            Err(_) => true,
        };
        let fresh_scan = if stale {
            log::info!("[indexing_link_context] link_scan.json missing or stale — triggering fresh scan");
            let repo_root = std::path::Path::new(project_path);
            if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
                if let Ok(articles) = crate::content::article_index::list_articles(&db, &task.project_id) {
                    let articles: Vec<_> = articles.into_iter().filter(|a| !a.file.is_empty()).collect();
                    if let Some(content_dir) = crate::content::locator::resolve(repo_root, None).selected {
                        if let Ok(scan_result) = crate::content::linking::scan_links(&content_dir, &articles) {
                            let scan_json = serde_json::to_string_pretty(&scan_result).unwrap_or_default();
                            let _ = std::fs::write(&link_scan_path, &scan_json);
                            serde_json::from_str(&scan_json).ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        fresh_scan.or_else(|| {
            std::fs::read_to_string(&link_scan_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
    };

    // Find target profile
    let target_profile = link_scan
        .as_ref()
        .and_then(|v| v["profiles"].as_array())
        .and_then(|profiles| {
            profiles
                .iter()
                .find(|p| p["id"].as_i64() == Some(target_article_id))
                .cloned()
        });

    let current_incoming_ids: Vec<i64> = target_profile
        .as_ref()
        .and_then(|p| p["incoming_ids"].as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let current_outgoing_ids: Vec<i64> = target_profile
        .as_ref()
        .and_then(|p| p["outgoing_ids"].as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    // Build source context from source_candidates in the artifact
    let source_candidates = target_data["source_candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut sources: Vec<serde_json::Value> = Vec::new();

    for candidate in &source_candidates {
        let source_id = candidate["article_id"].as_i64().unwrap_or(0);
        let source_slug = candidate["slug"].as_str().unwrap_or("").to_string();
        let source_file = candidate["file"].as_str().unwrap_or("").to_string();

        // Check if already links to target (outgoing_ids is Vec<i64>)
        let already_links = link_scan
            .as_ref()
            .and_then(|v| v["profiles"].as_array())
            .and_then(|profiles| {
                profiles
                    .iter()
                    .find(|p| p["id"].as_i64() == Some(source_id))
                    .and_then(|p| {
                        p["outgoing_ids"].as_array().map(|outgoing| {
                            outgoing
                                .iter()
                                .any(|o| o.as_i64() == Some(target_article_id))
                        })
                    })
            })
            .unwrap_or(false);

        sources.push(serde_json::json!({
            "article_id": source_id,
            "title": candidate["title"],
            "slug": source_slug,
            "file": source_file,
            "gsc_impressions": candidate["gsc_impressions"],
            "score": candidate["score"],
            "already_links_to_target": already_links,
        }));
    }

    let context = serde_json::json!({
        "target": {
            "article_id": target_article_id,
            "title": target_data["title"],
            "slug": target_slug,
            "url": target_data["url"],
            "target_keyword": target_keyword,
            "current_incoming_ids": current_incoming_ids,
            "current_outgoing_ids": current_outgoing_ids,
        },
        "sources": sources,
    });

    StepResult {
        success: true,
        message: format!(
            "Context built for target {}: {} incoming, {} source candidates",
            target_slug,
            current_incoming_ids.len(),
            sources.len()
        ),
        output: Some(context.to_string()),
    }
}

// ─── Step 2: Plan ─────────────────────────────────────────────────────────────

/// Agentic step: choose the best source and anchor text from the shortlist.
///
/// V1 uses the existing prompt-based agent pattern (not Rig extraction)
/// to keep the implementation simple and proven.
pub(crate) fn exec_indexing_link_plan(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    use std::path::Path;
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Parse target artifact
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

    let target_slug = crate::content::slug::normalize_url_slug(target_data["slug"].as_str().unwrap_or(""));
    let target_url = target_data["url"].as_str().unwrap_or("").to_string();
    let target_keyword = target_data["target_keyword"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let reason_code = target_data["reason_code"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Load context from previous step (or rebuild from artifact)
    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_link_context")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: re-run context logic
            let ctx_result = exec_indexing_link_context(task, project_path);
            ctx_result.output.clone()
        });

    let context: serde_json::Value = match context_json {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => serde_json::json!({}),
    };

    let sources = context["sources"].as_array().cloned().unwrap_or_default();
    if sources.is_empty() {
        return StepResult {
            success: false,
            message: "No source candidates available for this target".to_string(),
            output: None,
        };
    }

    // Build compact prompt
    let sources_json = serde_json::to_string(&sources).unwrap_or_default();
    let prompt = format!(
        r#"You are an SEO specialist choosing the best internal link to add.

## Target page
- URL: {target_url}
- Slug: {target_slug}
- Keyword: {target_keyword}
- Issue: {reason_code}

## Candidate source pages (already filtered for relevance)
{sources_json}

## Task
Choose exactly ONE source page from the candidate list above and decide:
1. Which source page should link to the target.
2. What anchor text to use (should naturally include or relate to the target keyword).

Return ONLY a valid JSON object — no markdown fences, no commentary.

Output schema:
{{
  "links_to_add": [
    {{
      "source_article_id": <number>,
      "target_article_id": <number>,
      "anchor_text": "<natural anchor text>",
      "target_slug": "{target_slug}",
      "placement": "related_section",
      "reason": "<one sentence explaining why this source and anchor were chosen>"
    }}
  ]
}}

Requirements:
- Only ONE link in links_to_add.
- Choose from the candidate sources above.
- Do NOT pick a source where already_links_to_target is true.
- placement must be "related_section" in V1.
"#,
    );

    let raw_output = match crate::engine::agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(out) => out,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Agent failed: {}", e),
                output: None,
            }
        }
    };

    let plan_json = crate::engine::text::extract_json(&raw_output).unwrap_or_else(|| {
        serde_json::json!({
            "links_to_add": [],
        })
    });

    // Validate: ensure we got exactly one link
    let link_count = plan_json["links_to_add"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    if link_count == 0 {
        return StepResult {
            success: false,
            message: "Agent returned no link recommendations".to_string(),
            output: None,
        };
    }

    // Persist plan for apply step
    let plan_path = paths
        .automation_dir
        .join(format!("indexing_link_plan_{}.json", task.id));
    let _ = std::fs::write(
        &plan_path,
        serde_json::to_string_pretty(&plan_json).unwrap_or_default(),
    );

    StepResult {
        success: true,
        message: format!(
            "Link plan: {} link recommended for {}",
            link_count, target_slug
        ),
        output: Some(plan_json.to_string()),
    }
}

// ─── Step 3: Apply ────────────────────────────────────────────────────────────

/// Deterministic apply: append a Related Articles link or insert a contextual
/// paragraph link to the chosen source file.
///
/// Uses snapshot/rollback for safety. If validation fails after a
/// contextual_paragraph edit, the original file is restored.
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
            success: false,
            message: "No links to apply — plan is empty".to_string(),
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
    let mut snapshots: Vec<crate::content::snapshot::FileSnapshot> = Vec::new();

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

        // Create snapshot before editing
        let snapshot = match crate::content::snapshot::snapshot_file(file_path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(
                    "[indexing_link_apply] failed to snapshot {}: {}",
                    file_path.display(),
                    e
                );
                links_failed += 1;
                continue;
            }
        };

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
                snapshots.push(snapshot);
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

    let snapshot_json: Vec<serde_json::Value> = snapshots
        .iter()
        .map(|s| {
            serde_json::json!({
                "original_path": s.original_path.to_string_lossy(),
                "backup_path": s.backup_path.to_string_lossy(),
            })
        })
        .collect();

    let summary = serde_json::json!({
        "target_slug": target_slug,
        "links_added": links_added,
        "links_skipped_existing": links_skipped_existing,
        "links_failed": links_failed,
        "source_files_modified": files_modified,
        "snapshots": snapshot_json,
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

fn apply_related_section_link(content: &str, anchor_text: &str, target_slug: &str) -> String {
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
fn insert_contextual_link(content: &str, anchor_text: &str, target_slug: &str) -> Option<String> {
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

// ─── Step 4: Verify ───────────────────────────────────────────────────────────

/// Rescan the link graph and verify the target gained at least one inbound link.
pub(crate) fn exec_indexing_link_verify(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = std::path::Path::new(project_path);

    // Parse target artifact
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
    let incoming_before = target_data["incoming_link_count_before"]
        .as_u64()
        .unwrap_or(0) as usize;

    // Re-scan link graph
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open DB for verification: {}", e),
                output: None,
            }
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a
            .into_iter()
            .filter(|a| !a.file.is_empty())
            .collect::<Vec<_>>(),
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load articles: {}", e),
                output: None,
            }
        }
    };

    let content_dir = match crate::content::locator::resolve(repo_root, None).selected {
        Some(d) => d,
        None => {
            return StepResult {
                success: false,
                message: "Could not locate content directory for verification".to_string(),
                output: None,
            }
        }
    };

    let scan_result = match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Link scan failed during verification: {}", e),
                output: None,
            }
        }
    };

    // Update link_scan.json
    let scan_json = serde_json::to_string_pretty(&scan_result).unwrap_or_default();
    let scan_path = paths.automation_dir.join("link_scan.json");
    if let Err(e) = std::fs::write(&scan_path, &scan_json) {
        log::warn!(
            "[indexing_link_verify] failed to write link_scan.json: {}",
            e
        );
    }

    // Find target's new incoming count
    let target_profile = scan_result
        .profiles
        .iter()
        .find(|p| p.id == target_article_id);

    let incoming_after = target_profile.map(|p| p.incoming_ids.len()).unwrap_or(0);

    let links_added = if incoming_after > incoming_before {
        incoming_after - incoming_before
    } else {
        0
    };

    // Also check that at least one source file contains the target slug
    // Check source files modified by reading the link scan profiles
    // We need to find profiles that now have outgoing links to the target slug
    let source_files_modified: Vec<String> = scan_result
        .profiles
        .iter()
        .filter(|p| {
            // Re-read file content to check if it links to target
            // This is a simple heuristic: check if the file contains the target slug link
            let file_path = content_dir.join(&p.file);
            std::fs::read_to_string(&file_path)
                .ok()
                .map(|content| content.contains(&crate::content::slug::format_blog_link(&target_slug)))
                .unwrap_or(false)
        })
        .map(|p| p.file.clone())
        .collect();

    let passed = incoming_after > incoming_before;

    // Commit or rollback snapshots based on verification result
    let snapshot_actions = handle_snapshots(task, passed);

    let verification = serde_json::json!({
        "target_article_id": target_article_id,
        "target_slug": target_slug,
        "incoming_link_count_before": incoming_before,
        "incoming_link_count_after": incoming_after,
        "links_added": links_added,
        "source_files_modified": source_files_modified,
        "passed": passed,
        "snapshot_actions": snapshot_actions,
    });

    StepResult {
        success: passed,
        message: if passed {
            format!(
                "Verification passed: target {} gained {} inbound link(s) ({} → {}). {}",
                target_slug, links_added, incoming_before, incoming_after, snapshot_actions
            )
        } else {
            format!(
                "Verification FAILED: target {} still has {} inbound link(s) (expected > {}). {}",
                target_slug, incoming_after, incoming_before, snapshot_actions
            )
        },
        output: Some(verification.to_string()),
    }
}

/// Commit snapshots on success, rollback on failure.
/// Reads snapshot paths from the indexing_link_apply artifact.
fn handle_snapshots(task: &Task, passed: bool) -> String {
    let apply_artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_link_apply");

    let snapshots = match apply_artifact {
        Some(a) => match a.content.as_ref() {
            Some(c) => match serde_json::from_str::<serde_json::Value>(c) {
                Ok(v) => v["snapshots"].as_array().cloned().unwrap_or_default(),
                Err(_) => return "No snapshot info found in apply artifact".to_string(),
            },
            None => return "No snapshot info found in apply artifact".to_string(),
        },
        None => return "No apply artifact found".to_string(),
    };

    let mut committed = 0usize;
    let mut rolled_back = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for snap in &snapshots {
        let original = snap["original_path"].as_str().unwrap_or("");
        let backup = snap["backup_path"].as_str().unwrap_or("");
        if original.is_empty() || backup.is_empty() {
            continue;
        }

        let snapshot = crate::content::snapshot::FileSnapshot {
            original_path: std::path::PathBuf::from(original),
            backup_path: std::path::PathBuf::from(backup),
            created_at: String::new(),
        };

        if passed {
            match crate::content::snapshot::commit_snapshot(&snapshot) {
                Ok(_) => committed += 1,
                Err(e) => errors.push(format!("commit {}: {}", original, e)),
            }
        } else {
            match crate::content::snapshot::rollback_file(&snapshot) {
                Ok(_) => rolled_back += 1,
                Err(e) => errors.push(format!("rollback {}: {}", original, e)),
            }
        }
    }

    if !errors.is_empty() {
        log::warn!("[handle_snapshots] errors: {}", errors.join("; "));
    }

    if passed {
        format!("Committed {} snapshot(s)", committed)
    } else {
        format!("Rolled back {} snapshot(s)", rolled_back)
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn parse_target_artifact(task: &Task) -> Option<serde_json::Value> {
    task.artifacts
        .iter()
        .find(|a| a.key == "indexing_link_target")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v.get("target").cloned())
}

fn find_file_by_article_id(project_id: &str, article_id: i64) -> Option<String> {
    let db_path = crate::db::default_db_path();
    let db = rusqlite::Connection::open(&db_path).ok()?;
    let articles = crate::content::article_index::list_articles(&db, project_id).ok()?;
    articles
        .into_iter()
        .find(|a| a.id == article_id)
        .map(|a| a.file)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_related_section_link_creates_new_section() {
        let content = "# Hello\n\nThis is a paragraph.\n";
        let result = apply_related_section_link(content, "Example Page", "example-page");
        assert!(result.contains("## Related Articles"));
        assert!(result.contains("[Example Page](/blog/example-page)"));
    }

    #[test]
    fn apply_related_section_link_appends_to_existing_section() {
        let content = "# Hello\n\nThis is a paragraph.\n\n## Related Articles\n\n- [Another](/blog/another)\n";
        let result = apply_related_section_link(content, "Example Page", "example-page");
        assert!(result.contains("[Another](/blog/another)"));
        assert!(result.contains("[Example Page](/blog/example-page)"));
        // Should not create a second Related Articles section
        let section_count = result.matches("## Related Articles").count();
        assert_eq!(section_count, 1);
    }

    #[test]
    fn insert_contextual_link_finds_relevant_paragraph() {
        let content = "# Machine Learning Guide\n\nMachine learning is a subset of artificial intelligence.\n\nBaking cakes is a fun hobby.\n";
        let result = insert_contextual_link(content, "machine learning tutorial", "ml-tutorial");
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.contains("[machine learning tutorial](/blog/ml-tutorial)"));
        // The link should be in the ML paragraph, not the baking paragraph
        let lines: Vec<&str> = result.lines().collect();
        let ml_line = lines
            .iter()
            .position(|l| l.contains("Machine learning"))
            .unwrap();
        let baking_line = lines
            .iter()
            .position(|l| l.contains("Baking cakes"))
            .unwrap();
        assert!(lines[ml_line].contains("ml-tutorial"));
        assert!(!lines[baking_line].contains("ml-tutorial"));
    }

    #[test]
    fn insert_contextual_link_falls_back_to_longest_paragraph() {
        let content = "# Baking Guide\n\nBaking cakes is a fun hobby that many people enjoy on weekends with their families and friends.\n\nChocolate is delicious.\n";
        let result = insert_contextual_link(content, "machine learning tutorial", "ml-tutorial");
        // No keyword match, but falls back to the longest substantial paragraph (>80 chars)
        assert!(result.is_some(), "should fall back to longest paragraph");
        let result = result.unwrap();
        // The longest paragraph gets the link
        assert!(result.contains("Baking cakes"));
        assert!(result.contains("ml-tutorial"));
    }

    #[test]
    fn parse_target_artifact_extracts_target() {
        let task = Task {
            id: "task-1".to_string(),
            project_id: "proj-1".to_string(),
            task_type: "fix_indexing_internal_links".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::BackendAuto,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "indexing_link_target".to_string(),
                path: None,
                artifact_type: None,
                source: None,
                content: Some(r#"{"target": {"slug": "test-page", "article_id": 42}}"#.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        };

        let target = parse_target_artifact(&task);
        assert!(target.is_some());
        let target = target.unwrap();
        assert_eq!(target["slug"].as_str(), Some("test-page"));
        assert_eq!(target["article_id"].as_i64(), Some(42));
    }

    #[test]
    fn parse_target_artifact_returns_none_for_missing_key() {
        let task = Task {
            id: "task-1".to_string(),
            project_id: "proj-1".to_string(),
            task_type: "fix_indexing_internal_links".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::BackendAuto,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        };

        assert!(parse_target_artifact(&task).is_none());
    }

    #[test]
    fn normalize_link_slug_strips_prefixes() {
        use crate::content::slug::normalize_url_slug;
        assert_eq!(normalize_url_slug("my-post"), "my-post");
        assert_eq!(normalize_url_slug("blog/my-post"), "my-post");
        assert_eq!(normalize_url_slug("/blog/my-post"), "my-post");
        assert_eq!(normalize_url_slug("tools/blog/my-post"), "my-post");
        // Double numeric prefix (date + sequence) — must be fully stripped
        assert_eq!(
            normalize_url_slug("2025-08-01-the-good-enough-mindset"),
            "the-good-enough-mindset"
        );
        assert_eq!(
            normalize_url_slug("01-the-good-enough-mindset"),
            "the-good-enough-mindset"
        );
    }

    /// End-to-end test: apply a link to the real learnedlate repo file and verify
    /// scan_links detects it. This exercises the full apply → verify path.
    #[test]
    #[ignore = "requires filesystem + DB"] // run with: cargo test -- --ignored
    fn apply_and_verify_on_real_file() {
        let project_path = "/Users/fstrauf/01_code/learnedlate";
        let content_dir = std::path::Path::new(project_path).join("src/blog/posts");
        let source_file = content_dir.join("070_product_management_for_non_technical_founders_a_practical_guide.mdx");

        // Read original content
        let original = std::fs::read_to_string(&source_file).expect("read source");

        // Apply link using the fixed function
        let modified = apply_related_section_link(&original, "the good enough mindset", "the-good-enough-mindset");

        // Sanity: the link line must contain proper markdown ()
        assert!(
            modified.contains("[the good enough mindset](/blog/the-good-enough-mindset)"),
            "link must be properly formatted markdown"
        );

        // Write to a temp copy so we don't mutate the repo
        let temp_dir = std::path::PathBuf::from(format!("/tmp/test_link_fix_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let temp_file = temp_dir.join("070_product_management_for_non_technical_founders_a_practical_guide.mdx");
        std::fs::write(&temp_file, &modified).expect("write temp");

        // Also copy the target file so scan_links can build profiles for it
        let target_file = content_dir.join("2025-08-01-the-good-enough-mindset.mdx");
        let temp_target = temp_dir.join("2025-08-01-the-good-enough-mindset.mdx");
        std::fs::copy(&target_file, &temp_target).expect("copy target");

        // Load articles from DB (need at least source + target)
        let db_path = crate::db::default_db_path();
        let db = rusqlite::Connection::open(&db_path).expect("open db");
        let articles: Vec<crate::models::article::Article> = crate::content::article_index::list_articles(&db, "learnedlate")
            .expect("list articles")
            .into_iter()
            .filter(|a| {
                a.file.contains("070_product_management")
                    || a.file.contains("2025-08-01-the-good-enough-mindset")
            })
            .collect();

        assert!(
            articles.iter().any(|a| a.id == 70),
            "source article 70 must be in DB"
        );
        assert!(
            articles.iter().any(|a| a.id == 19),
            "target article 19 must be in DB"
        );

        // Scan links in the temp dir
        let scan_result = crate::content::linking::scan_links(&temp_dir, &articles)
            .expect("scan_links");

        // Find target profile
        let target_profile = scan_result.profiles.iter().find(|p| p.id == 19);
        let incoming_after = target_profile.map(|p| p.incoming_ids.len()).unwrap_or(0);

        assert!(
            incoming_after > 0,
            "target article 19 must have >0 inbound links after apply; found {}. Profiles: {:?}",
            incoming_after,
            scan_result.profiles
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
