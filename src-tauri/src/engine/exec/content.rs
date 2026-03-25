/// Content review and sync execution module.
///
/// Covers:
///   - exec_content_review_apply   (apply agent-generated recommendations to MDX files)
///   - exec_content_sync           (sync articles.json ↔ MDX files)
///   - exec_content_review_recommend (select priority articles + run agent)
///   - exec_cluster_link_scan      (native Rust internal-link scan for cluster_and_link step 1)
///   - exec_cluster_link_strategy  (agentic: interpret scan, recommend links to add, write links_to_add.json)
///   - exec_cluster_link_apply     (deterministic: write "Related Articles" sections to MDX files)
///   - select_priority_articles    (scoring formula)
///   - build_review_context        (structured context for LLM)
///   - build_review_prompt         (prompt assembly)
///   - create_content_review_apply_task (auto-spawn follow-up task)
///   - create_cluster_and_link_task    (auto-spawn follow-up task after write_article)

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

/// Execute the `content_review_apply` task.
///
/// Reads the `recommendations` artifact embedded in the task, builds a
/// structured prompt, and runs one agent call.
pub(crate) fn exec_content_review_apply(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;

    let rec_content = task.artifacts.iter()
        .find(|a| a.key == "recommendations")
        .and_then(|a| a.content.as_deref())
        .unwrap_or("");

    if rec_content.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "No recommendations artifact found on this task — re-run Content Review to regenerate it".to_string(),
            output: None,
        };
    }

    let paths = ProjectPaths::from_path(project_path);
    let articles_json = paths.automation_dir.join("articles.json");
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let prompt = format!(
        r#"Apply content improvements to article files.

Repo root: {project_path}
Articles registry: {articles_json}
Today's date: {today}

## Recommendations

{rec_content}

## Your job

1. Read the full recommendations object above. It contains a list of articles under the "articles" key.

2. For each article:
   a. Read the source file (article_file field, relative to {project_path})
   b. Apply every suggestion in that article's "suggestions" list:
      - title / h1: update frontmatter title and/or the first H1 heading
      - meta_description: update the description field in frontmatter
      - intro: rewrite the opening paragraph(s) as specified
      - internal_links: add the suggested links at appropriate places in the body
      - faq: add or expand the FAQ section with the suggested Q&As
      - eeat: add the credibility signal described
      - cta: add or strengthen the call-to-action as described
   c. Save the updated file
   d. In {articles_json}, find the article by id and set:
      - review_status → "reviewed"
      - last_reviewed_at → {today}

3. Report one line per article summarising what changed.

Work through every article. Make all changes directly. Do not ask questions.

IMPORTANT:
- Do NOT run git add, git commit, git push, or any git command.
- When updating {articles_json}, use python3 with `json.load` / `json.dump` and pass `sort_keys=False` so the existing key order is preserved. Only update the review_status and last_reviewed_at fields — do not reformat or reorder the file."#,
        project_path = project_path,
        articles_json = articles_json.display(),
        today = today,
        rec_content = rec_content,
    );

    log::info!(
        "[content_review_apply] running agent (provider={}, prompt_chars={})",
        agent_provider, prompt.len()
    );

    match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => crate::engine::workflows::StepResult {
            success: true,
            message: format!("Apply complete ({} chars output)", output.len()),
            output: Some(output),
        },
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Agent failed: {}", e),
            output: None,
        },
    }
}

/// Native Rust implementation of `pageseeds content sync-and-validate`.
pub(crate) fn exec_content_sync(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::content::ops::sync_and_validate;

    log::info!("[content_sync] starting for project={} path={}", task.project_id, project_path);

    let paths = ProjectPaths::from_path(project_path);
    match sync_and_validate(&paths.automation_dir, &paths.repo_root, false) {
        Ok(result) => {
            let output = serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| format!("{:?}", result));
            let ok = result.missing_files.is_empty() && result.malformed_file_refs.is_empty();
            crate::engine::workflows::StepResult {
                success: ok,
                message: format!("content_sync: {} — {}", if ok { "OK" } else { "issues found" }, result.next_action),
                output: Some(output),
            }
        }
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("content_sync failed: {}", e),
            output: None,
        },
    }
}

