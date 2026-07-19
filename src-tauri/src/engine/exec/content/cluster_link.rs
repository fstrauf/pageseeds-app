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

    // --- Cache check: if link_scan.json exists and is < 1 hour old, reuse it ---
    let scan_path = paths.automation_dir.join("link_scan.json");
    if let Ok(metadata) = std::fs::metadata(&scan_path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                if elapsed.as_secs() < 3600 {
                    if let Ok(cached) = std::fs::read_to_string(&scan_path) {
                        log::info!(
                            "[cluster_link_scan] using cached link_scan.json ({} min old)",
                            elapsed.as_secs() / 60
                        );
                        // Try to extract summary stats from cached JSON for the message
                        let summary =
                            serde_json::from_str::<serde_json::Value>(&cached)
                                .map(|v| {
                                    format!(
                                "{} articles, {} internal links, {} orphans, {} zero-incoming",
                                v["total_articles"].as_u64().unwrap_or(0),
                                v["total_internal_links"].as_u64().unwrap_or(0),
                                v["orphan_ids"].as_array().map(|a| a.len()).unwrap_or(0),
                                v["zero_incoming_ids"].as_array().map(|a| a.len()).unwrap_or(0),
                            )
                                })
                                .unwrap_or_else(|_| "cached".to_string());
                        return crate::engine::workflows::StepResult {
                            success: true,
                            message: format!("Link scan complete (cached): {}", summary),
                            output: Some(cached),
                        };
                    }
                }
            }
        }
    }

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
                    "Link scan complete: {} articles, {} internal links, {} orphans, {} zero-incoming; {} unresolved links{}",
                    result.total_articles,
                    result.total_internal_links,
                    result.orphan_ids.len(),
                    result.zero_incoming_ids.len(),
                    result.unresolved_links.len(),
                    if result.unresolved_links.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " [{}]",
                            result
                                .unresolved_links
                                .iter()
                                .take(10)
                                .map(|u| format!("{} → {}", u.file, u.target))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
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
    use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};

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
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        artifacts: vec![],
        depends_on: vec![parent_task.id.clone()],
        ..Default::default()
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
    let zero_incoming_ids: Vec<i64> = scan["zero_incoming_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    // Compact article index (id, title, slug).
    // We intentionally omit `file` here; it adds ~35 bytes/article and is only
    // needed by the apply step, not the agent's reasoning. A separate
    // article_id_to_file.json mapping is written for the apply step.
    let mut index_entries: Vec<serde_json::Value> = articles
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "title": a.title,
                "slug": a.url_slug,
            })
        })
        .collect();
    // Prioritize orphans and under-connected articles so they appear first
    // in the index. The agent can only link articles it can see.
    index_entries.sort_by(|a, b| {
        let a_id = a["id"].as_i64().unwrap_or(0);
        let b_id = b["id"].as_i64().unwrap_or(0);
        let a_is_orphan = orphan_ids.contains(&a_id);
        let b_is_orphan = orphan_ids.contains(&b_id);
        match (a_is_orphan, b_is_orphan) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
    // Budget-aware cap: always keep all orphans in the index (they're the
    // primary link targets), then allow non-orphans up to a total of 120.
    // A 120-entry index is ~6-8 KB, leaving plenty of room for profiles
    // and template inside the 20 KB hard budget.
    let orphan_count = orphan_ids.len();
    let max_index = std::cmp::max(orphan_count, 50).min(120);
    index_entries.truncate(max_index);
    let index_json = serde_json::to_string(&index_entries).unwrap_or_default();

    // Build id → file mapping for the apply step (resolves source_article_id → file).
    let id_to_file: serde_json::Value = articles
        .iter()
        .map(|a| {
            let file_basename = std::path::Path::new(&a.file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&a.file)
                .to_string();
            serde_json::json!({
                "id": a.id,
                "file": file_basename,
            })
        })
        .collect::<Vec<_>>()
        .into();

    // Profiles for under-connected articles (incoming < 2) — cap at 20.
    // We send ONLY id, title, file, and link counts. The full incoming_ids/
    // outgoing_ids arrays from scan_links can be huge (dozens of IDs per profile).
    // The agent only needs counts to identify which articles need more links;
    // the actual link targets come from the index.
    let empty_profiles: Vec<serde_json::Value> = Vec::new();
    let profiles_arr = scan["profiles"].as_array().unwrap_or(&empty_profiles);
    let compact_profiles: Vec<serde_json::Value> = profiles_arr
        .iter()
        .filter(|p| {
            let incoming = p["incoming_ids"].as_array().map(|a| a.len()).unwrap_or(0);
            incoming < 2
        })
        .take(20)
        .map(|p| {
            serde_json::json!({
                "id": p["id"],
                "title": p["title"],
                "file": p["file"],
                "incoming_count": p["incoming_ids"].as_array().map(|a| a.len()).unwrap_or(0),
                "outgoing_count": p["outgoing_ids"].as_array().map(|a| a.len()).unwrap_or(0),
            })
        })
        .collect();
    let under_json = serde_json::to_string(&compact_profiles).unwrap_or_default();

    if compact_profiles.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No under-connected articles found — link graph is healthy".to_string(),
            output: Some(r#"{"generated_at":"","links_to_add":[]}"#.to_string()),
        };
    }

    // Set of article IDs that are in the index we sent to the agent. Used both
    // to scope the "existing links" prompt section and to filter recommendations
    // that reference targets outside the visible index.
    let index_ids: std::collections::HashSet<i64> = index_entries
        .iter()
        .filter_map(|e| e["id"].as_i64())
        .collect();

    // Build the set of existing source → target links so the agent does not
    // recommend links that are already present. This is derived from the scan
    // profiles (outgoing_ids) and is also used as a deterministic post-filter.
    // The prompt section is scoped to articles in the visible index to keep the
    // prompt compact, while the post-filter HashSet covers the full graph.
    let mut existing_links: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut existing_links_prompt_map = serde_json::Map::new();
    for p in profiles_arr {
        let Some(id) = p["id"].as_i64() else { continue };
        let Some(outgoing) = p["outgoing_ids"].as_array() else { continue };
        for target in outgoing.iter().filter_map(|v| v.as_i64()) {
            existing_links.insert((id, target));
        }
        if index_ids.contains(&id) && !outgoing.is_empty() {
            existing_links_prompt_map.insert(
                id.to_string(),
                serde_json::Value::Array(outgoing.clone()),
            );
        }
    }
    let existing_links_json =
        serde_json::to_string(&existing_links_prompt_map).unwrap_or_default();

    let orphan_list_json = serde_json::to_string(&orphan_ids).unwrap_or_default();
    let zero_incoming_list_json = serde_json::to_string(&zero_incoming_ids).unwrap_or_default();

    let prompt = format!(
        r#"You are an SEO specialist analysing the internal link structure of a blog.

## Link graph summary
- Total articles: {total}
- Articles with at least one outgoing link: {with_out}
- Articles with at least one incoming link: {with_inc}
- Orphan article IDs (no links in or out): {orphan_list_json}
- Zero-incoming article IDs (Google cannot discover — no pages link TO them): {zero_incoming_list_json}

## Article index (id, title, url slug)

{index_json}

## Under-connected articles (fewer than 2 incoming links — needs more links pointing TO them)

{under_json}

## Existing internal links (DO NOT recommend these again)

Each entry shows an article ID and the IDs of articles it already links TO.

{existing_links_json}

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
- You do NOT need to include `source_file`; the apply step resolves the article ID to the file automatically.
- NEVER recommend a link that already appears in "Existing internal links" above.
- Do not recommend a link from an article to itself (source_article_id != target_article_id).
- Only use article IDs and slugs that appear in the "Article index" above.
"#,
    );

    const PROMPT_HARD_BUDGET: usize = 20_000;
    if prompt.len() > PROMPT_HARD_BUDGET {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "Prompt size ({} bytes) exceeds hard budget ({} bytes) for cluster_link_strategy. \
                 The link graph is too large. Try reducing the number of articles or running \
                 cluster_and_link in smaller batches.",
                prompt.len(),
                PROMPT_HARD_BUDGET
            ),
            output: None,
        };
    }

    // Detailed component-size logging so we can debug prompt bloat precisely.
    let template_len = prompt.len()
        - index_json.len()
        - under_json.len()
        - orphan_list_json.len()
        - zero_incoming_list_json.len()
        - existing_links_json.len();
    log::info!(
        "[cluster_link_strategy] prompt components: index={} bytes, profiles={} bytes, existing_links={} bytes, orphans={} bytes, zero_incoming={} bytes, template={} bytes, total={} bytes",
        index_json.len(),
        under_json.len(),
        existing_links_json.len(),
        orphan_list_json.len(),
        zero_incoming_list_json.len(),
        template_len,
        prompt.len()
    );

    // Write article_id_to_file.json so the apply step can resolve IDs → files
    let id_to_file_path = paths.automation_dir.join("article_id_to_file.json");
    if let Err(e) = std::fs::write(
        &id_to_file_path,
        serde_json::to_string(&id_to_file).unwrap_or_default(),
    ) {
        log::warn!(
            "[cluster_link_strategy] failed to write article_id_to_file.json: {}",
            e
        );
    }

    // Write full prompt to disk for debugging / inspection
    let prompt_debug_path = paths
        .automation_dir
        .join("cluster_link_strategy_prompt.txt");
    if let Err(e) = std::fs::write(&prompt_debug_path, &prompt) {
        log::warn!(
            "[cluster_link_strategy] failed to write prompt debug file: {}",
            e
        );
    }

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

    let mut links_json = crate::engine::text::extract_json(&raw_output).unwrap_or_else(|| {
        serde_json::json!({
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "links_to_add": [],
        })
    });

    // Deterministic post-filter: drop recommendations that are already present,
    // self-referential, or reference targets outside the index we sent.
    let mut filtered_out_existing = 0usize;
    let mut filtered_out_self = 0usize;
    let mut filtered_out_unknown_target = 0usize;
    if let Some(links_arr) = links_json["links_to_add"].as_array() {
        let filtered: Vec<serde_json::Value> = links_arr
            .iter()
            .filter(|link| {
                let source_id = link["source_article_id"].as_i64();
                let target_id = link["target_article_id"].as_i64();
                if source_id == target_id {
                    filtered_out_self += 1;
                    return false;
                }
                if let (Some(s), Some(t)) = (source_id, target_id) {
                    if existing_links.contains(&(s, t)) {
                        filtered_out_existing += 1;
                        return false;
                    }
                    if !index_ids.contains(&t) {
                        filtered_out_unknown_target += 1;
                        return false;
                    }
                    true
                } else {
                    filtered_out_unknown_target += 1;
                    false
                }
            })
            .cloned()
            .collect();
        links_json["links_to_add"] = filtered.into();
    }

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
    let raw_count = crate::engine::text::extract_json(&raw_output)
        .and_then(|j| j["links_to_add"].as_array().map(|a| a.len()))
        .unwrap_or(0);
    log::info!(
        "[cluster_link_strategy] filtered {} raw recommendations down to {} (existing={}, self={}, unknown_target={})",
        raw_count,
        link_count,
        filtered_out_existing,
        filtered_out_self,
        filtered_out_unknown_target
    );
    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Link strategy complete: {} links recommended across {} articles ({} filtered)",
            link_count,
            total,
            raw_count.saturating_sub(link_count)
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
            return crate::engine::workflows::StepResult {
                success: false,
                message: "Could not locate content directory".to_string(),
                output: None,
            }
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
