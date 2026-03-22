/// Workflow execution orchestrator.
///
/// Finds the correct handler for a task, plans the step graph,
/// executes each step sequentially, persists artifacts, and
/// updates task status in SQLite.

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::engine::workflows::{
    handlers::{default_handlers, exec_agentic, exec_deterministic},
    WorkflowStep,
};
use crate::engine::task_store;
use crate::models::task::{Task, TaskArtifact};

// ─── Public Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepProgress {
    pub step_name: String,
    pub kind: String,
    pub status: String, // "pending" | "running" | "ok" | "failed" | "skipped"
    pub message: String,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub task_id: String,
    pub success: bool,
    pub message: String,
    pub steps: Vec<StepProgress>,
    pub started_at: String,
    pub finished_at: String,
}

// ─── Engine ───────────────────────────────────────────────────────────────────

pub fn execute_task(conn: &Connection, task_id: &str) -> Result<ExecutionResult, String> {
    let mut task = task_store::get_task(conn, task_id).map_err(|e| e.to_string())?;

    let started_at = Utc::now().to_rfc3339();

    // Transition to in_progress
    if task.status == "todo" {
        task.status = "in_progress".to_string();
        task.updated_at = started_at.clone();
        task_store::update_task_status(conn, task_id, "in_progress").map_err(|e| e.to_string())?;
    }

    let (project_path, site_url, agent_provider) = {
        let project = task_store::get_project(conn, &task.project_id).map_err(|e| e.to_string())?;
        (
            project.path.clone(),
            project.site_url.clone().unwrap_or_default(),
            project.agent_provider.clone().unwrap_or_else(|| "copilot".to_string()),
        )
    };

    let handlers = default_handlers();
    let handler = handlers.iter().find(|h| h.supports(&task));
    let Some(handler) = handler else {
        let msg = format!("No handler found for task type '{}'", task.task_type);
        _fail_task(conn, &mut task, &msg);
        return Ok(ExecutionResult {
            task_id: task_id.to_string(),
            success: false,
            message: msg,
            steps: vec![],
            started_at,
            finished_at: Utc::now().to_rfc3339(),
        });
    };

    let steps = handler.plan(&task);
    let mut progress: Vec<StepProgress> = steps
        .iter()
        .map(|s| StepProgress {
            step_name: s.name.clone(),
            kind: s.kind.clone(),
            status: "pending".to_string(),
            message: String::new(),
            output: None,
        })
        .collect();

    let mut all_ok = true;
    let mut last_error = String::new();
    let mut latest_raw_output: Option<String> = None;

    for (i, step) in steps.iter().enumerate() {
        progress[i].status = "running".to_string();

        let result = run_step(step, &task, &project_path, &site_url, &agent_provider, latest_raw_output.as_deref());

        // Track the raw output of agentic steps for the normalizer that follows
        if step.kind == "agentic" {
            if let Some(ref out) = result.output {
                log::info!("[executor] agentic step '{}' output ({} chars): {:?}",
                    step.name, out.len(), &out[..out.len().min(300)]);
            } else {
                log::warn!("[executor] agentic step '{}' produced no output", step.name);
            }
            latest_raw_output = result.output.clone();
        } else if step.kind == "normalizer" {
            // Normalizer consumed latest_raw; clear so it isn't reused
            latest_raw_output = None;
        }

        progress[i].status = if result.success { "ok".to_string() } else { "failed".to_string() };
        progress[i].message = result.message.clone();
        progress[i].output = result.output.clone();

        // Persist agentic / deterministic output as artifact
        if let Some(ref out) = result.output {
            let artifact = TaskArtifact {
                key: step.name.clone(),
                path: None,
                artifact_type: Some(step.kind.clone()),
                source: Some(step.kind.clone()),
                content: Some(out.clone()),
            };
            let _ = task_store::append_task_artifact(conn, task_id, &artifact);
        }

        // After a reddit_search step, upsert posts directly from the CLI JSON output.
        if step.kind == "reddit_search" && result.success {
            if let Some(ref out) = result.output {
                persist_reddit_opportunities(conn, &task.project_id, out);
                // Phase 2: AI enrichment pass — fills why_relevant, key_pain_points, website_fit, reply_text
                // Loops in batches of 5 until all posts have been enriched.
                loop {
                    let pending: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM reddit_opportunities \
                         WHERE project_id=?1 AND (why_relevant IS NULL OR reply_text IS NULL) \
                         AND reply_status != 'skipped'",
                        rusqlite::params![&task.project_id],
                        |r| r.get(0),
                    ).unwrap_or(0);
                    if pending == 0 { break; }
                    log::info!("[reddit_enrich] {} posts still pending enrichment — running batch", pending);
                    exec_reddit_enrich(conn, &task.project_id, &project_path, &agent_provider);
                }
            }
        }

        // After a reddit_opportunities normalizer step, upsert parsed opportunities into DB.
        if step.kind == "normalizer"
            && step.params.get("normalizer_id").map(|s| s.as_str()) == Some("reddit_opportunities")
        {
            log::info!("[reddit] normalizer step complete — success={} output_len={}",
                result.success,
                result.output.as_ref().map(|o| o.len()).unwrap_or(0)
            );
            if result.success {
                match &result.output {
                    Some(out) => persist_reddit_opportunities(conn, &task.project_id, out),
                    None => log::warn!("[reddit] normalizer succeeded but produced no output"),
                }
            } else {
                log::warn!("[reddit] normalizer step failed: {}", result.message);
            }
        }

        if !result.success {
            if step.optional {
                progress[i].status = "skipped".to_string();
            } else {
                all_ok = false;
                last_error = result.message.clone();
                break;
            }
        }
    }

    let finished_at = Utc::now().to_rfc3339();
    let new_status = if all_ok { "done" } else { "todo" }; // reset to todo on failure so it can be retried

    task_store::update_task_status(conn, task_id, new_status).map_err(|e| e.to_string())?;

    // After a successful content review, create a single content_review_apply task from recommendations.json.
    if all_ok && matches!(task.task_type.as_str(), "content_review" | "content_audit") {
        create_content_review_apply_task(conn, &task, &project_path);
    }

    if !all_ok {
        task_store::record_task_run(conn, task_id, false, Some(&last_error), None)
            .map_err(|e| e.to_string())?;
    } else {
        task_store::record_task_run(conn, task_id, true, None, None)
            .map_err(|e| e.to_string())?;
    }

    Ok(ExecutionResult {
        task_id: task_id.to_string(),
        success: all_ok,
        message: if all_ok { "Task completed".to_string() } else { last_error },
        steps: progress,
        started_at,
        finished_at,
    })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn run_step(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    latest_raw: Option<&str>,
) -> crate::engine::workflows::StepResult {
    match step.kind.as_str() {
        "deterministic" => exec_deterministic(step, task, project_path),
        "agentic" => exec_agentic(step, task, project_path, site_url, agent_provider),
        "manual" => crate::engine::workflows::StepResult {
            success: true,
            message: format!("Manual step '{}' — requires user action", step.name),
            output: None,
        },
        "normalizer" => {
            if let Some(raw) = latest_raw {
                let normalized = crate::engine::normalizer::normalize_agent_output(raw);
                let msg = if normalized.success {
                    format!("Normalized via '{}' — {} chars", normalized.extraction_method, normalized.raw_output.len())
                } else {
                    format!("Normalizer recorded raw output ({} chars)", normalized.raw_output.len())
                };
                let output_str = normalized.json_artifact
                    .as_ref()
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
                    .unwrap_or_else(|| normalized.raw_output.clone());
                crate::engine::workflows::StepResult {
                    success: true,
                    message: msg,
                    output: Some(output_str),
                }
            } else {
                crate::engine::workflows::StepResult {
                    success: true,
                    message: format!("Normalizer step '{}' — no raw output to normalize", step.name),
                    output: None,
                }
            }
        }
        "content_review_recommend" => exec_content_review_recommend(task, project_path, agent_provider),
        "content_review_apply_execute" => exec_content_review_apply(task, project_path, agent_provider),
        "reddit_search" => exec_reddit_search(task, project_path),
        "content_sync" => exec_content_sync(task, project_path),
        "gsc_sync_articles" => exec_gsc_sync_articles(task, project_path),
        "content_audit" => exec_content_audit(task, project_path),
        other => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Unknown step kind '{}'", other),
            output: None,
        },
    }
}