/// Port of `_select_priority_articles` from the PageSeeds CLI.
///
/// Scores each article against a tiered formula and returns the top `max_items`
/// candidates sorted by score descending.
///
/// Scoring:
///   +1000  position 5–20, impressions > 200, CTR < 3%  (quick CTR wins)
///   +700   health == "poor"                             (needs improvement)
///   +15×N  checks_failed × 15                          (weak content quality)
///   +∆     max(0, 100 − health_score)                  (inverse health)
///   −600   position 1–4 and CTR ≥ 5%                   (already strong)
pub(crate) fn select_priority_articles(
    raw_articles: &[serde_json::Value],
    audit_articles: &[serde_json::Value],
    max_items: usize,
) -> Vec<serde_json::Value> {
    let mut audit_by_file: std::collections::HashMap<String, &serde_json::Value> = Default::default();
    let mut audit_by_slug: std::collections::HashMap<String, &serde_json::Value> = Default::default();
    for a in audit_articles {
        if let Some(f) = a["file"].as_str() {
            if !f.is_empty() { audit_by_file.insert(f.to_string(), a); }
        }
        if let Some(s) = a["url_slug"].as_str() {
            if !s.is_empty() { audit_by_slug.insert(s.to_string(), a); }
        }
    }

    let null_value = serde_json::Value::Null;
    let mut candidates: Vec<(i64, serde_json::Value)> = Vec::new();

    for article in raw_articles {
        let status = article["status"].as_str().unwrap_or("").to_lowercase();
        let review_status = article["review_status"].as_str().unwrap_or("").to_lowercase();
        let file_rel = article["file"].as_str().unwrap_or("").to_string();
        if status == "draft" || review_status == "in_review" || file_rel.is_empty() {
            continue;
        }

        let gsc = &article["gsc"];
        let pos = gsc["avg_position"].as_f64().unwrap_or(0.0);
        let impressions = gsc["impressions"].as_f64().unwrap_or(0.0);
        let ctr = gsc["ctr"].as_f64().unwrap_or(0.0);

        let url_slug = article["url_slug"].as_str().unwrap_or("");
        let audit_row: &serde_json::Value = audit_by_file.get(&file_rel)
            .or_else(|| audit_by_slug.get(url_slug))
            .copied()
            .unwrap_or(&null_value);

        let health = audit_row["health"].as_str().unwrap_or("").to_lowercase();
        let checks_failed = audit_row["checks_failed"].as_i64().unwrap_or(0);
        let health_score = audit_row["health_score"].as_i64().unwrap_or(0);

        let failed_checks: Vec<serde_json::Value> = audit_row["checks"].as_object()
            .map(|checks| {
                checks.iter()
                    .filter(|(_, v)| v["pass"].as_bool() == Some(false))
                    .map(|(k, v)| serde_json::json!({
                        "check_id": k,
                        "label": v["label"].as_str().unwrap_or(k),
                    }))
                    .collect()
            })
            .unwrap_or_default();

        let mut score: i64 = 0;
        if pos >= 5.0 && pos <= 20.0 && impressions > 200.0 && ctr < 0.03 {
            score += 1000;
        }
        if health == "poor" {
            score += 700;
        }
        score += checks_failed * 15;
        score += (100 - health_score).max(0);
        if pos >= 1.0 && pos <= 4.0 && ctr >= 0.05 {
            score -= 600;
        }
        if score <= 0 {
            continue;
        }

        let mut enriched = article.clone();
        enriched["_failed_checks"] = serde_json::json!(failed_checks);
        candidates.push((score, enriched));
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().take(max_items).map(|(_, a)| a).collect()
}

/// Build a structured context payload for the LLM.
///
/// For each selected article, reads the first `max_excerpt_chars` of the source
/// MDX file so the agent has concrete content — not just check names.
pub(crate) fn build_review_context(
    selected: &[serde_json::Value],
    repo_root: &std::path::Path,
    max_excerpt_chars: usize,
) -> serde_json::Value {
    let now = chrono::Utc::now().to_rfc3339();
    let articles: Vec<serde_json::Value> = selected.iter().filter_map(|article| {
        let file_ref = article["file"].as_str().unwrap_or("");
        if file_ref.is_empty() { return None; }
        let source = crate::engine::exec::utils::read_source_file(repo_root, file_ref);
        let source_excerpt = source.as_deref()
            .map(|s| s.char_indices().nth(max_excerpt_chars).map_or(s, |(i, _)| &s[..i]))
            .unwrap_or("")
            .to_string();
        Some(serde_json::json!({
            "article_id": article["id"],
            "article_title": article["title"],
            "article_file": file_ref,
            "url_slug": article["url_slug"],
            "target_keyword": article["target_keyword"],
            "gsc_snapshot": article["gsc"],
            "failed_checks": article["_failed_checks"],
            "source_excerpt": source_excerpt,
        }))
    }).collect();
    serde_json::json!({
        "generated_at": now,
        "articles": articles,
    })
}

/// Build the structured agent prompt for the content review recommendations step.
pub(crate) fn build_review_prompt(context: &serde_json::Value) -> String {
    let context_json = serde_json::to_string_pretty(context).unwrap_or_default();
    format!(
        r#"Generate SEO recommendations JSON from the provided article context.

Return ONLY one valid JSON object. No markdown fences, no commentary.

Input context:
{context_json}

Output schema:
{{
  "generated_at": "<ISO>",
  "total_articles": <N>,
  "articles": [
    {{
      "article_id": <id>,
      "article_title": "<title>",
      "article_file": "<path>",
      "url_slug": "<slug>",
      "target_keyword": "<keyword>",
      "gsc_snapshot": {{}},
      "failed_checks": [],
      "suggestions": [
        {{
          "category": "title|meta_description|intro|h1|internal_links|faq|eeat|cta",
          "current": "<what's there now>",
          "proposed": "<specific replacement>",
          "reason": "<one sentence why>"
        }}
      ]
    }}
  ]
}}

Requirements:
- 4-8 actionable suggestions per article.
- Use only the provided context.
- Preserve article metadata fields exactly from input."#,
        context_json = context_json,
    )
}

/// Step runner for `content_review_recommend` steps.
///
/// 1. Reads content_audit.json + articles.json
/// 2. Selects top 5 priority articles via `select_priority_articles`
/// 3. Builds structured context with source excerpts
/// 4. Makes one agent call with a targeted structured prompt
/// 5. Writes recommendations.json to the automation dir
pub(crate) fn exec_content_review_recommend(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    let audit_path = paths.automation_dir.join("content_audit.json");
    let audit_str = match std::fs::read_to_string(&audit_path) {
        Ok(s) => s,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("content_audit.json not found — run content audit first: {}", e),
            output: None,
        },
    };
    let audit: serde_json::Value = match serde_json::from_str(&audit_str) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to parse content_audit.json: {}", e),
            output: None,
        },
    };

    let articles_path = paths.automation_dir.join("articles.json");
    let articles_str = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("articles.json not found: {}", e),
            output: None,
        },
    };
    let articles_doc: serde_json::Value = match serde_json::from_str(&articles_str) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to parse articles.json: {}", e),
            output: None,
        },
    };

    let empty_vec: Vec<serde_json::Value> = Vec::new();
    let raw_articles = if articles_doc.is_array() {
        articles_doc.as_array().unwrap_or(&empty_vec)
    } else {
        articles_doc.get("articles").and_then(|v| v.as_array()).unwrap_or(&empty_vec)
    };
    let audit_articles = audit.get("articles").and_then(|v| v.as_array()).unwrap_or(&empty_vec);

    let selected = select_priority_articles(raw_articles, audit_articles, 5);
    log::info!(
        "[content_review_recommend] {} priority articles selected (project={})",
        selected.len(), task.project_id
    );

    if selected.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No eligible articles found for review — all healthy or already in-review".to_string(),
            output: Some(serde_json::json!({
                "generated_at": chrono::Utc::now().to_rfc3339(),
                "total_articles": 0,
                "articles": []
            }).to_string()),
        };
    }

    let context = build_review_context(&selected, repo_root, 2600);
    let n_context = context["articles"].as_array().map(|a| a.len()).unwrap_or(0);
    log::info!("[content_review_recommend] context built for {} articles", n_context);

    if n_context == 0 {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "Could not read source files for selected articles — check file paths in articles.json".to_string(),
            output: None,
        };
    }

    let prompt = build_review_prompt(&context);
    log::info!(
        "[content_review_recommend] running agent ({} chars prompt, provider={})",
        prompt.len(), agent_provider
    );

    let raw_output = match crate::engine::agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(out) => out,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Agent failed: {}", e),
            output: None,
        },
    };

    let normalized = crate::engine::normalizer::normalize_agent_output(&raw_output);
    let rec = normalized.json_artifact.unwrap_or_else(|| serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "total_articles": 0,
        "articles": [],
    }));

    let rec_path = paths.automation_dir.join("recommendations.json");
    let rec_str = serde_json::to_string_pretty(&rec).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&rec_path, &rec_str) {
        log::warn!("[content_review_recommend] failed to write recommendations.json: {}", e);
    } else {
        let n = rec["articles"].as_array().map(|a| a.len()).unwrap_or(0);
        log::info!("[content_review_recommend] wrote recommendations.json ({} articles)", n);
    }

    let article_count = rec["articles"].as_array().map(|a| a.len()).unwrap_or(0);
    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Recommendations generated for {} / {} selected articles",
            article_count, selected.len()
        ),
        output: Some(serde_json::to_string_pretty(&rec).unwrap_or_default()),
    }
}

