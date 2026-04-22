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
const REVIEW_REVISIT_STALE_DAYS: i64 = 45;
const REVIEW_REVISIT_REGRESSION_DAYS: i64 = 14;

fn reviewed_article_revisit_reason(
    review_status: &str,
    last_reviewed_at: &str,
    now: chrono::DateTime<chrono::Utc>,
    has_regression_signal: bool,
) -> Option<&'static str> {
    let has_review_history = review_status == "reviewed" || !last_reviewed_at.trim().is_empty();
    if !has_review_history {
        return None;
    }

    let review_age_days = chrono::DateTime::parse_from_rfc3339(last_reviewed_at)
        .ok()
        .map(|reviewed_at| now.signed_duration_since(reviewed_at.with_timezone(&chrono::Utc)).num_days().max(0));

    match review_age_days {
        Some(days) if days >= REVIEW_REVISIT_STALE_DAYS => Some("stale"),
        Some(days) if days >= REVIEW_REVISIT_REGRESSION_DAYS && has_regression_signal => Some("regressed"),
        Some(_) => None,
        None => Some("stale"),
    }
}

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
    let now = chrono::Utc::now();
    let mut backlog_candidates: Vec<(i64, serde_json::Value)> = Vec::new();
    let mut revisit_candidates: Vec<(i64, serde_json::Value)> = Vec::new();

    for article in raw_articles {
        let status = article["status"].as_str().unwrap_or("").to_lowercase();
        let review_status = article["review_status"].as_str().unwrap_or("").to_lowercase();
        let last_reviewed_at = article["last_reviewed_at"].as_str().unwrap_or("").trim();
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
        let quick_ctr_opportunity = pos >= 5.0 && pos <= 20.0 && impressions > 200.0 && ctr < 0.03;
        if quick_ctr_opportunity {
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

        let has_regression_signal = quick_ctr_opportunity
            || health == "poor"
            || checks_failed >= 3
            || health_score <= 70;

        let has_review_history = review_status == "reviewed" || !last_reviewed_at.is_empty();

        let mut enriched = article.clone();
        enriched["_failed_checks"] = serde_json::json!(failed_checks);

        if has_review_history {
            let Some(reason) = reviewed_article_revisit_reason(
                &review_status,
                last_reviewed_at,
                now,
                has_regression_signal,
            ) else {
                continue;
            };
            enriched["_review_bucket"] = serde_json::json!("revisit");
            enriched["_review_reason"] = serde_json::json!(reason);
            revisit_candidates.push((score, enriched));
        } else {
            enriched["_review_bucket"] = serde_json::json!("backlog");
            backlog_candidates.push((score, enriched));
        }
    }

    backlog_candidates.sort_by(|a, b| b.0.cmp(&a.0));
    revisit_candidates.sort_by(|a, b| b.0.cmp(&a.0));

    let mut selected: Vec<serde_json::Value> = backlog_candidates
        .into_iter()
        .take(max_items)
        .map(|(_, article)| article)
        .collect();

    if selected.len() < max_items {
        selected.extend(
            revisit_candidates
                .into_iter()
                .take(max_items - selected.len())
                .map(|(_, article)| article),
        );
    }

    selected
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

fn recommendation_article_id(article: &serde_json::Value) -> Option<i64> {
    match article.get("article_id") {
        Some(serde_json::Value::String(id)) => {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse::<i64>().ok()
            }
        }
        Some(serde_json::Value::Number(id)) => id.as_i64(),
        _ => None,
    }
}

fn fix_content_article_id(task: &Task) -> Option<i64> {
    task.artifacts
        .iter()
        .find_map(|artifact| {
            artifact
                .content
                .as_deref()
                .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok())
                .and_then(|article| recommendation_article_id(&article))
                .or_else(|| {
                    artifact
                        .key
                        .strip_prefix("recommendations_")
                        .and_then(|suffix| suffix.parse::<i64>().ok())
                })
        })
}

fn sync_article_review_state_to_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> crate::error::Result<()> {
    let json = crate::db::export::export_articles(conn, project_id)?;
    let out_path = std::path::Path::new(project_path)
        .join(".github")
        .join("automation")
        .join("articles.json");
    std::fs::create_dir_all(out_path.parent().unwrap())?;
    std::fs::write(out_path, json)?;
    Ok(())
}

