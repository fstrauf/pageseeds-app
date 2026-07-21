use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

const REVIEW_REVISIT_STALE_DAYS: i64 = 45;
const REVIEW_REVISIT_REGRESSION_DAYS: i64 = 14;
const ARTICLE_FIX_COOLDOWN_DAYS: i64 = 30;
/// Cap for the full-body content view passed to the recommender. Articles are
/// typically 1,500–3,000 words (< 20k chars), so most bodies fit untruncated;
/// when truncation kicks in, the heading outline + link inventory still give
/// the agent the article's structure.
const RECOMMEND_BODY_MAX_CHARS: usize = 20_000;
/// Number of top GSC queries (from `ctr_query_metrics`) attached per article.
const TOP_QUERIES_PER_ARTICLE: usize = 10;

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
        let last_edited_at = article["last_edited_at"].as_str().unwrap_or("").trim();
        let file_rel = article["file"].as_str().unwrap_or("").to_string();
        if status == "draft" || review_status == "in_review" || file_rel.is_empty() {
            continue;
        }

        // Skip articles that were edited (fixed) recently — give them time to mature in GSC
        if !last_edited_at.is_empty() {
            if let Ok(edited) = chrono::DateTime::parse_from_rfc3339(last_edited_at) {
                let days_since_edit = now
                    .signed_duration_since(edited.with_timezone(&chrono::Utc))
                    .num_days()
                    .max(0);
                if days_since_edit < ARTICLE_FIX_COOLDOWN_DAYS {
                    continue;
                }
            }
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
        let quality_score = audit_row["quality_score"].as_i64().unwrap_or(0);
        let quality_grade = audit_row["quality_grade"].as_str().unwrap_or("");

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

        // Quality rater integration: low quality scores boost priority
        if quality_score > 0 && quality_score < 60 {
            score += 400; // F-grade content is urgent
        } else if quality_score > 0 && quality_score < 75 {
            score += 200; // D/C grade still worth reviewing
        }
        // Quality grade penalties for already-good content
        if quality_grade == "A" || quality_grade == "B" {
            score -= 300;
        }

        if pos >= 1.0 && pos <= 4.0 && ctr >= 0.05 {
            score -= 600;
        }

        let has_regression_signal =
            quick_ctr_opportunity || health == "poor" || checks_failed >= 3 || health_score <= 70 || (quality_score > 0 && quality_score < 60);

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

/// Build a deterministic content view of an article's MDX source for the
/// recommender: the full body (frontmatter stripped, capped at
/// `RECOMMEND_BODY_MAX_CHARS`), a heading outline, and an internal-link
/// inventory. The outline + link inventory are always included so the agent
/// keeps the article's structure even when the body is truncated.
pub(crate) fn build_article_content_view(source: &str) -> serde_json::Value {
    let body = crate::content::frontmatter::split_mdx(source)
        .map(|(_, b)| b)
        .unwrap_or(source);

    let body_truncated = body.chars().count() > RECOMMEND_BODY_MAX_CHARS;
    let source_body: String = body.chars().take(RECOMMEND_BODY_MAX_CHARS).collect();

    let heading_outline: Vec<serde_json::Value> = body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            // '#' is ASCII, so byte-indexing at `level` is safe.
            if (1..=4).contains(&level) && trimmed[level..].starts_with(' ') {
                Some(serde_json::json!({
                    "level": level,
                    "text": trimmed[level..].trim(),
                }))
            } else {
                None
            }
        })
        .collect();

    let internal_links: Vec<serde_json::Value> =
        crate::content::linking::extract_blog_link_hrefs(body)
            .into_iter()
            .map(|(anchor, href, _slug)| serde_json::json!({ "anchor": anchor, "href": href }))
            .collect();

    serde_json::json!({
        "source_body": source_body.trim(),
        "body_truncated": body_truncated,
        "heading_outline": heading_outline,
        "internal_links": internal_links,
    })
}