fn _fail_task(conn: &Connection, task: &mut Task, msg: &str) {
    let _ = task_store::update_task_status(conn, &task.id, "todo");
    let _ = task_store::record_task_run(conn, &task.id, false, Some(msg), None);
}

// ─── Content review apply ────────────────────────────────────────────────────

/// Execute the `content_review_apply` task.
///
/// Reads the `recommendations` artifact embedded in the task, builds a
/// structured prompt that tells the agent exactly which files to edit and
/// what to change, then runs one agent call.
///
/// Mirrors the CLI's `_run_apply` method.
fn exec_content_review_apply(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use crate::engine::project_paths::ProjectPaths;
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

// ─── Content sync + validate ──────────────────────────────────────────────────

/// Native Rust implementation of `pageseeds content sync-and-validate`.
///
/// Reads articles.json, resolves the content directory, cross-references them,
/// and optionally patches frontmatter dates. No subprocess required.
fn exec_content_sync(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::content::ops::sync_and_validate;
    use crate::engine::project_paths::ProjectPaths;

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

// ─── Content review helpers ──────────────────────────────────────────────────

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
fn select_priority_articles(
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
fn build_review_context(
    selected: &[serde_json::Value],
    repo_root: &std::path::Path,
    max_excerpt_chars: usize,
) -> serde_json::Value {
    let now = chrono::Utc::now().to_rfc3339();
    let articles: Vec<serde_json::Value> = selected.iter().filter_map(|article| {
        let file_ref = article["file"].as_str().unwrap_or("");
        if file_ref.is_empty() { return None; }
        let source = read_source_file(repo_root, file_ref);
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
fn build_review_prompt(context: &serde_json::Value) -> String {
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
fn exec_content_review_recommend(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use crate::engine::project_paths::ProjectPaths;
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
fn create_content_review_apply_task(conn: &Connection, parent_task: &Task, project_path: &str) {
    use crate::engine::project_paths::ProjectPaths;
    use crate::models::task::{Task as TaskModel, TaskArtifact, TaskRun};

    let paths = ProjectPaths::from_path(project_path);
    let rec_path = paths.automation_dir.join("recommendations.json");

    let rec_str = match std::fs::read_to_string(&rec_path) {
        Ok(s) => s,
        Err(_) => {
            log::info!("[create_apply_task] recommendations.json not found — no apply task created");
            return;
        }
    };
    let rec: serde_json::Value = match serde_json::from_str(&rec_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[create_apply_task] failed to parse recommendations.json: {}", e);
            return;
        }
    };

    let article_count = rec["articles"].as_array().map(|a| a.len()).unwrap_or(0);
    if article_count == 0 {
        log::info!("[create_apply_task] no articles in recommendations — skipping");
        return;
    }

    let existing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND type='content_review_apply' AND status IN ('todo','in_progress')",
        rusqlite::params![&parent_task.project_id],
        |r| r.get(0),
    ).unwrap_or(0);
    if existing > 0 {
        log::info!("[create_apply_task] apply task already pending — skipping");
        return;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!("task-{}", chrono::Utc::now().timestamp_millis());

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

    let new_task = TaskModel {
        id: task_id.clone(),
        phase: "implementation".to_string(),
        execution_mode: "auto".to_string(),
        task_type: "content_review_apply".to_string(),
        status: "todo".to_string(),
        priority: "high".to_string(),
        agent_policy: "required".to_string(),
        title: Some(title),
        description: Some(format!(
            "Apply SEO recommendations from recommendations.json to {} article(s). \
             The recommendations artifact contains specific suggestions per article \
             (title, meta description, intro, H1, internal links, etc.).",
            article_count
        )),
        project_id: parent_task.project_id.clone(),
        depends_on: vec![],
        artifacts: vec![artifact],
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    match task_store::create_task(conn, &new_task) {
        Ok(_) => log::info!(
            "[create_apply_task] created {} for {} article(s)",
            task_id, article_count
        ),
        Err(e) => log::warn!("[create_apply_task] failed to create apply task: {}", e),
    }
}

// ─── GSC sync articles ────────────────────────────────────────────────────────

/// Native Rust replacement for `pageseeds automation seo gsc-sync-articles`.
///
/// Fetches page-level GSC metrics for the last `days` days and writes a `gsc`
/// block into each matching article in automation/articles.json.
/// Matching uses normalised URL paths (scheme-stripped, trailing-slash removed,
/// underscore→dash, lowercase) with a secondary last-segment index.
fn exec_gsc_sync_articles(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::config::env_resolver::EnvResolver;
    use crate::engine::project_paths::ProjectPaths;
    use regex::Regex;
    use std::collections::HashMap;

    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. Get GSC service account credentials
    let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        Some(p) => p,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "GSC_SERVICE_ACCOUNT_PATH not configured — add it to ~/.config/automation/secrets.env".to_string(),
            output: None,
        },
    };

    // 2. Get token
    let rt = tokio::runtime::Handle::current();
    let token = match rt.block_on(crate::gsc::auth::get_service_account_token(&sa_path)) {
        Ok(t) => t.access_token,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("GSC auth failed: {}", e),
            output: None,
        },
    };

    // 3. Read articles.json
    let articles_path = paths.automation_dir.join("articles.json");
    let raw = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("articles.json not found at {}: {}", articles_path.display(), e),
            output: None,
        },
    };
    let mut doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to parse articles.json: {}", e),
            output: None,
        },
    };

    // 4. Get site_url — from project DB record (stored as task.project_id)
    // The task was launched with a site_url resolved at execution time; we pass it as project context.
    // Fall back to manifest.json in the automation dir.
    let site_url: String = {
        let manifest_path = paths.automation_dir.join("manifest.json");
        let from_manifest = std::fs::read_to_string(&manifest_path).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("gsc_site").or_else(|| v.get("url"))
                    .and_then(|u| u.as_str())
                    .map(String::from)
            });
        match from_manifest {
            Some(u) => u,
            None => return crate::engine::workflows::StepResult {
                success: false,
                message: "No site_url found in manifest.json — add 'url' or 'gsc_site' field".to_string(),
                output: None,
            },
        }
    };

    // Normalise sc-domain: properties to https:// base URLs for slug comparison
    let base_url = if site_url.starts_with("sc-domain:") {
        format!("https://{}", &site_url["sc-domain:".len()..])
    } else {
        site_url.clone()
    };
    let base_url = base_url.trim_end_matches('/').to_string();

    // 5. Fetch GSC page metrics (90-day window)
    let days = 90i64;
    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let start = end - chrono::Duration::days(days - 1);
    let page_rows = match rt.block_on(crate::gsc::analytics::fetch_page_rows(
        &token, &site_url,
        &start.format("%Y-%m-%d").to_string(),
        &end.format("%Y-%m-%d").to_string(),
        1000,
    )) {
        Ok(rows) => rows,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("GSC fetch failed: {}", e),
            output: None,
        },
    };

    // 6. Build normalised path → metrics lookup
    let num_prefix_re = Regex::new(r"^\d+[_\-]+").unwrap();

    let normalize_path = |url: &str| -> String {
        let stripped = if let Some(rest) = url.strip_prefix("https://") { rest }
            else if let Some(rest) = url.strip_prefix("http://") { rest }
            else { url };
        let path = if let Some(slash) = stripped.find('/') { &stripped[slash..] } else { "/" };
        path.trim_end_matches('/').replace('_', "-").to_lowercase()
    };

    let mut gsc_by_path: HashMap<String, &crate::models::gsc::PageMetrics> = HashMap::new();
    for row in &page_rows {
        let p = normalize_path(&row.page);
        if !p.is_empty() { gsc_by_path.entry(p).or_insert(row); }
    }
    // Secondary: last segment index (for bare-slug articles matching /blog/slug paths)
    let mut gsc_by_segment: HashMap<String, &crate::models::gsc::PageMetrics> = HashMap::new();
    for (path, m) in &gsc_by_path {
        let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        if !last.is_empty() {
            gsc_by_segment.entry(last.clone()).or_insert(m);
            let stripped = num_prefix_re.replace(&last, "").to_string();
            if stripped != last && !stripped.is_empty() {
                gsc_by_segment.entry(stripped).or_insert(m);
            }
        }
    }

    // 7. Match articles and write gsc block
    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let articles = doc["articles"].as_array_mut()
        .ok_or("no articles array")
        .unwrap();

    let mut matched = 0usize;
    let mut unmatched = 0usize;
    let _ = &base_url; // used above

    for article in articles.iter_mut() {
        let slug = article["url_slug"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();

        // Build article path to match against GSC
        let article_path: String = if !slug.is_empty() {
            let s = slug.trim_matches('/').replace('_', "-").to_lowercase();
            format!("/{}", s)
        } else if !file_ref.is_empty() {
            let stem = std::path::Path::new(&file_ref)
                .file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let s = num_prefix_re.replace(&stem, "").to_string();
            format!("/{}", s.replace('_', "-").to_lowercase())
        } else {
            article["gsc"] = serde_json::Value::Null;
            unmatched += 1;
            continue;
        };

        let metrics = gsc_by_path.get(&article_path)
            .or_else(|| gsc_by_segment.get(article_path.trim_start_matches('/')));

        if let Some(m) = metrics {
            article["gsc"] = serde_json::json!({
                "impressions": m.impressions,
                "clicks": m.clicks,
                "ctr": (m.ctr * 10000.0).round() / 10000.0,
                "avg_position": (m.position * 10.0).round() / 10.0,
                "last_synced": now_iso,
                "period_days": days,
            });
            matched += 1;
        } else {
            article["gsc"] = serde_json::Value::Null;
            unmatched += 1;
        }
    }

    // 8. Write back
    let out = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&articles_path, &out) {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to write articles.json: {}", e),
            output: None,
        };
    }

    let summary = serde_json::json!({
        "matched": matched,
        "unmatched": unmatched,
        "total": matched + unmatched,
        "gsc_rows": page_rows.len(),
        "site": site_url,
        "period_days": days,
    });

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "GSC sync: matched {}/{} articles ({} GSC pages fetched)",
            matched, matched + unmatched, page_rows.len()
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