fn mark_articles_in_review(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    article_ids: &[i64],
) -> crate::error::Result<usize> {
    if article_ids.is_empty() {
        return Ok(0);
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut updated = 0usize;
    for article_id in article_ids {
        updated += conn.execute(
            "UPDATE articles
             SET review_status = 'in_review', review_started_at = ?1
             WHERE id = ?2 AND project_id = ?3",
            rusqlite::params![&now, article_id, project_id],
        )?;
    }

    if updated > 0 {
        sync_article_review_state_to_repo(conn, project_id, project_path)?;
    }

    Ok(updated)
}

pub(crate) fn mark_fix_content_article_reviewed(
    conn: &Connection,
    task: &Task,
    project_path: &str,
) -> crate::error::Result<Option<i64>> {
    let Some(article_id) = fix_content_article_id(task) else {
        return Ok(None);
    };

    let now = chrono::Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE articles
         SET review_status = 'reviewed',
             review_started_at = NULL,
             last_reviewed_at = ?1,
             review_count = COALESCE(review_count, 0) + 1
         WHERE id = ?2 AND project_id = ?3",
        rusqlite::params![&now, article_id, &task.project_id],
    )?;

    if rows > 0 {
        sync_article_review_state_to_repo(conn, &task.project_id, project_path)?;
        Ok(Some(article_id))
    } else {
        Ok(None)
    }
}

