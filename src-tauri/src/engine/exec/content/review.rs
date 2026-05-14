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
        .map(|reviewed_at| {
            now.signed_duration_since(reviewed_at.with_timezone(&chrono::Utc))
                .num_days()
                .max(0)
        });

    match review_age_days {
        Some(days) if days >= REVIEW_REVISIT_STALE_DAYS => Some("stale"),
        Some(days) if days >= REVIEW_REVISIT_REGRESSION_DAYS && has_regression_signal => {
            Some("regressed")
        }
        Some(_) => None,
        None => Some("stale"),
    }
}

pub(crate) fn select_priority_articles(
    raw_articles: &[serde_json::Value],
    audit_articles: &[serde_json::Value],
    max_items: usize,
) -> Vec<serde_json::Value> {
    let mut audit_by_file: std::collections::HashMap<String, &serde_json::Value> =
        Default::default();
    let mut audit_by_slug: std::collections::HashMap<String, &serde_json::Value> =
        Default::default();
    for a in audit_articles {
        if let Some(f) = a["file"].as_str() {
            if !f.is_empty() {
                audit_by_file.insert(f.to_string(), a);
            }
        }
        if let Some(s) = a["url_slug"].as_str() {
            if !s.is_empty() {
                audit_by_slug.insert(s.to_string(), a);
            }
        }
    }

    let null_value = serde_json::Value::Null;
    let now = chrono::Utc::now();
    let mut backlog_candidates: Vec<(i64, serde_json::Value)> = Vec::new();
    let mut revisit_candidates: Vec<(i64, serde_json::Value)> = Vec::new();

    for article in raw_articles {
        let status = article["status"].as_str().unwrap_or("").to_lowercase();
        let review_status = article["review_status"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
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
        let audit_row: &serde_json::Value = audit_by_file
            .get(&file_rel)
            .or_else(|| audit_by_slug.get(url_slug))
            .copied()
            .unwrap_or(&null_value);

        let health = audit_row["health"].as_str().unwrap_or("").to_lowercase();
        let checks_failed = audit_row["checks_failed"].as_i64().unwrap_or(0);
        let health_score = audit_row["health_score"].as_i64().unwrap_or(0);

        let failed_checks: Vec<serde_json::Value> = audit_row["checks"]
            .as_object()
            .map(|checks| {
                checks
                    .iter()
                    .filter(|(_, v)| v["pass"].as_bool() == Some(false))
                    .map(|(k, v)| {
                        serde_json::json!({
                            "check_id": k,
                            "label": v["label"].as_str().unwrap_or(k),
                        })
                    })
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

        let has_regression_signal =
            quick_ctr_opportunity || health == "poor" || checks_failed >= 3 || health_score <= 70;

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
    let articles: Vec<serde_json::Value> = selected
        .iter()
        .filter_map(|article| {
            let file_ref = article["file"].as_str().unwrap_or("");
            if file_ref.is_empty() {
                return None;
            }
            let source = crate::engine::exec::utils::read_source_file(repo_root, file_ref);
            let source_excerpt = source
                .as_deref()
                .map(|s| {
                    s.char_indices()
                        .nth(max_excerpt_chars)
                        .map_or(s, |(i, _)| &s[..i])
                })
                .unwrap_or("")
                .to_string();
            Some(serde_json::json!({
                "article_id": article["id"],
                "article_title": article["title"],
                "article_file": file_ref,
                "url_slug": article["url_slug"],
                "target_keyword": article["target_keyword"],
                "published_date": article["published_date"],
                "gsc_snapshot": article["gsc"],
                "failed_checks": article["_failed_checks"],
                "source_excerpt": source_excerpt,
            }))
        })
        .collect();
    serde_json::json!({
        "generated_at": now,
        "articles": articles,
    })
}

/// Build the structured agent prompt for the content review recommendations step.
pub(crate) fn build_review_prompt(context: &serde_json::Value) -> String {
    let context_json = serde_json::to_string_pretty(context).unwrap_or_default();
    format!(
        r#"Analyze the following articles and generate specific, actionable SEO recommendations.

Input context:
{context_json}

For each article, examine:
1. Title and H1 quality — keyword presence, clarity, length
2. Meta description — presence, length (50-155 chars), keyword inclusion
3. Introduction — engagement, keyword placement
4. Content structure — H2 headings, readability
5. Internal links — quantity, relevance
6. EEAT signals — credibility, authoritativeness
7. Call-to-action — clarity and placement
8. Year freshness — compare any year mentioned in the title or H1 against the published_date. If they differ (e.g., title says "2025" but published_date is "2026-03-15"), suggest either:
   - Update the title/H1 year to match the published date
   - Update the published_date to match the title year (only if content is genuinely about the older year)

For each suggestion, use one of these categories: title, meta_description, intro, h1, internal_links, faq, eeat, cta, date.

Requirements:
- 4-8 actionable suggestions per article.
- Use only the provided context.
- Be specific: include the exact current text and proposed replacement."#,
        context_json = context_json,
    )
}

/// Build a prompt for a single article — used in per-article extraction.
pub(crate) fn build_single_article_prompt(article: &serde_json::Value) -> String {
    let article_json = serde_json::to_string_pretty(article).unwrap_or_default();
    format!(
        r#"Analyze the following article and generate specific, actionable SEO recommendations.

Input context:
{article_json}

Examine:
1. Title and H1 quality — keyword presence, clarity, length
2. Meta description — presence, length (50-155 chars), keyword inclusion
3. Introduction — engagement, keyword placement
4. Content structure — H2 headings, readability
5. Internal links — quantity, relevance
6. EEAT signals — credibility, authoritativeness
7. Call-to-action — clarity and placement
8. Year freshness — compare any year mentioned in the title or H1 against the published_date

For each suggestion, use one of these categories: title, meta_description, intro, h1, internal_links, faq, eeat, cta, date.

Requirements:
- 4-8 actionable suggestions for THIS article only.
- Use only the provided context.
- Be specific: include the exact current text and proposed replacement."#,
        article_json = article_json,
    )
}

/// Step runner for `content_review_recommend` steps.
///
/// 1. Reads content_audit.json + articles.json
/// 2. Selects top 5 priority articles via `select_priority_articles`
/// 3. Builds structured context with source excerpts
/// 4. Makes one rig `Extractor<T>` call for guaranteed structured JSON output
/// 5. Writes recommendations.json to the automation dir
pub(crate) async fn exec_content_review_recommend(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    let audit_path = paths.automation_dir.join("content_audit.json");
    let audit: serde_json::Value =
        match crate::engine::exec::common::read_json(&audit_path, "content_audit.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    let articles_path = paths.automation_dir.join("articles.json");
    let articles_doc: serde_json::Value =
        match crate::engine::exec::common::read_json(&articles_path, "articles.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    let empty_vec: Vec<serde_json::Value> = Vec::new();
    let raw_articles_ref = if articles_doc.is_array() {
        articles_doc.as_array().unwrap_or(&empty_vec)
    } else {
        articles_doc
            .get("articles")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty_vec)
    };
    let audit_articles = audit
        .get("articles")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty_vec);

    // ── Filter out articles that Google cannot see (not indexed) ──────────────
    // Load gsc_collection.json to cross-reference indexing status.
    let gsc_collection_path = paths.automation_dir.join("gsc_collection.json");
    let mut non_indexed_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(gsc_raw) = std::fs::read_to_string(&gsc_collection_path) {
        if let Ok(gsc_doc) = serde_json::from_str::<serde_json::Value>(&gsc_raw) {
            if let Some(items) = gsc_doc["items"].as_array() {
                for item in items {
                    if item["reason_code"].as_str().unwrap_or("") != "indexed_pass" {
                        if let Some(url) = item["url"].as_str() {
                            let slug = crate::content::slug::extract_slug_from_url(url);
                            // Also index last path segment for flat slug matching
                            let last = slug.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
                            if !slug.is_empty() {
                                non_indexed_slugs.insert(slug);
                            }
                            if !last.is_empty() {
                                non_indexed_slugs.insert(last);
                            }
                        }
                    }
                }
            }
        }
    }

    let raw_articles: Vec<serde_json::Value> = raw_articles_ref
        .iter()
        .filter(|article| {
            let slug = article["url_slug"].as_str().unwrap_or("");
            if non_indexed_slugs.contains(slug) {
                log::info!(
                    "[content_review_recommend] skipping non-indexed article: {}",
                    slug
                );
                return false;
            }
            true
        })
        .cloned()
        .collect();

    let selected = select_priority_articles(&raw_articles, audit_articles, 5);
    log::info!(
        "[content_review_recommend] {} priority articles selected (project={})",
        selected.len(),
        task.project_id
    );

    if selected.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No eligible articles found for review — all healthy or already in-review"
                .to_string(),
            output: Some(
                serde_json::json!({
                    "generated_at": chrono::Utc::now().to_rfc3339(),
                    "total_articles": 0,
                    "articles": []
                })
                .to_string(),
            ),
        };
    }

    // Build individual contexts — one per article, full excerpt, no cap
    let contexts: Vec<serde_json::Value> = selected
        .iter()
        .filter_map(|article| {
            let file_ref = article["file"].as_str().unwrap_or("");
            if file_ref.is_empty() {
                return None;
            }
            let source = crate::engine::exec::utils::read_source_file(repo_root, file_ref);
            let source_excerpt = source
                .as_deref()
                .map(|s| {
                    s.char_indices()
                        .nth(2600)
                        .map_or(s, |(i, _)| &s[..i])
                        .to_string()
                })
                .unwrap_or_default();
            Some(serde_json::json!({
                "article_id": article["id"],
                "article_title": article["title"],
                "article_file": file_ref,
                "url_slug": article["url_slug"],
                "target_keyword": article["target_keyword"],
                "published_date": article["published_date"],
                "gsc_snapshot": article["gsc"],
                "failed_checks": article["_failed_checks"],
                "source_excerpt": source_excerpt,
            }))
        })
        .collect();

    if contexts.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "Could not read source files for selected articles — check file paths in articles.json".to_string(),
            output: None,
        };
    }

    // Process articles individually with concurrency limit 2 (matches bridge limit)
    let preamble = "You are an expert SEO content reviewer. Analyze the single article below and generate structured recommendations using the submit tool.";
    let mut all_articles: Vec<crate::models::content_review::ReviewArticleRecommendation> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    async fn extract_one(
        agent_provider: &str,
        ctx: &serde_json::Value,
        preamble: &str,
    ) -> Result<crate::models::content_review::SingleArticleRecommendations, String> {
        let prompt = build_single_article_prompt(ctx);
        log::info!(
            "[content_review_recommend] extracting article {} ({} chars prompt)",
            ctx["article_id"].as_i64().unwrap_or(0),
            prompt.len()
        );
        crate::rig::extraction::extract_structured::<
            crate::models::content_review::SingleArticleRecommendations,
        >(agent_provider, &prompt, Some(preamble), Some("direct"), None)
        .await
    }

    for chunk in contexts.chunks(2) {
        match chunk.len() {
            1 => {
                let ctx = &chunk[0];
                let article_id = ctx["article_id"].as_i64().unwrap_or(0);
                match extract_one(agent_provider, ctx, preamble).await {
                    Ok(single) => {
                        let article_rec = crate::models::content_review::ReviewArticleRecommendation {
                            article_id,
                            article_title: ctx["article_title"].as_str().unwrap_or("").to_string(),
                            article_file: ctx["article_file"].as_str().unwrap_or("").to_string(),
                            url_slug: ctx["url_slug"].as_str().unwrap_or("").to_string(),
                            target_keyword: ctx["target_keyword"].as_str().map(|s| s.to_string()),
                            suggestions: single.suggestions,
                        };
                        all_articles.push(article_rec);
                        log::info!(
                            "[content_review_recommend] article {} extracted successfully",
                            article_id
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[content_review_recommend] article {} failed: {}",
                            article_id,
                            e
                        );
                        failed.push(format!("article {}: {}", article_id, e));
                    }
                }
            }
            2 => {
                let (r1, r2) = tokio::join!(
                    extract_one(agent_provider, &chunk[0], preamble),
                    extract_one(agent_provider, &chunk[1], preamble),
                );
                for (ctx, result) in chunk.iter().zip([r1, r2].into_iter()) {
                    let article_id = ctx["article_id"].as_i64().unwrap_or(0);
                    match result {
                        Ok(single) => {
                            let article_rec = crate::models::content_review::ReviewArticleRecommendation {
                                article_id,
                                article_title: ctx["article_title"].as_str().unwrap_or("").to_string(),
                                article_file: ctx["article_file"].as_str().unwrap_or("").to_string(),
                                url_slug: ctx["url_slug"].as_str().unwrap_or("").to_string(),
                                target_keyword: ctx["target_keyword"].as_str().map(|s| s.to_string()),
                                suggestions: single.suggestions,
                            };
                            all_articles.push(article_rec);
                            log::info!(
                                "[content_review_recommend] article {} extracted successfully",
                                article_id
                            );
                        }
                        Err(e) => {
                            log::warn!(
                                "[content_review_recommend] article {} failed: {}",
                                article_id,
                                e
                            );
                            failed.push(format!("article {}: {}", article_id, e));
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    let rec = crate::models::content_review::ContentReviewRecommendations {
        generated_at: chrono::Utc::now().to_rfc3339(),
        total_articles: all_articles.len(),
        articles: all_articles,
    };

    let rec_value = match serde_json::to_value(&rec) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to serialize recommendations: {}", e),
                output: None,
            }
        }
    };

    let rec_path = paths.automation_dir.join("recommendations.json");
    let rec_str = serde_json::to_string_pretty(&rec_value).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&rec_path, &rec_str) {
        log::warn!(
            "[content_review_recommend] failed to write recommendations.json: {}",
            e
        );
    } else {
        log::info!(
            "[content_review_recommend] wrote recommendations.json ({} articles)",
            rec.articles.len()
        );
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Recommendations generated for {} / {} selected articles",
            rec.articles.len(),
            selected.len()
        ),
        output: Some(rec_str),
    }
}