// ─── Content audit ────────────────────────────────────────────────────────────

/// Native Rust replacement for `pageseeds automation seo content-audit`.
///
/// Runs 13 deterministic checks per article (keyword in title/H1/meta, word count,
/// internal links, etc.), scores each article, and writes content_audit.json to
/// automation/content_audit.json. No LLM or external API needed.
fn exec_content_audit(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::engine::project_paths::ProjectPaths;
    use regex::Regex;

    let paths = ProjectPaths::from_path(project_path);
    let articles_path = paths.automation_dir.join("articles.json");

    let raw = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("articles.json not found: {}", e),
            output: None,
        },
    };
    let doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to parse articles.json: {}", e),
            output: None,
        },
    };

    let empty = vec![];
    let articles = doc["articles"].as_array().unwrap_or(&empty);

    // Only audit published/live articles (skip drafts)
    let to_audit: Vec<&serde_json::Value> = articles.iter()
        .filter(|a| {
            let status = a["status"].as_str().unwrap_or("").to_lowercase();
            matches!(status.as_str(), "published" | "live" | "")
        })
        .collect();

    let num_prefix_re = Regex::new(r"^\d+[_\-]+").unwrap();

    let mut results: Vec<serde_json::Value> = to_audit.iter().map(|article| {
        audit_one_article(article, &paths.repo_root, &num_prefix_re)
    }).collect();

    // Sort: worst first (highest priority_score, lowest health_score)
    results.sort_by(|a, b| {
        let pa = a["priority_score"].as_f64().unwrap_or(0.0);
        let pb = b["priority_score"].as_f64().unwrap_or(0.0);
        pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let good = results.iter().filter(|r| r["health"].as_str() == Some("good")).count();
    let needs = results.iter().filter(|r| r["health"].as_str() == Some("needs_improvement")).count();
    let poor = results.iter().filter(|r| r["health"].as_str() == Some("poor")).count();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let output_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_audited": results.len(),
        "health_summary": { "good": good, "needs_improvement": needs, "poor": poor },
        "articles": results,
    });

    let out_path = paths.automation_dir.join("content_audit.json");
    let out_str = serde_json::to_string_pretty(&output_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &out_str) {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to write content_audit.json: {}", e),
            output: None,
        };
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Content audit: {} articles — {} good, {} needs work, {} poor",
            good + needs + poor, good, needs, poor
        ),
        output: Some(serde_json::to_string_pretty(&serde_json::json!({
            "total": good + needs + poor,
            "good": good, "needs_improvement": needs, "poor": poor,
            "output_path": out_path.display().to_string(),
        })).unwrap_or_default()),
    }
}