/// After a successful content review, create individual `fix_content_article` tasks
/// for each article in recommendations.json.
///
/// This replaces the previous monolithic `content_review_apply` approach with
/// per-article tasks that can be run independently.
///
/// Skips if recommendations.json is absent (review found nothing).
pub(crate) fn create_content_review_apply_task(conn: &Connection, parent_task: &Task, project_path: &str) -> Vec<String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, TaskArtifact, TaskStatus};
    use std::collections::HashSet;

    let paths = ProjectPaths::from_path(project_path);
    let rec_path = paths.automation_dir.join("recommendations.json");

    let rec_str = match std::fs::read_to_string(&rec_path) {
        Ok(s) => s,
        Err(_) => {
            log::info!("[create_apply_task] recommendations.json not found — no apply tasks created");
            return Vec::new();
        }
    };
    let rec: serde_json::Value = match serde_json::from_str(&rec_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[create_apply_task] failed to parse recommendations.json: {}", e);
            return Vec::new();
        }
    };

    let articles = match rec["articles"].as_array() {
        Some(a) if !a.is_empty() => a,
        _ => {
            log::info!("[create_apply_task] no articles in recommendations — skipping");
            return Vec::new();
        }
    };

    let mut created_task_ids = Vec::new();
    let mut seen_article_ids = HashSet::new();
    let mut in_review_article_ids = Vec::new();

    for article in articles {
        let Some(article_id) = recommendation_article_id(article) else {
            let article_title = article["article_title"].as_str().unwrap_or("article");
            log::warn!(
                "[create_apply_task] skipping article '{}' with missing/invalid article_id",
                article_title
            );
            continue;
        };

        let article_title = article["article_title"].as_str().unwrap_or("article");
        let article_file = article["article_file"].as_str().unwrap_or("");

        if !seen_article_ids.insert(article_id.clone()) {
            log::warn!(
                "[create_apply_task] skipping duplicate recommendation for article '{}' ({})",
                article_title,
                article_id
            );
            continue;
        }

        // Extract specific recommendations for this article
        let article_rec = serde_json::json!({
            "article_id": article["article_id"].clone(),
            "article_title": article_title,
            "article_file": article_file,
            "suggestions": article["suggestions"],
        });
        let article_rec_str = serde_json::to_string_pretty(&article_rec).unwrap_or_default();
        let article_id_str = article_id.to_string();

        let title = format!("Fix: {}", article_title);

        // Create individual artifact for this article
        let artifact = TaskArtifact {
            key: format!("recommendations_{}", article_id_str),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("content_review".to_string()),
            content: Some(article_rec_str),
        };

        // Idempotency key per article: content_review_apply:{project_id}:{article_id}
        let idempotency_key = format!("content_review_apply:{}:{}", parent_task.project_id, article_id_str);

        // Calculate priority based on issue count
        let issue_count = article["suggestions"].as_array().map(|s| s.len()).unwrap_or(0);
        let priority = if issue_count >= 5 {
            Priority::High
        } else if issue_count >= 2 {
            Priority::Medium
        } else {
            Priority::Low
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_content_article".to_string(),
            title: Some(title),
            description: Some(format!(
                "Apply SEO recommendations to '{}' ({} issue{}). \
                 File: {}",
                article_title,
                issue_count,
                if issue_count == 1 { "" } else { "s" },
                article_file
            )),
            phase: Some("implementation".to_string()),
            execution_mode: Some(ExecutionMode::Batchable),
            priority,
            agent_policy: AgentPolicy::Required,
            idempotency_key: Some(idempotency_key),
            artifacts: vec![artifact],
            depends_on: vec![],
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                if matches!(task.status, TaskStatus::Todo | TaskStatus::InProgress | TaskStatus::Review) {
                    log::info!(
                        "[create_apply_task] created {} for article '{}' ({} issues)",
                        task.id, article_title, issue_count
                    );
                    created_task_ids.push(task.id);
                    in_review_article_ids.push(article_id);
                } else {
                    log::info!(
                        "[create_apply_task] existing task {} for article '{}' is {:?}; not reopening review",
                        task.id,
                        article_title,
                        task.status
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "[create_apply_task] failed to create task for article '{}': {}",
                    article_title, e
                );
            }
        }
    }

    if let Err(e) = mark_articles_in_review(conn, &parent_task.project_id, project_path, &in_review_article_ids) {
        log::warn!("[create_apply_task] failed to mark articles in_review: {}", e);
    }

    log::info!(
        "[create_apply_task] created {} individual fix task(s) from content review",
        created_task_ids.len()
    );

    created_task_ids
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::task_store;
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, TaskRun, TaskStatus};
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TempProjectDir {
        path: PathBuf,
    }

    impl TempProjectDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pageseeds-content-review-{}",
                Uuid::new_v4()
            ));
            fs::create_dir_all(path.join(".github").join("automation")).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempProjectDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                active INTEGER DEFAULT 1
            );
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                execution_mode TEXT NOT NULL DEFAULT 'manual',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT,
                description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER DEFAULT 0,
                run_last_error TEXT,
                run_provider TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE task_idempotency_keys (
                key TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                attempt INTEGER NOT NULL,
                provider TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                success INTEGER,
                error TEXT
            );
            CREATE TABLE articles (
                id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '',
                file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT,
                keyword_difficulty TEXT,
                target_volume INTEGER DEFAULT 0,
                published_date TEXT,
                word_count INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT,
                review_started_at TEXT,
                last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT,
                project_id TEXT NOT NULL,
                PRIMARY KEY (id, project_id)
            );
            CREATE TABLE articles_meta (
                project_id TEXT PRIMARY KEY,
                next_article_id INTEGER NOT NULL DEFAULT 1
            );",
        )
        .unwrap();
        conn
    }

    fn create_test_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![id, "Test Project", path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES (?1, 200)",
            [id],
        )
        .unwrap();
    }

    fn insert_test_article(conn: &Connection, project_id: &str, id: i64, status: &str, review_status: Option<&str>) {
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                review_status, review_started_at, last_reviewed_at, review_count,
                content_gaps_addressed, estimated_traffic_monthly, project_id
             ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, NULL, 0, ?5, ?6, NULL, NULL, 0, '[]', NULL, ?7)",
            rusqlite::params![
                id,
                format!("Article {id}"),
                format!("article-{id}"),
                format!("./content/{id}_article.mdx"),
                status,
                review_status,
                project_id,
            ],
        )
        .unwrap();
    }

    fn make_parent_task(project_id: &str) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: format!("task-{}", Uuid::new_v4()),
            project_id: project_id.to_string(),
            task_type: "content_review".to_string(),
            phase: "investigation".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Batchable,
            agent_policy: AgentPolicy::Required,
            title: Some("Content Review".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn write_recommendations(project_dir: &Path, recommendations: serde_json::Value) {
        let path = project_dir
            .join(".github")
            .join("automation")
            .join("recommendations.json");
        fs::write(path, serde_json::to_string_pretty(&recommendations).unwrap()).unwrap();
    }

    fn idempotency_keys(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT key FROM task_idempotency_keys ORDER BY key")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    }

    #[test]
    fn recommendation_article_id_accepts_strings_and_numbers() {
        assert_eq!(
            recommendation_article_id(&json!({ "article_id": "109" })),
            Some(109)
        );
        assert_eq!(
            recommendation_article_id(&json!({ "article_id": 111 })),
            Some(111)
        );
        assert_eq!(recommendation_article_id(&json!({ "article_id": "   " })), None);
        assert_eq!(recommendation_article_id(&json!({})), None);
    }

    fn reviewed_at(days_ago: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::days(days_ago)).to_rfc3339()
    }

    #[test]
    fn select_priority_articles_prioritizes_unreviewed_backlog_before_reviewed_revisits() {
        let raw_articles = vec![
            json!({
                "id": 1,
                "title": "Reviewed winner",
                "file": "./content/1_reviewed.mdx",
                "url_slug": "reviewed-winner",
                "status": "published",
                "review_status": "reviewed",
                "last_reviewed_at": reviewed_at(60),
                "gsc": { "avg_position": 8.0, "impressions": 800.0, "ctr": 0.0 }
            }),
            json!({
                "id": 2,
                "title": "Unreviewed backlog",
                "file": "./content/2_unreviewed.mdx",
                "url_slug": "unreviewed-backlog",
                "status": "published",
                "gsc": { "avg_position": 2.0, "impressions": 10.0, "ctr": 0.2 }
            })
        ];

        let audit_articles = vec![
            json!({
                "file": "./content/1_reviewed.mdx",
                "health": "poor",
                "checks_failed": 6,
                "health_score": 40,
                "checks": {}
            }),
            json!({
                "file": "./content/2_unreviewed.mdx",
                "health": "good",
                "checks_failed": 0,
                "health_score": 100,
                "checks": {}
            })
        ];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0]["id"], 2);
        assert_eq!(selected[0]["_review_bucket"], "backlog");
        assert_eq!(selected[1]["id"], 1);
        assert_eq!(selected[1]["_review_reason"], "stale");
    }

    #[test]
    fn select_priority_articles_backfills_with_stale_reviewed_articles() {
        let raw_articles = vec![json!({
            "id": 1,
            "title": "Stale reviewed article",
            "file": "./content/1_reviewed.mdx",
            "url_slug": "stale-reviewed",
            "status": "published",
            "review_status": "reviewed",
            "last_reviewed_at": reviewed_at(90),
            "gsc": { "avg_position": 2.0, "impressions": 50.0, "ctr": 0.10 }
        })];

        let audit_articles = vec![json!({
            "file": "./content/1_reviewed.mdx",
            "health": "good",
            "checks_failed": 0,
            "health_score": 100,
            "checks": {}
        })];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 5);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["id"], 1);
        assert_eq!(selected[0]["_review_reason"], "stale");
    }

    #[test]
    fn select_priority_articles_allows_regressed_reviewed_articles_after_cooldown() {
        let raw_articles = vec![json!({
            "id": 1,
            "title": "Regressed reviewed article",
            "file": "./content/1_reviewed.mdx",
            "url_slug": "regressed-reviewed",
            "status": "published",
            "review_status": "reviewed",
            "last_reviewed_at": reviewed_at(20),
            "gsc": { "avg_position": 8.0, "impressions": 900.0, "ctr": 0.01 }
        })];

        let audit_articles = vec![json!({
            "file": "./content/1_reviewed.mdx",
            "health": "good",
            "checks_failed": 0,
            "health_score": 100,
            "checks": {}
        })];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 5);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0]["id"], 1);
        assert_eq!(selected[0]["_review_reason"], "regressed");
    }

    #[test]
    fn select_priority_articles_keeps_recent_reviewed_regressions_on_cooldown() {
        let raw_articles = vec![json!({
            "id": 1,
            "title": "Recently reviewed article",
            "file": "./content/1_reviewed.mdx",
            "url_slug": "recent-reviewed",
            "status": "published",
            "review_status": "reviewed",
            "last_reviewed_at": reviewed_at(5),
            "gsc": { "avg_position": 9.0, "impressions": 1200.0, "ctr": 0.01 }
        })];

        let audit_articles = vec![json!({
            "file": "./content/1_reviewed.mdx",
            "health": "poor",
            "checks_failed": 5,
            "health_score": 45,
            "checks": {}
        })];

        let selected = select_priority_articles(&raw_articles, &audit_articles, 5);
        assert!(selected.is_empty());
    }

    #[test]
    fn create_content_review_apply_task_uses_numeric_article_ids_in_idempotency_keys() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", None);
        insert_test_article(&conn, "proj1", 111, "published", None);

        write_recommendations(
            project_dir.path(),
            json!({
                "articles": [
                    {
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": [{ "category": "title" }]
                    },
                    {
                        "article_id": 111,
                        "article_title": "Beta",
                        "article_file": "./content/111_beta.mdx",
                        "suggestions": [{ "category": "meta_description" }, { "category": "cta" }]
                    }
                ]
            }),
        );

        let parent = make_parent_task("proj1");
        let created = create_content_review_apply_task(&conn, &parent, &project_path);

        assert_eq!(created.len(), 2);
        assert_eq!(
            idempotency_keys(&conn),
            vec![
                "content_review_apply:proj1:109".to_string(),
                "content_review_apply:proj1:111".to_string(),
            ]
        );

        let tasks = task_store::list_tasks(&conn, "proj1").unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().all(|task| task.task_type == "fix_content_article"));
        assert!(tasks.iter().all(|task| {
            task.artifacts
                .iter()
                .any(|artifact| artifact.key == "recommendations_109" || artifact.key == "recommendations_111")
        }));

        let articles = task_store::list_articles(&conn, "proj1").unwrap();
        assert!(articles.iter().all(|article| article.review_status.as_deref() == Some("in_review")));

        let exported: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                project_dir
                    .path()
                    .join(".github")
                    .join("automation")
                    .join("articles.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let exported_articles = exported["articles"].as_array().unwrap();
        assert!(exported_articles.iter().all(|article| article["review_status"] == "in_review"));
    }

    #[test]
    fn create_content_review_apply_task_skips_invalid_and_duplicate_article_ids() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", None);

        write_recommendations(
            project_dir.path(),
            json!({
                "articles": [
                    {
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": [{ "category": "title" }]
                    },
                    {
                        "article_id": 109,
                        "article_title": "Alpha Duplicate",
                        "article_file": "./content/109_alpha_dup.mdx",
                        "suggestions": [{ "category": "cta" }]
                    },
                    {
                        "article_title": "Missing ID",
                        "article_file": "./content/missing_id.mdx",
                        "suggestions": [{ "category": "faq" }]
                    }
                ]
            }),
        );

        let parent = make_parent_task("proj1");
        let created = create_content_review_apply_task(&conn, &parent, &project_path);

        assert_eq!(created.len(), 1);
        assert_eq!(
            idempotency_keys(&conn),
            vec!["content_review_apply:proj1:109".to_string()]
        );
    }

    #[test]
    fn mark_fix_content_article_reviewed_updates_article_state_and_export() {
        let conn = in_memory_db();
        let project_dir = TempProjectDir::new();
        let project_path = project_dir.path().to_string_lossy().to_string();
        create_test_project(&conn, "proj1", &project_path);
        insert_test_article(&conn, "proj1", 109, "published", Some("in_review"));

        let task = Task {
            id: format!("task-{}", Uuid::new_v4()),
            project_id: "proj1".to_string(),
            task_type: "fix_content_article".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Batchable,
            agent_policy: AgentPolicy::Required,
            title: Some("Fix: Alpha".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "recommendations_109".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("content_review".to_string()),
                content: Some(
                    serde_json::to_string(&json!({
                        "article_id": 109,
                        "article_title": "Alpha",
                        "article_file": "./content/109_alpha.mdx",
                        "suggestions": []
                    }))
                    .unwrap(),
                ),
            }],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let article_id = mark_fix_content_article_reviewed(&conn, &task, &project_path).unwrap();
        assert_eq!(article_id, Some(109));

        let articles = task_store::list_articles(&conn, "proj1").unwrap();
        let article = articles.iter().find(|article| article.id == 109).unwrap();
        assert_eq!(article.review_status.as_deref(), Some("reviewed"));
        assert_eq!(article.review_count, 1);
        assert!(article.last_reviewed_at.is_some());
        assert!(article.review_started_at.is_none());

        let exported: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                project_dir
                    .path()
                    .join(".github")
                    .join("automation")
                    .join("articles.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let exported_article = exported["articles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|article| article["id"] == 109)
            .unwrap();
        assert_eq!(exported_article["review_status"], "reviewed");
        assert_eq!(exported_article["review_count"], 1);
        assert!(exported_article.get("last_reviewed_at").is_some());
    }
}
