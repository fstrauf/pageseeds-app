use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

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
            return crate::engine::workflows::StepResult::fail(format!("Failed to open app database: {}", e))
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a
            .into_iter()
            .filter(|a| !a.file.is_empty())
            .collect::<Vec<_>>(),
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Failed to load articles from DB: {}", e))
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
            artifact_key: None,
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
        return crate::engine::workflows::StepResult::fail(format!(
                "Prompt size ({} bytes) exceeds hard budget ({} bytes) for cluster_link_strategy. \
                 The link graph is too large. Try reducing the number of articles or running \
                 cluster_and_link in smaller batches.",
                prompt.len(),
                PROMPT_HARD_BUDGET
            ));
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
            return crate::engine::workflows::StepResult::fail(format!("Agent failed: {}", e))
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
        artifact_key: None,
    }
}
