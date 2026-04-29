use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;
use rusqlite::Connection;

/// Native Rust scan for `cluster_and_link_scan` step.
///
/// Reads articles.json from the automation dir, resolves the content directory,
/// and calls `content::linking::scan_links()`.  Returns the scan result as JSON
/// so the downstream agentic step has concrete link-graph data to work with.
pub(crate) fn exec_cluster_link_scan(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
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
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to load articles from DB: {}", e),
                output: None,
            }
        }
    };

    if articles.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No articles in app index — nothing to scan".to_string(),
            output: Some(
                r#"{"total_articles":0,"total_internal_links":0,"orphan_ids":[],"profiles":[]}"#
                    .to_string(),
            ),
        };
    }

    // Locate the content directory via the standard locator (project override → heuristics)
    let resolution = crate::content::locator::resolve(&paths.repo_root, None);

    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: "Could not locate content directory — set content_dir in project config"
                    .to_string(),
                output: None,
            }
        }
    };

    log::info!(
        "[cluster_link_scan] scanning {} articles in {}",
        articles.len(),
        content_dir.display()
    );

    match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(result) => {
            let json = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

            // Persist to link_scan.json so the downstream strategy step can read it.
            let scan_path = paths.automation_dir.join("link_scan.json");
            if let Err(e) = std::fs::write(&scan_path, &json) {
                log::warn!("[cluster_link_scan] failed to write link_scan.json: {}", e);
            }

            crate::engine::workflows::StepResult {
                success: true,
                message: format!(
                    "Link scan complete: {} articles, {} internal links, {} orphans",
                    result.total_articles,
                    result.total_internal_links,
                    result.orphan_ids.len()
                ),
                output: Some(json),
            }
        }
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Link scan failed: {}", e),
            output: None,
        },
    }
}

/// Create a `cluster_and_link` follow-up task after a successful `write_article`.
///
/// De-duplicates: if an active `cluster_and_link` task already exists for this
/// project, no second task is created.
pub(crate) fn create_cluster_and_link_task(
    conn: &Connection,
    parent_task: &Task,
    _project_path: &str,
) -> Option<String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority};

    let parent_title = parent_task
        .title
        .as_deref()
        .unwrap_or("new article")
        .trim_start_matches("Write article: ");

    let title = format!("Cluster and link: {}", parent_title);
    let description = format!(
        "Scan internal link graph and add missing hub-to-spoke, \
         spoke-to-hub, and cross-cluster links following the article: {}. \
         Depends on: {}",
        parent_title, parent_task.id,
    );

    // Use spawn with custom idempotency key to allow specific execution_mode and agent_policy
    let idempotency_key = format!("followup:{}:cluster_and_link:{}", parent_task.id, title);

    let spec = TaskSpec {
        project_id: parent_task.project_id.clone(),
        task_type: "cluster_and_link".to_string(),
        title: Some(title),
        description: Some(description),
        phase: Some("implementation".to_string()),
        execution_mode: Some(ExecutionMode::Automatic),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        artifacts: vec![],
        depends_on: vec![parent_task.id.clone()],
    };

    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[cluster_link] spawned cluster_and_link task {} after write_article {}",
                task.id,
                parent_task.id
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!(
                "[cluster_link] failed to create cluster_and_link task: {}",
                e
            );
            None
        }
    }
}