/// Run all deterministic checks on one article, return an audit record Value.
fn audit_one_article(
    article: &serde_json::Value,
    repo_root: &std::path::Path,
    num_prefix_re: &regex::Regex,
) -> serde_json::Value {
    let keyword = article["target_keyword"].as_str().unwrap_or("").trim().to_lowercase();
    let title = article["title"].as_str().unwrap_or("").trim().to_string();
    let file_ref = article["file"].as_str().unwrap_or("").trim().to_string();
    let gsc = &article["gsc"];
    let published_date = article["published_date"].as_str().unwrap_or("").to_string();
    let status = article["status"].as_str().unwrap_or("").to_lowercase();

    // Read source file
    let source = read_source_file(repo_root, &file_ref);
    let (fm, body) = parse_frontmatter(source.as_deref().unwrap_or(""));

    let meta_description = fm.get("description").map(String::as_str).unwrap_or("").trim().to_string();

    // Parse headings + structure
    let h1 = body.lines()
        .find(|l| l.trim_start().starts_with("# ") && !l.trim_start().starts_with("## "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();
    let h2_count = body.lines()
        .filter(|l| { let t = l.trim_start(); t.starts_with("## ") && !t.starts_with("### ") })
        .count();

    // Word count (strip markdown syntax)
    let plain = {
        let no_code = regex::Regex::new(r"(?s)```.*?```").unwrap().replace_all(&body, " ").to_string();
        let no_links = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap().replace_all(&no_code, "$1").to_string();
        let no_md = regex::Regex::new(r"[#*_`>|]").unwrap().replace_all(&no_links, " ").to_string();
        no_md
    };
    let actual_word_count = plain.split_whitespace().count();

    // Keyword density
    let kw_count = if keyword.is_empty() { 0 } else {
        body.to_lowercase().matches(keyword.as_str()).count()
    };
    let kw_density = if actual_word_count > 0 && !keyword.is_empty() {
        kw_count as f64 / actual_word_count as f64 * 100.0
    } else { 0.0 };

    // First paragraph (first non-empty, non-heading line)
    let first_para = body.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .unwrap_or("")
        .to_lowercase();

    // Links
    let link_re = regex::Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    let all_links: Vec<(String, String)> = link_re.captures_iter(&body)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    let internal_link_count = all_links.iter()
        .filter(|(_, href)| !href.starts_with("http"))
        .count();
    let broken_links: Vec<serde_json::Value> = all_links.iter()
        .filter(|(_, href)| href.contains("TODO") || href.trim() == "" || href.trim() == "#")
        .map(|(text, href)| serde_json::json!({ "text": text, "href": href }))
        .collect();

    // ─── Checks ──────────────────────────────────────────────────────────────
    let check_pass = |pass: Option<bool>, label: &str| -> serde_json::Value {
        serde_json::json!({ "pass": pass, "label": label })
    };
    let check_val = |pass: Option<bool>, value: serde_json::Value, label: &str| -> serde_json::Value {
        serde_json::json!({ "pass": pass, "value": value, "label": label })
    };

    let kw_opt = if keyword.is_empty() { None } else { Some(keyword.clone()) };

    let checks = serde_json::json!({
        "title_keyword":        check_pass(kw_opt.as_ref().map(|kw| title.to_lowercase().contains(kw.as_str())), "Title contains keyword"),
        "h1_keyword":           check_pass(kw_opt.as_ref().map(|kw| h1.to_lowercase().contains(kw.as_str())), "H1 contains keyword"),
        "meta_desc_present":    check_pass(Some(!meta_description.is_empty()), "Meta description present"),
        "meta_desc_keyword":    check_pass(kw_opt.as_ref().map(|kw| meta_description.to_lowercase().contains(kw.as_str())), "Meta description contains keyword"),
        "meta_desc_length":     check_val(Some(meta_description.len() >= 50 && meta_description.len() <= 155), serde_json::json!(meta_description.len()), "Meta description length 50–155 chars"),
        "keyword_first_para":   check_pass(kw_opt.as_ref().map(|kw| first_para.contains(kw.as_str())), "Keyword in first paragraph"),
        "word_count":           check_val(Some(actual_word_count >= 800), serde_json::json!(actual_word_count), "Word count ≥ 800"),
        "keyword_density":      check_val(kw_opt.as_ref().map(|_| kw_density >= 0.2 && kw_density <= 0.8), serde_json::json!(format!("{:.2}%", kw_density)), "Keyword density 0.2–0.8%"),
        "h2_structure":         check_val(Some(h2_count >= 2), serde_json::json!(h2_count), "Has ≥2 H2 headings"),
        "internal_links":       check_val(Some(internal_link_count >= 3), serde_json::json!(internal_link_count), "Has ≥3 internal links"),
        "broken_links":         serde_json::json!({ "pass": broken_links.is_empty(), "value": broken_links.len(), "issues": broken_links, "label": "No broken/placeholder links" }),
        "gsc_data":             check_pass(Some(!gsc.is_null()), "GSC data synced"),
        "source_file_found":    check_pass(Some(source.is_some()), "Source file readable"),
    });

    // ─── Scoring ─────────────────────────────────────────────────────────────
    let weights = [
        ("broken_links", 30i64), ("source_file_found", 20), ("title_keyword", 10),
        ("h1_keyword", 10), ("meta_desc_keyword", 10), ("keyword_first_para", 8),
        ("keyword_density", 8), ("meta_desc_present", 7), ("meta_desc_length", 5),
        ("word_count", 5), ("h2_structure", 3), ("internal_links", 3), ("gsc_data", 1),
    ];
    let penalty: i64 = weights.iter().map(|(k, w)| {
        if checks[k]["pass"].as_bool() == Some(false) { *w } else { 0 }
    }).sum();
    let health_score = (100 - penalty).max(0);

    let health = if health_score >= 85 { "good" }
        else if health_score >= 60 { "needs_improvement" }
        else { "poor" };

    let critical_issues = ["broken_links", "source_file_found", "title_keyword"].iter()
        .filter(|k| checks[*k]["pass"].as_bool() == Some(false)).count();
    let high_issues = ["meta_desc_keyword", "keyword_first_para", "keyword_density", "h1_keyword"].iter()
        .filter(|k| checks[*k]["pass"].as_bool() == Some(false)).count();

    // GSC priority boost for old articles with no/low impressions
    let gsc_boost: i64 = if gsc.is_null() {
        if let Ok(pub_date) = chrono::NaiveDate::parse_from_str(&published_date, "%Y-%m-%d") {
            let age = (chrono::Utc::now().date_naive() - pub_date).num_days();
            if age > 60 { 15 } else { 0 }
        } else { 0 }
    } else {
        let impressions = gsc["impressions"].as_f64().unwrap_or(0.0) as i64;
        if impressions == 0 { 10 } else if impressions < 50 { 5 } else { 0 }
    };

    let priority_score = penalty + gsc_boost;
    let checks_passed = weights.iter().filter(|(k, _)| checks[*k]["pass"].as_bool() == Some(true)).count();
    let checks_failed = weights.iter().filter(|(k, _)| checks[*k]["pass"].as_bool() == Some(false)).count();

    let _ = num_prefix_re; // used by caller for slug normalization

    serde_json::json!({
        "id": article["id"],
        "title": title,
        "url_slug": article["url_slug"],
        "file": file_ref,
        "target_keyword": keyword,
        "status": status,
        "published_date": published_date,
        "word_count": actual_word_count,
        "gsc": gsc,
        "health_score": health_score,
        "health": health,
        "priority_score": priority_score,
        "critical_issues": critical_issues,
        "high_issues": high_issues,
        "checks": checks,
        "checks_passed": checks_passed,
        "checks_failed": checks_failed,
        "checks_total": weights.len(),
    })
}

/// Read an article source file. Returns None if not found or unreadable.
fn read_source_file(repo_root: &std::path::Path, file_ref: &str) -> Option<String> {
    if file_ref.is_empty() { return None; }
    let p = std::path::Path::new(file_ref);
    let full = if p.is_absolute() { p.to_path_buf() } else { repo_root.join(p) };
    std::fs::read_to_string(&full).ok()
}

/// Parse YAML frontmatter from an MDX/markdown source string.
/// Returns (frontmatter_map, body_string).
fn parse_frontmatter(source: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut fm = std::collections::HashMap::new();
    if !source.starts_with("---") {
        return (fm, source.to_string());
    }
    let end = match source[3..].find("\n---") {
        Some(i) => i + 3,
        None => return (fm, source.to_string()),
    };
    let fm_text = &source[3..end];
    let body = source[end + 4..].trim_start().to_string();
    for line in fm_text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            fm.insert(k.trim().to_string(), val);
        }
    }
    (fm, body)
}

