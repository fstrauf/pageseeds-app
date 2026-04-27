use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

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

/// Step runner for `content_review_recommend` steps.
///
/// 1. Reads content_audit.json + articles.json
/// 2. Selects top 5 priority articles via `select_priority_articles`
/// 3. Builds structured context with source excerpts
/// 4. Makes one agent call with a targeted structured prompt
/// 5. Writes recommendations.json to the automation dir
#[allow(deprecated)]
pub(crate) fn exec_content_review_recommend(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    let audit_path = paths.automation_dir.join("content_audit.json");
    let audit: serde_json::Value = match crate::engine::exec::common::read_json(&audit_path, "content_audit.json") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let articles_path = paths.automation_dir.join("articles.json");
    let articles_doc: serde_json::Value = match crate::engine::exec::common::read_json(&articles_path, "articles.json") {
        Ok(v) => v,
        Err(e) => return e,
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

    let rec = crate::engine::text::extract_json(&raw_output)
        .unwrap_or_else(|| serde_json::json!({
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