/// After a successful content review, create a single `content_review_apply` task
/// pointing at the recommendations.json written by `exec_content_review_recommend`.
///
/// Skips if recommendations.json is absent (review found nothing) or if an apply
/// task is already pending.
pub(crate) fn create_content_review_apply_task(conn: &Connection, parent_task: &Task, project_path: &str) -> Option<String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{TaskArtifact, ExecutionMode, Priority, AgentPolicy};

    let paths = ProjectPaths::from_path(project_path);
    let rec_path = paths.automation_dir.join("recommendations.json");

    let rec_str = match std::fs::read_to_string(&rec_path) {
        Ok(s) => s,
        Err(_) => {
            log::info!("[create_apply_task] recommendations.json not found — no apply task created");
            return None;
        }
    };
    let rec: serde_json::Value = match serde_json::from_str(&rec_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[create_apply_task] failed to parse recommendations.json: {}", e);
            return None;
        }
    };

    let article_count = rec["articles"].as_array().map(|a| a.len()).unwrap_or(0);
    if article_count == 0 {
        log::info!("[create_apply_task] no articles in recommendations — skipping");
        return None;
    }

    let title = if article_count == 1 {
        let name = rec["articles"][0]["article_title"].as_str().unwrap_or("article");
        format!("Apply review fixes: {}", name)
    } else {
        format!("Apply review fixes: {} articles", article_count)
    };

    let rec_rel = rec_path
        .strip_prefix(project_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| rec_path.to_string_lossy().to_string());

    let artifact = TaskArtifact {
        key: "recommendations".to_string(),
        path: Some(rec_rel),
        artifact_type: Some("json".to_string()),
        source: Some("content_review".to_string()),
        content: Some(rec_str),
    };

    // Idempotency key: content_review_apply:{project_id}
    // This ensures only one apply task per project is created
    let idempotency_key = format!("content_review_apply:{}", parent_task.project_id);

    let spec = TaskSpec {
        project_id: parent_task.project_id.clone(),
        task_type: "content_review_apply".to_string(),
        title: Some(title),
        description: Some(format!(
            "Apply SEO recommendations from recommendations.json to {} article(s). \
             The recommendations artifact contains specific suggestions per article \
             (title, meta description, intro, H1, internal links, etc.).",
            article_count
        )),
        phase: Some("implementation".to_string()),
        execution_mode: Some(ExecutionMode::Manual),
        priority: Priority::High,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        artifacts: vec![artifact],
        depends_on: vec![],
    };

    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[create_apply_task] created {} for {} article(s)",
                task.id, article_count
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!("[create_apply_task] failed to create apply task: {}", e);
            None
        }
    }
}