// ─── Reddit deterministic search ─────────────────────────────────────────────

/// Resolve the Python interpreter used by the installed `pageseeds` CLI.
/// Extract lines from the "## Trigger Topics" section of a reddit_config.md.
fn extract_trigger_topics(config: &str, max: usize) -> Vec<String> {
    let mut in_section = false;
    let mut topics: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Trigger Topics") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") {
                break; // next section
            }
            if let Some(topic) = trimmed.strip_prefix("- ") {
                // Strip parenthetical descriptions: "Covered calls (selling, ...)" → "Covered calls"
                let topic = topic.split('(').next().unwrap_or(topic).trim().to_string();
                if !topic.is_empty() {
                    topics.push(topic);
                    if topics.len() >= max {
                        break;
                    }
                }
            }
        }
    }
    topics
}

/// Extract subreddit names from the "## Seed Subreddits" or "## Target Subreddits" section
/// of reddit_config.md.  Normalises entries by stripping a leading `r/` prefix, the
/// post-subreddit description (em dash / hyphen separated), and converting to lowercase.
fn extract_seed_subreddits(config: &str) -> Vec<String> {
    let mut in_section = false;
    let mut subs: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        // Accept both common section header variants.
        if trimmed.starts_with("## Seed Subreddits")
            || trimmed.starts_with("## Target Subreddits")
        {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(name) = trimmed.strip_prefix("- ") {
                // Strip leading "r/" and anything after " — " or " - " (description).
                let name = name.trim().trim_start_matches("r/");
                let name = name.split(" — ").next().unwrap_or(name);
                let name = name.split(" - ").next().unwrap_or(name);
                let name = name.trim().to_lowercase();
                if !name.is_empty() { subs.push(name); }
            }
        }
    }
    subs
}

/// Extract compact search queries from the "## Query Keywords" section of reddit_config.md.
/// These are the preferred search terms — precise and short, unlike the verbose
/// "## Trigger Topics" descriptions.
/// Returns an empty vec if the section doesn't exist.
fn extract_query_keywords(config: &str) -> Vec<String> {
    let mut in_section = false;
    let mut keywords: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Query Keywords") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(raw) = trimmed.strip_prefix("- ") {
                // Each bullet may have multiple quoted terms: `"covered call" "covered calls"`
                // Extract just the first quoted term as the primary search string.
                let raw = raw.trim();
                if raw.starts_with('"') {
                    if let Some(end) = raw[1..].find('"') {
                        let kw = raw[1..end + 1].trim().to_string();
                        if !kw.is_empty() { keywords.push(kw); }
                        continue;
                    }
                }
                // No quotes — use the whole bullet stripped of markdown
                let kw = raw.trim_matches('`').trim().to_string();
                if !kw.is_empty() { keywords.push(kw); }
            }
        }
    }
    keywords
}

/// Extract subreddit names from the "## Excluded Subreddits" section of reddit_config.md.
fn extract_excluded_subreddits(config: &str) -> std::collections::HashSet<String> {
    let mut in_section = false;
    let mut excluded: std::collections::HashSet<String> = Default::default();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Excluded Subreddits") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("##") { break; }
            if let Some(name) = trimmed.strip_prefix("- ") {
                let name = name.trim().to_lowercase();
                if !name.is_empty() { excluded.insert(name); }
            }
        }
    }
    excluded
}