/// Step 2 for `cluster_and_link`: structured agentic step that interprets the
/// scan output and recommends specific links to add across MDX files.
///
/// Input: `link_scan.json` (written by step 1) + `articles.json`
///
/// Output contract:
/// ```json
/// {
///   "generated_at": "<ISO>",
///   "links_to_add": [
///     {
///       "source_article_id": <number>,
///       "source_file": "<basename.mdx>",
///       "target_article_id": <number>,
///       "target_title": "<title>",
///       "target_slug": "<url-slug>",
///       "reason": "<one sentence>"
///     }
///   ]
/// }
/// ```
///
/// Cannot be deterministic: deciding which cross-cluster links are valuable
/// and which orphans are topically related requires understanding article
/// content and business priorities — not just graph connectivity counts.
#[allow(deprecated)]
pub(crate) fn exec_cluster_link_strategy(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // --- Load scan output ---
    let scan_path = paths.automation_dir.join("link_scan.json");
    let scan: serde_json::Value =
        match crate::engine::exec::common::read_json(&scan_path, "link_scan.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    // --- Load articles from SQLite for title/slug map ---
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
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
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to load articles from DB: {}", e),
                output: None,
            }
        }
    };

    // --- Build prompt context ---
    let total = scan["total_articles"].as_u64().unwrap_or(0);
    let with_out = scan["articles_with_outgoing"].as_u64().unwrap_or(0);
    let with_inc = scan["articles_with_incoming"].as_u64().unwrap_or(0);
    let orphan_ids: Vec<i64> = scan["orphan_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    // Compact article index (id, title, slug, file) — cap at 100 to keep prompt bounded
    let mut index_entries: Vec<serde_json::Value> = articles
        .iter()
        .map(|a| {
            let file_basename = std::path::Path::new(&a.file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&a.file)
                .to_string();
            serde_json::json!({
                "id": a.id,
                "title": a.title,
                "slug": a.url_slug,
                "file": file_basename,
            })
        })
        .collect();
    index_entries.truncate(100);
    let index_json = serde_json::to_string(&index_entries).unwrap_or_default();

    // Profiles for under-connected articles (incoming < 2) — cap at 40
    let empty_profiles: Vec<serde_json::Value> = Vec::new();
    let profiles_arr = scan["profiles"].as_array().unwrap_or(&empty_profiles);
    let under_connected: Vec<&serde_json::Value> = profiles_arr
        .iter()
        .filter(|p| {
            let incoming = p["incoming_ids"].as_array().map(|a| a.len()).unwrap_or(0);
            incoming < 2
        })
        .take(40)
        .collect();
    let under_json = serde_json::to_string(&under_connected).unwrap_or_default();
    let orphan_list_json = serde_json::to_string(&orphan_ids).unwrap_or_default();

    let prompt = format!(
        r#"You are an SEO specialist analysing the internal link structure of a blog.

## Link graph summary
- Total articles: {total}
- Articles with at least one outgoing link: {with_out}
- Articles with at least one incoming link: {with_inc}
- Orphan article IDs (no links in or out): {orphan_list_json}

## Article index (id, title, url slug, file)

{index_json}

## Under-connected articles (fewer than 2 incoming links — needs more links pointing TO them)

{under_json}

## Task

Identify the top 20 most valuable internal links to add. Priorities:
1. Give every orphan article at least one incoming link from a thematically related article.
2. Link hub articles (broad topics) DOWN to relevant spoke articles.
3. Link spoke articles UP to their parent hub when relevant.

Return ONLY a valid JSON object — no markdown fences, no commentary.

Output schema:
{{
  "generated_at": "<ISO-8601 timestamp>",
  "links_to_add": [
    {{
      "source_article_id": <number — the article whose MDX file will receive the new link>,
      "source_file": "<exact basename.mdx from the article index>",
      "target_article_id": <number>,
      "target_title": "<exact title from the article index>",
      "target_slug": "<exact slug from the article index>",
      "reason": "<one sentence explaining the topical connection>"
    }}
  ]
}}

Requirements:
- Maximum 20 entries in links_to_add.
- Only suggest links that make genuine topical sense.
- Each entry adds a link IN the source article TO the target article at URL /blog/<target_slug>.
- Use exact slugs and titles from the article index above.
"#,
    );

    log::info!(
        "[cluster_link_strategy] running agent ({} chars prompt, {} articles, {} orphans, provider={})",
        prompt.len(), total, orphan_ids.len(), agent_provider
    );

    let raw_output = match crate::engine::agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(out) => out,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Agent failed: {}", e),
                output: None,
            }
        }
    };

    let links_json = crate::engine::text::extract_json(&raw_output).unwrap_or_else(|| {
        serde_json::json!({
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "links_to_add": [],
        })
    });

    // Persist to links_to_add.json for the apply step
    let links_path = paths.automation_dir.join("links_to_add.json");
    let links_str = serde_json::to_string_pretty(&links_json).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&links_path, &links_str) {
        log::warn!(
            "[cluster_link_strategy] failed to write links_to_add.json: {}",
            e
        );
    }

    let link_count = links_json["links_to_add"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Link strategy complete: {} links recommended across {} articles",
            link_count, total
        ),
        output: Some(serde_json::to_string_pretty(&links_json).unwrap_or_default()),
    }
}

/// Step 3 for `cluster_and_link`: deterministic apply step that writes the
/// recommended "Related Articles" sections to MDX files.
///
/// Reads `links_to_add.json` produced by the strategy step, groups links by
/// source article, and appends a `## Related Articles` section to each MDX
/// file that does not already have one.
pub(crate) fn exec_cluster_link_apply(
    _task: &Task,
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

    // Locate content directory
    let resolution = crate::content::locator::resolve(repo_root, None);
    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: "Could not locate content directory".to_string(),
                output: None,
            }
        }
    };

    // Group links by source_file basename: source_file → vec[(title, slug)]
    let mut by_source: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for link in links_to_add {
        let source_file = link["source_file"].as_str().unwrap_or("").to_string();
        let target_title = link["target_title"].as_str().unwrap_or("").to_string();
        let target_slug = link["target_slug"].as_str().unwrap_or("").to_string();
        if source_file.is_empty() || target_slug.is_empty() {
            continue;
        }
        by_source
            .entry(source_file)
            .or_default()
            .push((target_title, target_slug));
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

    for (source_basename, new_links) in &by_source {
        let Some(file_path) = file_map.get(source_basename) else {
            log::warn!(
                "[cluster_link_apply] source file not found in content dir: {}",
                source_basename
            );
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
                continue;
            }
        };

        // Skip if a "Related Articles" section already exists
        let has_related = content.lines().any(|l| {
            let t = l.trim();
            t.starts_with("##") && t.to_lowercase().contains("related")
        });
        if has_related {
            log::info!(
                "[cluster_link_apply] {} already has Related Articles section — skipping",
                source_basename
            );
            continue;
        }

        // Build section, skipping slugs already present in the file
        let mut section = String::from("\n\n## Related Articles\n\n");
        let mut added_in_file = 0usize;
        for (title, slug) in new_links {
            if content.contains(slug.as_str()) {
                log::info!(
                    "[cluster_link_apply] {} already links to /blog/{} — skipping",
                    source_basename,
                    slug
                );
                continue;
            }
            section.push_str(&format!("- [{}](/blog/{})\n", title, slug));
            added_in_file += 1;
        }

        if added_in_file == 0 {
            continue;
        }

        let new_content = format!("{}{}", content.trim_end(), section);
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

    let summary = serde_json::json!({
        "files_modified": files_modified,
        "links_added": links_added,
        "changes": change_log,
    });
    crate::engine::workflows::StepResult {
        success: true,
        message: format!("Applied {} links to {} files", links_added, files_modified),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}