/// Native Rust scan for `cluster_and_link_scan` step.
///
/// Reads articles.json from the automation dir, resolves the content directory,
/// and calls `content::linking::scan_links()`.  Returns the scan result as JSON
/// so the downstream agentic step has concrete link-graph data to work with.
pub(crate) fn exec_cluster_link_scan(
    _task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let articles_path = paths.automation_dir.join("articles.json");

    let articles_str = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("articles.json not found — run a content sync first: {}", e),
                output: None,
            }
        }
    };

    let articles_doc: serde_json::Value = match serde_json::from_str(&articles_str) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to parse articles.json: {}", e),
                output: None,
            }
        }
    };

    // Support both bare array and {articles:[...]} envelope
    let article_values: Vec<serde_json::Value> = if articles_doc.is_array() {
        articles_doc.as_array().cloned().unwrap_or_default()
    } else {
        articles_doc
            .get("articles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };

    let articles: Vec<crate::models::article::Article> = article_values
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .filter(|a: &crate::models::article::Article| !a.file.is_empty())
        .collect();

    if articles.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No articles in articles.json — nothing to scan".to_string(),
            output: Some(r#"{"total_articles":0,"total_internal_links":0,"orphan_ids":[],"profiles":[]}"#.to_string()),
        };
    }

    // Locate the content directory via the standard locator (project override → heuristics)
    let resolution = crate::content::locator::resolve(&paths.repo_root, None);

    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: "Could not locate content directory — set content_dir in project config".to_string(),
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
            let json = serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| "{}".to_string());

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
    use crate::models::task::{ExecutionMode, Priority, AgentPolicy};

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
        parent_title,
        parent_task.id,
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
            log::warn!("[cluster_link] failed to create cluster_and_link task: {}", e);
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
pub(crate) fn exec_cluster_link_strategy(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // --- Load scan output ---
    let scan_path = paths.automation_dir.join("link_scan.json");
    let scan_str = match std::fs::read_to_string(&scan_path) {
        Ok(s) => s,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("link_scan.json not found — run the scan step first: {}", e),
                output: None,
            }
        }
    };
    let scan: serde_json::Value = match serde_json::from_str(&scan_str) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to parse link_scan.json: {}", e),
                output: None,
            }
        }
    };

    // --- Load articles for title/slug map ---
    let articles_path = paths.automation_dir.join("articles.json");
    let articles_str = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("articles.json not found: {}", e),
                output: None,
            }
        }
    };
    let articles_doc: serde_json::Value = match serde_json::from_str(&articles_str) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to parse articles.json: {}", e),
                output: None,
            }
        }
    };

    let empty_vec: Vec<serde_json::Value> = Vec::new();
    let article_values = if articles_doc.is_array() {
        articles_doc.as_array().unwrap_or(&empty_vec)
    } else {
        articles_doc.get("articles").and_then(|v| v.as_array()).unwrap_or(&empty_vec)
    };

    let articles: Vec<crate::models::article::Article> = article_values
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .filter(|a: &crate::models::article::Article| !a.file.is_empty())
        .collect();

    // --- Build prompt context ---
    let total = scan["total_articles"].as_u64().unwrap_or(0);
    let with_out = scan["articles_with_outgoing"].as_u64().unwrap_or(0);
    let with_inc = scan["articles_with_incoming"].as_u64().unwrap_or(0);
    let orphan_ids: Vec<i64> = scan["orphan_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    // Compact article index (id, title, slug, file) — cap at 100 to keep prompt bounded
    let mut index_entries: Vec<serde_json::Value> = articles.iter().map(|a| {
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
    }).collect();
    index_entries.truncate(100);
    let index_json = serde_json::to_string(&index_entries).unwrap_or_default();

    // Profiles for under-connected articles (incoming < 2) — cap at 40
    let empty_profiles: Vec<serde_json::Value> = Vec::new();
    let profiles_arr = scan["profiles"].as_array().unwrap_or(&empty_profiles);
    let under_connected: Vec<&serde_json::Value> = profiles_arr.iter().filter(|p| {
        let incoming = p["incoming_ids"].as_array().map(|a| a.len()).unwrap_or(0);
        incoming < 2
    }).take(40).collect();
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

    let normalized = crate::engine::normalizer::normalize_agent_output(&raw_output);
    let links_json = normalized.json_artifact.unwrap_or_else(|| serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "links_to_add": [],
    }));

    // Persist to links_to_add.json for the apply step
    let links_path = paths.automation_dir.join("links_to_add.json");
    let links_str = serde_json::to_string_pretty(&links_json).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&links_path, &links_str) {
        log::warn!("[cluster_link_strategy] failed to write links_to_add.json: {}", e);
    }

    let link_count = links_json["links_to_add"].as_array().map(|a| a.len()).unwrap_or(0);
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
    let links_str = match std::fs::read_to_string(&links_path) {
        Ok(s) => s,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("links_to_add.json not found — run strategy step first: {}", e),
                output: None,
            }
        }
    };
    let links_doc: serde_json::Value = match serde_json::from_str(&links_str) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to parse links_to_add.json: {}", e),
                output: None,
            }
        }
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
        by_source.entry(source_file).or_default().push((target_title, target_slug));
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
            log::warn!("[cluster_link_apply] source file not found in content dir: {}", source_basename);
            continue;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[cluster_link_apply] cannot read {}: {}", file_path.display(), e);
                continue;
            }
        };

        // Skip if a "Related Articles" section already exists
        let has_related = content.lines().any(|l| {
            let t = l.trim();
            t.starts_with("##") && t.to_lowercase().contains("related")
        });
        if has_related {
            log::info!("[cluster_link_apply] {} already has Related Articles section — skipping", source_basename);
            continue;
        }

        // Build section, skipping slugs already present in the file
        let mut section = String::from("\n\n## Related Articles\n\n");
        let mut added_in_file = 0usize;
        for (title, slug) in new_links {
            if content.contains(slug.as_str()) {
                log::info!("[cluster_link_apply] {} already links to /blog/{} — skipping", source_basename, slug);
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
                let link_entries: Vec<serde_json::Value> = new_links.iter()
                    .map(|(t, s)| serde_json::json!({"title": t, "slug": s}))
                    .collect();
                change_log.push(serde_json::json!({
                    "file": source_basename,
                    "links_added": added_in_file,
                    "links": link_entries,
                }));
                log::info!("[cluster_link_apply] {} — added {} Related Articles links", source_basename, added_in_file);
            }
            Err(e) => log::warn!("[cluster_link_apply] failed to write {}: {}", file_path.display(), e),
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