/// Compute scores matching the PageSeeds CLI SKILL.md formula.
///
/// - engagement  = min(10, upvotes / max(1, days_old) / 10)
/// - accessibility = <10 comments→10, 10-30→8, 30-100→6, 100+→2
/// - relevance   = 5.0 (neutral; agent-assessed in the full CLI, deterministic default here)
/// - final_score = (relevance + engagement + accessibility) / 3
/// - severity    = CRITICAL ≥8.5, HIGH ≥7.0, MEDIUM ≥5.0
fn compute_scores(upvotes: i64, comment_count: i64, days_old: i64)
    -> (f64, f64, f64, f64, &'static str)
{
    let relevance_score: f64 = 5.0;
    let age = days_old.max(1) as f64;
    let engagement_score = (upvotes as f64 / age / 10.0).min(10.0).max(0.0);
    let accessibility_score: f64 = match comment_count {
        c if c < 10  => 10.0,
        c if c < 30  => 8.0,
        c if c < 100 => 6.0,
        _            => 2.0,
    };
    let final_score = (relevance_score + engagement_score + accessibility_score) / 3.0;
    let severity = if final_score >= 8.5 { "CRITICAL" }
        else if final_score >= 7.0 { "HIGH" }
        else if final_score >= 5.0 { "MEDIUM" }
        else { "LOW" };
    (relevance_score, engagement_score, accessibility_score, final_score, severity)
}

/// Run a deterministic Reddit search using the installed `pageseeds` Python CLI.
/// Applies the same rules as the PageSeeds CLI SKILL.md:
///   - Reads trigger topics + excluded subreddits from `reddit_config.md`
///   - Searches with --time week (last 7 days only passed to API)
///   - Hard filter: skips posts older than 14 days
///   - Scores: engagement, accessibility, final_score (relevance defaults to 5.0)
///   - Only keeps MEDIUM+ (final_score >= 5.0)
///   - Deduplicates by post_id across queries
fn exec_reddit_search(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    const MAX_AGE_DAYS: i64 = 14;

    log::info!("[reddit_search] starting for project={} path={}", task.project_id, project_path);

    let config_path = format!("{}/.github/automation/reddit_config.md", project_path);
    let config = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[reddit_search] cannot read reddit_config.md at {}: {}", config_path, e);
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("reddit_config.md not found at {} — create it first", config_path),
                output: None,
            };
        }
    };

    // Prefer the focused "## Query Keywords" section (compact search terms).
    // Fall back to the first N "## Trigger Topics" bullets if no keywords section exists.
    let queries = {
        let kw = extract_query_keywords(&config);
        if kw.is_empty() {
            extract_trigger_topics(&config, 5)
        } else {
            // Cap at 10 to avoid generating an enormous search matrix.
            kw.into_iter().take(10).collect()
        }
    };
    let seed_subs = extract_seed_subreddits(&config);
    let excluded_subs = extract_excluded_subreddits(&config);
    let mention_stance = {
        let cfg = crate::reddit::config::parse_reddit_config(&config);
        cfg.mention_stance.as_str().to_string()
    };
    log::info!(
        "[reddit_search] queries ({}) {:?}  seed_subreddits ({}) {:?}",
        queries.len(), &queries[..queries.len().min(5)],
        seed_subs.len(), &seed_subs[..seed_subs.len().min(5)]
    );

    if queries.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "No trigger topics or query keywords found in reddit_config.md — add a '## Query Keywords' or '## Trigger Topics' section".to_string(),
            output: None,
        };
    }

    // Build (subreddit, query) search pairs per SKILL.md.
    // If seed subreddits are configured, search each sub × query combination.
    // If none are configured, fall back to global search (empty subreddit = all of Reddit).
    // Cap total pairs at 50 to match CLI agent's practical limit and avoid rate-limit issues.
    const MAX_SEARCH_PAIRS: usize = 50;
    let search_pairs: Vec<(String, String)> = if seed_subs.is_empty() {
        log::warn!("[reddit_search] no '## Seed Subreddits' or '## Target Subreddits' in reddit_config.md — falling back to global search");
        queries.iter().take(MAX_SEARCH_PAIRS).map(|q| (String::new(), q.clone())).collect()
    } else {
        let pairs: Vec<(String, String)> = seed_subs.iter()
            .flat_map(|sub| queries.iter().map(move |q| (sub.clone(), q.clone())))
            .take(MAX_SEARCH_PAIRS)
            .collect();
        pairs
    };
    log::info!("[reddit_search] {} search pairs: {:?}", search_pairs.len(), search_pairs);

    let mut all_posts: Vec<serde_json::Value> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = Default::default();
    let mut too_old = 0usize;
    let mut excluded_sub_count = 0usize;
    let mut below_threshold = 0usize;
    let mut history_filtered = 0usize;

    // Load history file so we skip already-handled posts (dedup sync with CLI).
    let history_manager = crate::reddit::history::RedditHistoryManager::new(
        std::path::Path::new(project_path)
    );
    let handled_ids = history_manager.get_all_handled_ids();

    // Grab a Tokio handle so we can call the async search_submissions from this sync fn.
    let rt_handle = tokio::runtime::Handle::current();

    for (subreddit, query) in &search_pairs {
        log::info!("[reddit_search] searching sub={:?} query={:?}", subreddit, query);

        let posts = match rt_handle.block_on(
            crate::reddit::search::search_submissions(query, subreddit, 10, "relevance", "week")
        ) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("[reddit_search] search failed for {:?}/{:?}: {}", subreddit, query, e);
                continue;
            }
        };

        let before = all_posts.len();
        for post in posts {
                    let post_id = post.post_id.clone();

                    // Excluded subreddit check
                    if let Some(ref sub) = post.subreddit {
                        if excluded_subs.contains(&sub.to_lowercase()) {
                            log::info!("[reddit_search] skip excluded subreddit {} for {}", sub, post_id);
                            excluded_sub_count += 1;
                            continue;
                        }
                    }

                    // 14-day hard filter
                    let days_old = post.days_old.unwrap_or(0);
                    if days_old > MAX_AGE_DAYS {
                        log::info!("[reddit_search] skip post {} ({} days old > {})", post_id, days_old, MAX_AGE_DAYS);
                        too_old += 1;
                        continue;
                    }

                    // Dedup across queries
                    if !seen_ids.insert(post_id.clone()) { continue; }

                    // Skip posts already handled
                    if handled_ids.contains(&post_id) {
                        log::info!("[reddit_search] skip history-handled post {}", post_id);
                        history_filtered += 1;
                        continue;
                    }

                    // Score: engagement + accessibility + relevance=5.0
                    let upvotes = post.upvotes.unwrap_or(0);
                    let comments = post.comment_count.unwrap_or(0);
                    let (relevance, engagement, accessibility, final_score, severity) =
                        compute_scores(upvotes, comments, days_old);

                    // Only keep MEDIUM+ (final_score >= 5.0)
                    if final_score < 5.0 {
                        below_threshold += 1;
                        continue;
                    }

                    let enriched = serde_json::json!({
                        "post_id": post_id,
                        "title": post.title,
                        "url": post.url,
                        "subreddit": post.subreddit,
                        "author": post.author,
                        "upvotes": upvotes,
                        "comment_count": comments,
                        "days_old": days_old,
                        "created_at": post.created_at,
                        "posted_date": post.created_at,
                        "selftext": post.selftext,
                        "relevance_score": relevance,
                        "engagement_score": engagement,
                        "accessibility_score": accessibility,
                        "final_score": final_score,
                        "severity": severity,
                        "mention_stance": mention_stance,
                    });
                    all_posts.push(enriched);
        }
        log::info!("[reddit_search] query {:?} sub {:?}: +{} accepted (running total {})",
            query, subreddit, all_posts.len() - before, all_posts.len());
    }

    // Cap at top 10 by pre-score, matching the CLI which naturally surfaces ~10 best opportunities.
    // Relevance is placeholder (5.0) at this stage — the AI enrichment pass will score it properly.
    // Keeping only the 10 highest-engagement posts gives the enrichment pass the best candidates.
    const MAX_RESULTS: usize = 10;
    all_posts.sort_by(|a, b| {
        let fa = a.get("final_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let fb = b.get("final_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal)
    });
    all_posts.truncate(MAX_RESULTS);

    log::info!(
        "[reddit_search] done — kept={} (top {} by score) too_old={} excluded_sub={} below_threshold={} history_filtered={}",
        all_posts.len(), MAX_RESULTS, too_old, excluded_sub_count, below_threshold, history_filtered
    );

    if all_posts.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No Reddit posts found across {} search pairs (filtered: {} too old, {} excluded subreddits, {} below score threshold)",
                search_pairs.len(), too_old, excluded_sub_count, below_threshold
            ),
            output: None,
        };
    }

    let output = match serde_json::to_string(&serde_json::json!({"posts": all_posts})) {
        Ok(s) => s,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to serialize results: {}", e),
            output: None,
        },
    };

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Found {} Reddit posts (top {} by score across {} search pairs — {} too old, {} excluded, {} below threshold, {} already handled)",
            all_posts.len(), MAX_RESULTS, search_pairs.len(), too_old, excluded_sub_count, below_threshold, history_filtered
        ),
        output: Some(output),
    }
}