/// Build the per-article context object handed to the recommender.
///
/// Combines the article metadata + failed checks with the deterministic
/// content view and the article's top GSC queries (from `ctr_query_metrics`,
/// already ordered by impressions DESC).
pub(crate) fn build_article_review_context(
    article: &serde_json::Value,
    source: Option<&str>,
    top_queries: &[crate::db::CtrQueryMetricRow],
) -> serde_json::Value {
    let queries: Vec<serde_json::Value> = top_queries
        .iter()
        .take(TOP_QUERIES_PER_ARTICLE)
        .map(|q| {
            serde_json::json!({
                "query": q.query,
                "impressions": q.impressions,
                "clicks": q.clicks,
                "ctr": q.ctr,
                "avg_position": q.avg_position,
            })
        })
        .collect();

    let mut ctx = serde_json::json!({
        "article_id": article["id"],
        "article_title": article["title"],
        "article_file": article["file"].as_str().unwrap_or(""),
        "url_slug": article["url_slug"],
        "target_keyword": article["target_keyword"],
        "published_date": article["published_date"],
        "gsc_snapshot": article["gsc"],
        "failed_checks": article["_failed_checks"],
        "top_queries": queries,
    });

    if let Some(view) = source.map(build_article_content_view) {
        if let (Some(dst), Some(view_map)) = (ctx.as_object_mut(), view.as_object()) {
            for (k, v) in view_map {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
    ctx
}

/// Build a structured context payload for the LLM.
///
/// For each selected article, attaches the deterministic content view
/// (full body, heading outline, internal-link inventory) so the agent has
/// concrete content — not just check names. Query metrics are not attached
/// here; the per-article recommend step adds them from the DB.
pub(crate) fn build_review_context(
    selected: &[serde_json::Value],
    repo_root: &std::path::Path,
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
            Some(build_article_review_context(
                article,
                source.as_deref(),
                &[],
            ))
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
    let meta_min = crate::engine::exec::audit_health::META_MIN_LEN;
    let meta_max = crate::engine::exec::audit_health::META_MAX_LEN;
    let snippet_min = crate::engine::exec::audit_health::SNIPPET_MIN_WORDS;
    let snippet_max = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
    format!(
        r#"Analyze the following articles and generate specific, actionable SEO recommendations.

Input context (per article: metadata, failed deterministic checks, top_queries with real GSC query data, source_body with the full article body, heading_outline, and internal_links):
{context_json}

For each article, examine:
1. Title and H1 quality — keyword presence, clarity, length
2. Meta description — presence, length ({meta_min}-{meta_max} chars), keyword inclusion
3. Introduction — engagement, keyword placement, length ({snippet_min}-{snippet_max} words)
4. Content structure — H2 headings, readability
5. Internal links — quantity, relevance
6. EEAT signals — credibility, authoritativeness
7. Call-to-action — clarity and placement
8. Year freshness — compare any year mentioned in the title or H1 against the published_date. If they differ (e.g., title says "2025" but published_date is "2026-03-15"), suggest updating the title/H1 year to match the published date. NEVER suggest changing the published_date.

For each suggestion, use one of these categories: title, meta_description, intro, h1, internal_links, faq, eeat, cta.

Requirements:
- 4-8 actionable suggestions per article.
- Use only the provided context.
- Be specific: include the exact current text and proposed replacement."#,
        context_json = context_json,
        meta_min = meta_min,
        meta_max = meta_max,
        snippet_min = snippet_min,
        snippet_max = snippet_max,
    )
}

/// Build a prompt for a single article — used in per-article extraction.
pub(crate) fn build_single_article_prompt(article: &serde_json::Value) -> String {
    let article_json = serde_json::to_string_pretty(article).unwrap_or_default();
    let meta_min = crate::engine::exec::audit_health::META_MIN_LEN;
    let meta_max = crate::engine::exec::audit_health::META_MAX_LEN;
    let snippet_min = crate::engine::exec::audit_health::SNIPPET_MIN_WORDS;
    let snippet_max = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
    format!(
        r#"Analyze the following article and generate specific, actionable SEO recommendations.

Input context: the article's metadata, failed deterministic checks, top_queries (real GSC queries driving its impressions), source_body (full article body; when body_truncated is true, rely on heading_outline and internal_links for the rest of the structure), heading_outline, and internal_links.
{article_json}

Examine:
1. Title and H1 quality — keyword presence, clarity, length
2. Meta description — presence, length ({meta_min}-{meta_max} chars), keyword inclusion
3. Introduction — engagement, keyword placement, length ({snippet_min}-{snippet_max} words)
4. Content structure — H2 headings, readability
5. Internal links — quantity, relevance
6. EEAT signals — credibility, authoritativeness
7. Call-to-action — clarity and placement
8. Year freshness — compare any year mentioned in the title or H1 against the published_date. If they differ, suggest updating the title/H1 year to match the published date. NEVER suggest changing the published_date.

For each suggestion, use one of these categories: title, meta_description, intro, h1, internal_links, faq, eeat, cta.

Requirements:
- 4-8 actionable suggestions for THIS article only.
- Use only the provided context.
- Be specific: include the exact current text and proposed replacement."#,
        article_json = article_json,
        meta_min = meta_min,
        meta_max = meta_max,
        snippet_min = snippet_min,
        snippet_max = snippet_max,
    )
}

/// Step runner for `content_review_recommend` steps.
///
/// 1. Reads content_audit.json + articles.json
/// 2. Selects priority articles via `select_priority_articles`
/// 3. Builds structured context per article: full body (capped), heading
///    outline, internal-link inventory, and per-page top GSC queries
/// 4. Makes one rig `Extractor<T>` call per article for guaranteed structured JSON output
/// 5. Writes recommendations.json to the automation dir
pub(crate) async fn exec_content_review_recommend(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use std::path::Path;

    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Open the app database once — reused below for per-page query metrics and
    // project info.
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
                output: None,
            };
        }
    };

    // Load content audit via the shared snapshot loader (DB primary, JSON fallback).
    let audit_articles =
        crate::engine::exec::common::load_audit_snapshot(&task.project_id, &paths).articles;

    let mut raw_articles: Vec<serde_json::Value> =
        crate::engine::exec::common::load_project_articles(&paths).articles;

    // ── Filter out articles that Google cannot see (not indexed) ──────────────
    // Load gsc_collection.json to cross-reference indexing status.
    let gsc_collection_path = paths.automation_dir.join("gsc_collection.json");
    let mut non_indexed_slugs: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(gsc_raw) = std::fs::read_to_string(&gsc_collection_path) {
        if let Ok(gsc_doc) = serde_json::from_str::<serde_json::Value>(&gsc_raw) {
            if let Some(items) = gsc_doc["items"].as_array() {
                for item in items {
                    let reason = item["reason_code"].as_str().unwrap_or("");
                    if reason.starts_with("not_indexed") {
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

    raw_articles.retain(|article| {
        let slug = article["url_slug"].as_str().unwrap_or("");
        if non_indexed_slugs.contains(slug) {
            log::info!(
                "[content_review_recommend] skipping non-indexed article: {}",
                slug
            );
            return false;
        }
        true
    });

    let selected = select_priority_articles(&raw_articles, &audit_articles, 20);
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

    // Build individual contexts — one per article: full body (capped), heading
    // outline, internal-link inventory, and per-page top GSC queries from
    // `ctr_query_metrics` (read from the DB — no GSC API calls here).
    let contexts: Vec<serde_json::Value> = selected
        .iter()
        .filter_map(|article| {
            let file_ref = article["file"].as_str().unwrap_or("");
            if file_ref.is_empty() {
                return None;
            }
            let source = crate::engine::exec::utils::read_source_file(repo_root, file_ref);
            let article_id = article["id"].as_i64().unwrap_or(0);
            let top_queries = crate::db::get_ctr_query_metrics(&db, &task.project_id, article_id)
                .unwrap_or_else(|e| {
                    log::warn!(
                        "[content_review_recommend] query metrics unavailable for article {}: {}",
                        article_id,
                        e
                    );
                    Vec::new()
                });
            Some(build_article_review_context(
                article,
                source.as_deref(),
                &top_queries,
            ))
        })
        .collect();

    if contexts.is_empty() {
        return crate::engine::workflows::StepResult::fail("Could not read source files for selected articles — check file paths in articles.json".to_string());
    }

    // Process articles with bounded concurrency (limit 2 to match provider limits).
    // No per-project voice/tone setting exists, so ground the preamble in the
    // project's name + site URL instead of a bare one-liner.
    let site_descriptor = crate::engine::task_store::get_project(&db, &task.project_id)
        .ok()
        .map(|p| match p.site_url.as_deref().map(str::trim) {
            Some(url) if !url.is_empty() => {
                format!("{} ({})", p.name, crate::models::project::site_base_url(url))
            }
            _ => p.name,
        });
    let preamble_owned;
    let preamble: &str = match site_descriptor.as_deref() {
        Some(site) => {
            preamble_owned = format!(
                "You are an expert SEO content reviewer for {site}. Analyze the single article below — you are given its full body (possibly truncated), heading outline, internal-link inventory, and the real search queries driving its impressions. Ground every suggestion in this evidence, not in the names of failed checks. Generate structured recommendations using the submit tool."
            );
            &preamble_owned
        }
        None => "You are an expert SEO content reviewer. Analyze the single article below — including its full body, heading outline, internal-link inventory, and real search queries — and generate structured recommendations using the submit tool.",
    };
    let mut all_articles: Vec<crate::models::content_review::ReviewArticleRecommendation> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    let results: Vec<(serde_json::Value, Result<crate::models::content_review::SingleArticleRecommendations, String>)> = {
        use futures::StreamExt;
        futures::stream::iter(contexts.iter().cloned())
            .map(|ctx| async {
                let article_id = ctx["article_id"].as_i64().unwrap_or(0);
                let prompt = build_single_article_prompt(&ctx);
                log::info!(
                    "[content_review_recommend] extracting article {} ({} chars prompt)",
                    article_id,
                    prompt.len()
                );
                let result = crate::rig::extraction::extract_structured::<
                    crate::models::content_review::SingleArticleRecommendations,
                >(agent_provider, &prompt, Some(preamble), Some("direct"), None)
                .await;
                (ctx, result)
            })
            .buffer_unordered(2)
            .collect()
            .await
    };

    for (ctx, result) in results {
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

    let rec = crate::models::content_review::ContentReviewRecommendations {
        generated_at: chrono::Utc::now().to_rfc3339(),
        total_articles: all_articles.len(),
        articles: all_articles,
    };

    let rec_value = match serde_json::to_value(&rec) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Failed to serialize recommendations: {}", e))
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


#[cfg(test)]
mod tests {
    use super::*;

    fn sample_article() -> serde_json::Value {
        serde_json::json!({
            "id": 42,
            "title": "Best Seed Trays 2026",
            "file": "content/blog/001_best-seed-trays.mdx",
            "url_slug": "best-seed-trays",
            "target_keyword": "seed trays",
            "published_date": "2026-01-10",
            "gsc": { "avg_position": 8.2, "impressions": 540.0, "ctr": 0.012 },
            "_failed_checks": [
                { "check_id": "meta_desc_length", "label": "Meta description length 120–155 chars" }
            ],
        })
    }

    fn sample_mdx() -> String {
        String::from(
            "---\ntitle: \"Best Seed Trays 2026\"\ndescription: \"meta here\"\n---\n\n# Best Seed Trays 2026\n\nOpening paragraph about seed trays.\n\n## What to Look For\n\nBody text with a [related guide](/blog/seed-starting-guide) link.\n\n### Materials\n\nMore text.\n\n## FAQ\n\nQuestions.\n",
        )
    }

    fn sample_query_row(query: &str) -> crate::db::CtrQueryMetricRow {
        crate::db::CtrQueryMetricRow {
            project_id: "proj".to_string(),
            article_id: 42,
            page_url: "https://example.com/blog/best-seed-trays".to_string(),
            query: query.to_string(),
            impressions: 320.0,
            clicks: 4.0,
            ctr: 0.0125,
            avg_position: 7.4,
            period_start: None,
            period_end: None,
            intent: None,
            fetched_at: "2026-07-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn content_view_includes_body_outline_and_links() {
        let view = build_article_content_view(&sample_mdx());
        let body = view["source_body"].as_str().unwrap();
        // Frontmatter is stripped; body and headings are present.
        assert!(!body.contains("description:"));
        assert!(body.contains("Opening paragraph about seed trays."));
        assert!(body.contains("## FAQ"));
        assert_eq!(view["body_truncated"], serde_json::json!(false));

        let outline = view["heading_outline"].as_array().unwrap();
        let texts: Vec<&str> = outline.iter().map(|h| h["text"].as_str().unwrap()).collect();
        assert_eq!(
            texts,
            vec!["Best Seed Trays 2026", "What to Look For", "Materials", "FAQ"]
        );
        assert_eq!(outline[1]["level"], serde_json::json!(2));

        let links = view["internal_links"].as_array().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0]["href"], serde_json::json!("/blog/seed-starting-guide"));
        assert_eq!(links[0]["anchor"], serde_json::json!("related guide"));
    }

    #[test]
    fn content_view_truncates_long_body_but_keeps_structure() {
        let long_body = format!(
            "---\ntitle: \"x\"\n---\n\n# Title\n\n{}\n\n## Final Section\n",
            "word ".repeat(10_000)
        );
        let view = build_article_content_view(&long_body);
        assert_eq!(view["body_truncated"], serde_json::json!(true));
        assert!(
            view["source_body"].as_str().unwrap().chars().count() <= RECOMMEND_BODY_MAX_CHARS,
            "body must be capped at RECOMMEND_BODY_MAX_CHARS"
        );
        // Heading outline still covers content beyond the truncation point.
        let outline = view["heading_outline"].as_array().unwrap();
        assert!(outline.iter().any(|h| h["text"] == "Final Section"));
    }

    #[test]
    fn review_context_attaches_top_queries() {
        let rows = vec![sample_query_row("best seed trays"), sample_query_row("seed tray sizes")];
        let ctx = build_article_review_context(&sample_article(), Some(&sample_mdx()), &rows);

        let queries = ctx["top_queries"].as_array().unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0]["query"], serde_json::json!("best seed trays"));
        assert!(queries[0]["impressions"].as_f64().unwrap() > 0.0);
        // Content view fields merged in.
        assert!(ctx["source_body"].as_str().unwrap().contains("Opening paragraph"));
        assert!(ctx["heading_outline"].is_array());
        assert!(ctx["internal_links"].is_array());
    }

    #[test]
    fn review_context_caps_queries_at_ten() {
        let rows: Vec<_> = (0..15)
            .map(|i| sample_query_row(&format!("query {}", i)))
            .collect();
        let ctx = build_article_review_context(&sample_article(), None, &rows);
        assert_eq!(ctx["top_queries"].as_array().unwrap().len(), TOP_QUERIES_PER_ARTICLE);
        // No source → no content view keys.
        assert!(ctx.get("source_body").is_none());
    }

    #[test]
    fn single_article_prompt_contains_top_queries_and_unified_ranges() {
        let rows = vec![sample_query_row("best seed trays for beginners")];
        let ctx = build_article_review_context(&sample_article(), Some(&sample_mdx()), &rows);
        let prompt = build_single_article_prompt(&ctx);

        assert!(prompt.contains("best seed trays for beginners"));
        assert!(prompt.contains("120-155"), "meta range missing: {}", prompt);
        assert!(prompt.contains("40-60"), "intro range missing: {}", prompt);
        assert!(!prompt.contains("50-155"));
        assert!(!prompt.contains("40-80"));
    }

    #[test]
    fn batch_prompt_uses_unified_ranges() {
        let ctx = serde_json::json!({ "generated_at": "now", "articles": [] });
        let prompt = build_review_prompt(&ctx);
        assert!(prompt.contains("120-155"));
        assert!(prompt.contains("40-60"));
        assert!(!prompt.contains("50-155"));
    }
}