/// Parse a JSON array of Reddit opportunity objects from normalizer output and upsert each into DB.
///
/// The agent may return the array directly, or as a value under a key like "opportunities".
/// We tolerate partial fields — only `post_id` is required.
fn persist_reddit_opportunities(conn: &Connection, project_id: &str, json_str: &str) {
    log::info!("[reddit] persist_reddit_opportunities called — project={} json_len={}", project_id, json_str.len());

    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[reddit] failed to parse normalizer JSON: {} — first 200 chars: {:?}", e, &json_str[..json_str.len().min(200)]);
            return;
        }
    };

    // Accept either a bare array or {"opportunities": [...]} / {"results": [...]} / {"posts": [...]}
    let array = if value.is_array() {
        log::info!("[reddit] JSON is a bare array");
        value.as_array().cloned().unwrap_or_default()
    } else if let Some(arr) = ["opportunities", "results", "posts", "items"]
        .iter()
        .find_map(|key| value.get(key).and_then(|v| v.as_array()).cloned())
    {
        log::info!("[reddit] JSON is an object with array field");
        arr
    } else {
        log::warn!("[reddit] JSON structure not recognised — top-level keys: {:?}",
            value.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()));
        return;
    };

    log::info!("[reddit] {} opportunities to upsert for project={}", array.len(), project_id);

    // Clear pending rows from previous runs before inserting fresh results.
    // Rows with reply_status='posted' or 'skipped' are preserved — they feed the history dedup.
    let deleted = conn.execute(
        "DELETE FROM reddit_opportunities WHERE project_id=?1 AND reply_status='pending'",
        rusqlite::params![project_id],
    ).unwrap_or(0);
    if deleted > 0 {
        log::info!("[reddit] cleared {} stale pending rows for project={}", deleted, project_id);
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut upserted = 0usize;
    let mut skipped = 0usize;

    for item in &array {
        let post_id = match item.get("post_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => { skipped += 1; continue; }
        };

        // Deduplication against DB: skip posts already posted or skipped (CLI DEDUPLICATION RULE)
        let already_handled: bool = conn.query_row(
            "SELECT COUNT(*) FROM reddit_opportunities WHERE post_id=?1 AND reply_status IN ('posted','skipped')",
            rusqlite::params![post_id],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0;
        if already_handled {
            log::info!("[reddit] skip already-posted/skipped post {}", post_id);
            skipped += 1;
            continue;
        }

        let opp = crate::models::reddit::RedditOpportunity {
            post_id,
            title: item.get("title").and_then(|v| v.as_str()).map(str::to_string),
            url: item.get("url").and_then(|v| v.as_str()).map(str::to_string),
            subreddit: item.get("subreddit").and_then(|v| v.as_str()).map(str::to_string),
            author: item.get("author").and_then(|v| v.as_str()).map(str::to_string),
            posted_date: item.get("posted_date").and_then(|v| v.as_str()).map(str::to_string),
            upvotes: item.get("upvotes").and_then(|v| v.as_i64()),
            comment_count: item.get("comment_count").and_then(|v| v.as_i64()),
            relevance_score: item.get("relevance_score").and_then(|v| v.as_f64()),
            engagement_score: item.get("engagement_score").and_then(|v| v.as_f64()),
            accessibility_score: item.get("accessibility_score").and_then(|v| v.as_f64()),
            final_score: item.get("final_score").and_then(|v| v.as_f64()),
            severity: item.get("severity").and_then(|v| v.as_str()).map(str::to_string),
            why_relevant: item.get("why_relevant").and_then(|v| v.as_str()).map(str::to_string),
            key_pain_points: item
                .get("key_pain_points")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            website_fit: item.get("website_fit").and_then(|v| v.as_str()).map(str::to_string),
            mention_stance: item.get("mention_stance").and_then(|v| v.as_str()).map(str::to_string),
            reply_status: item
                .get("reply_status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string(),
            reply_text: item.get("reply_text").and_then(|v| v.as_str()).map(str::to_string),
            reply_url: item.get("reply_url").and_then(|v| v.as_str()).map(str::to_string),
            reply_upvotes: item.get("reply_upvotes").and_then(|v| v.as_i64()),
            reply_replies: item.get("reply_replies").and_then(|v| v.as_i64()),
            posted_at: item.get("posted_at").and_then(|v| v.as_str()).map(str::to_string),
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match crate::reddit::db::upsert_opportunity(conn, &opp) {
            Ok(_) => { upserted += 1; }
            Err(e) => {
                log::warn!("[reddit] upsert failed for post_id={}: {}", opp.post_id, e);
                skipped += 1;
            }
        }
    }

    log::info!("[reddit] done — upserted={} skipped={} project={}", upserted, skipped, project_id);
}

/// AI enrichment pass: read un-enriched Reddit opportunities and fill in
/// `why_relevant`, `key_pain_points`, `website_fit`, and recalculate
/// `relevance_score` / `final_score` / `severity` using AI-assessed relevance.
///
/// Silently skips if no agent is configured or no unenriched posts exist.
pub fn exec_reddit_enrich(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    agent_provider: &str,
) {
    use crate::engine::agent;
    use std::path::Path;

    log::info!("[reddit_enrich] starting for project={}", project_id);

    // Fetch up to 5 posts that still need enrichment OR a reply draft.
    // Smaller batches keep the agent focused and prevent it trying to fetch external content.
    let rows: Vec<(String, Option<String>, Option<String>, Option<f64>, Option<f64>)> = {
        let mut result = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT post_id, title, subreddit, engagement_score, accessibility_score \
             FROM reddit_opportunities \
             WHERE project_id=?1 \
               AND (why_relevant IS NULL OR reply_text IS NULL) \
               AND reply_status != 'skipped' \
             LIMIT 5",
        ) {
            if let Ok(mapped) = stmt.query_map(rusqlite::params![project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<f64>>(4)?,
                ))
            }) {
                for item in mapped.flatten() {
                    result.push(item);
                }
            } else {
                log::warn!("[reddit_enrich] query failed");
                return;
            }
        } else {
            log::warn!("[reddit_enrich] prepare failed");
            return;
        }
        result
    };

    if rows.is_empty() {
        log::info!("[reddit_enrich] no unenriched posts — skipping");
        return;
    }
    log::info!("[reddit_enrich] {} posts to enrich", rows.len());

    // Read project context files.
    let automation_dir = Path::new(project_path).join(".github").join("automation");
    let project_summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
        .unwrap_or_default();
    let reddit_config_raw = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .unwrap_or_default();
    let brandvoice = std::fs::read_to_string(automation_dir.join("brandvoice.md"))
        .unwrap_or_default();
    let guardrails = std::fs::read_to_string(
        automation_dir.join("reddit").join("_reply_guardrails.md")
    ).unwrap_or_default();

    if project_summary.is_empty() && reddit_config_raw.is_empty() {
        log::warn!("[reddit_enrich] no project context available — skipping enrichment");
        return;
    }

    // Parse product name and mention stance from config.
    let cfg = crate::reddit::config::parse_reddit_config(&reddit_config_raw);
    let product_name = cfg.product_name.as_deref().unwrap_or("the product").to_string();
    let mention_stance_str = cfg.mention_stance.as_str().to_string();
    let stance_instruction = match cfg.mention_stance {
        crate::reddit::config::MentionStance::Required => format!(
            "REQUIRED: The reply MUST contain the exact product name \"{}\" — no vague substitutes like 'a tool' or 'the app'.",
            product_name
        ),
        crate::reddit::config::MentionStance::Recommended => format!(
            "RECOMMENDED: Mention \"{}\" by name if the topic is a natural fit.",
            product_name
        ),
        crate::reddit::config::MentionStance::Optional => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally. Not required.",
            product_name
        ),
        crate::reddit::config::MentionStance::Omit =>
            "OMIT: Do NOT mention any product name in this reply.".to_string(),
    };

    // Build the posts list for the prompt.
    let posts_block: String = rows.iter().enumerate().map(|(i, (pid, title, sub, _, _))| {
        format!(
            "{}. post_id=\"{}\"  subreddit=\"{}\"  title=\"{}\"",
            i + 1,
            pid,
            sub.as_deref().unwrap_or("unknown"),
            title.as_deref().unwrap_or("(no title)")
                .replace('"', "'")
                .chars().take(200).collect::<String>()
        )
    }).collect::<Vec<_>>().join("\n");

    let prompt = format!(
        r#"You are a copywriter. Your only job is to read the post titles below and produce a JSON array.

DO NOT run any shell commands. DO NOT fetch any URLs. DO NOT use curl or any tool. 
Work ONLY from the post titles and subreddits provided. This is a pure text-generation task.

## PRODUCT CONTEXT
{project_summary}

## REDDIT CONFIG
{reddit_config_raw}

## BRAND VOICE
{brandvoice}

## REPLY GUARDRAILS
{guardrails}

## PRODUCT MENTION RULES
Product name: {product_name}
Mention stance: {mention_stance_str}
{stance_instruction}
FORBIDDEN VAGUE PHRASES (replace with "{product_name}"): 'a dedicated tool', 'a platform', 'the app', 'a tracker', 'my tool', 'a tool I built'

## POST TITLES (this is all the data you have — do not fetch more)
{posts_block}

## OUTPUT FORMAT
Return a JSON array with exactly {count} objects, one per post:
[
  {{
    "post_id": "<exact post_id from above>",
    "relevance_score": <integer 0-10>,
    "why_relevant": "<one sentence>",
    "key_pain_points": ["<pain point 1>", "<pain point 2>"],
    "website_fit": "<one sentence>",
    "reply_text": "<3-5 sentence plain-text reply>"
  }}
]

reply_text rules:
- Formula: Acknowledge → Educate → Product mention (per stance) → Genuine question
- Plain text only — no markdown, no bullet points, no URLs
- Conversational, not corporate
- Use exact product name per stance rules above

Return ONLY the raw JSON array. No preamble, no explanation, no markdown fences, no shell commands.
"#,
        project_summary = project_summary,
        reddit_config_raw = reddit_config_raw,
        brandvoice = brandvoice,
        guardrails = guardrails,
        product_name = product_name,
        mention_stance_str = mention_stance_str,
        stance_instruction = stance_instruction,
        posts_block = posts_block,
        count = rows.len(),
    );

    let output = match agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[reddit_enrich] agent failed: {}", e);
            return;
        }
    };

    // Extract JSON array from output (agent may wrap in markdown fences).
    let json_str = extract_json_array(&output);
    let enrichments: Vec<serde_json::Value> = match serde_json::from_str(&json_str) {
        Ok(serde_json::Value::Array(arr)) => arr,
        _ => {
            log::warn!("[reddit_enrich] could not parse agent output as JSON array — first 300 chars: {:?}",
                &output[..output.len().min(300)]);
            return;
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut updated = 0usize;

    for item in &enrichments {
        let post_id = match item.get("post_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        let relevance_score = item.get("relevance_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(5.0)
            .max(0.0).min(10.0);

        let why_relevant = item.get("why_relevant").and_then(|v| v.as_str()).unwrap_or("");
        let website_fit = item.get("website_fit").and_then(|v| v.as_str()).unwrap_or("");
        let reply_text = item.get("reply_text").and_then(|v| v.as_str()).unwrap_or("");
        let pain_points_json = item.get("key_pain_points")
            .and_then(|v| v.as_array())
            .map(|arr| serde_json::to_string(arr).unwrap_or_else(|_| "[]".to_string()))
            .unwrap_or_else(|| "[]".to_string());

        // Fetch current engagement/accessibility scores to recalculate final_score.
        let (engagement_score, accessibility_score): (f64, f64) = rows.iter()
            .find(|(pid, _, _, _, _)| pid == post_id)
            .map(|(_, _, _, eng, acc)| (eng.unwrap_or(5.0), acc.unwrap_or(5.0)))
            .unwrap_or((5.0, 5.0));

        let final_score = (relevance_score + engagement_score + accessibility_score) / 3.0;
        let severity = if final_score >= 8.5 { "CRITICAL" }
            else if final_score >= 7.0 { "HIGH" }
            else if final_score >= 5.0 { "MEDIUM" }
            else { "LOW" };

        match conn.execute(
            "UPDATE reddit_opportunities \
             SET relevance_score=?1, why_relevant=?2, key_pain_points=?3, website_fit=?4, \
                 final_score=?5, severity=?6, reply_text=?7, mention_stance=?8, updated_at=?9 \
             WHERE post_id=?10 AND project_id=?11",
            rusqlite::params![
                relevance_score, why_relevant, pain_points_json, website_fit,
                final_score, severity,
                if reply_text.is_empty() { None } else { Some(reply_text) },
                &mention_stance_str,
                now, post_id, project_id
            ],
        ) {
            Ok(n) if n > 0 => { updated += 1; }
            Ok(_) => { log::warn!("[reddit_enrich] post_id={} not found in DB", post_id); }
            Err(e) => { log::warn!("[reddit_enrich] update failed for {}: {}", post_id, e); }
        }
    }

    log::info!("[reddit_enrich] enriched+drafted {} / {} posts for project={}", updated, rows.len(), project_id);
}

/// Extract a JSON array from agent output that may be wrapped in markdown code fences.
fn extract_json_array(output: &str) -> String {
    let trimmed = output.trim();
    // Try to strip markdown fences: ```json ... ``` or ``` ... ```
    let inner = if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    inner.to_string()
}
