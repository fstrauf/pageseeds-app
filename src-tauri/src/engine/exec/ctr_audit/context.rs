use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Position-aware target CTR curve.
///
/// | Position Range | Target CTR |
/// |----------------|------------|
/// | 1-2            | 8.0%       |
/// | 3-4            | 4.0%       |
/// | 5-7            | 1.5%       |
/// | 8-10           | 0.8%       |
/// | 11-20          | 0.3%       |
pub fn target_ctr_for_position(position: f64) -> f64 {
    match position {
        p if p <= 0.0 => 0.0,
        p if p <= 2.0 => 0.08,
        p if p <= 4.0 => 0.04,
        p if p <= 7.0 => 0.015,
        p if p <= 10.0 => 0.008,
        p if p <= 20.0 => 0.003,
        _ => 0.0,
    }
}

/// Classify query intent based on deterministic keyword patterns.
pub fn classify_query_intent(query: &str) -> &'static str {
    let lower = query.to_lowercase();

    if lower.starts_with("who ")
        || lower.starts_with("what ")
        || lower.starts_with("where ")
        || lower.starts_with("when ")
        || lower.starts_with("why ")
        || lower.starts_with("how ")
        || lower.starts_with("is ")
        || lower.starts_with("are ")
        || lower.starts_with("does ")
        || lower.starts_with("do ")
        || lower.starts_with("can ")
        || lower.starts_with("will ")
        || lower.starts_with("should ")
        || lower.ends_with('?')
    {
        return "question";
    }

    if lower.contains(" vs ")
        || lower.contains(" versus ")
        || lower.contains("compare")
        || lower.contains("difference between")
        || lower.contains(" or ")
    {
        return "comparison";
    }

    if lower.contains("best ")
        || lower.contains("top ")
        || lower.contains("worst ")
        || lower.contains("cheapest ")
        || lower.contains("highest ")
        || lower.contains("lowest ")
        || lower.contains("most ")
        || lower.contains("least ")
    {
        return "best_list";
    }

    if lower.contains("tax")
        || lower.contains("legal")
        || lower.contains("law")
        || lower.contains("regulation")
        || lower.contains("compliance")
    {
        return "tax_legal";
    }

    if lower.contains("calculator")
        || lower.contains("tool")
        || lower.contains("generator")
        || lower.contains("template")
    {
        return "calculator_tool";
    }

    "generic"
}

/// Build the CTR audit context by reading articles.json, extracting excerpts,
/// computing clicks_lost per article, and returning structured JSON.
///
/// Uses persistent `article_audit_state` to skip articles that were healthy on the
/// last audit AND have not changed since. This prevents re-flagging already-fixed
/// issues across repeated audit runs.
pub(crate) fn exec_ctr_build_context(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
    conn: &rusqlite::Connection,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let articles_path = paths.automation_dir.join("articles.json");

    // ── Step 0: Clean stale entries from articles.json ───────────────────────
    // The filesystem is the source of truth. Remove entries whose files no longer exist.
    let mut cleaned_summary = Vec::new();
    match crate::content::ops::clean_stale_articles_json(&paths.automation_dir, std::path::Path::new(project_path)) {
        Ok(removed) => {
            if !removed.is_empty() {
                log::info!(
                    "[ctr_audit] Removed {} stale entries from articles.json: {:?}",
                    removed.len(),
                    removed
                );
                cleaned_summary = removed;
            }
        }
        Err(e) => {
            log::warn!("[ctr_audit] Failed to clean stale articles.json entries: {}", e);
        }
    }

    let doc: serde_json::Value = match crate::engine::exec::common::read_json(&articles_path, "articles.json") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let empty = vec![];
    let articles = doc["articles"].as_array().unwrap_or(&empty);

    let mut article_records: Vec<serde_json::Value> = Vec::new();
    let mut skipped_healthy = 0usize;
    let mut skipped_unchanged = 0usize;

    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
        let target_keyword = article["target_keyword"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();

        let gsc = &article["gsc"];
        let impressions = gsc["impressions"].as_f64().unwrap_or(0.0);
        let clicks = gsc["clicks"].as_f64().unwrap_or(0.0);
        let ctr = gsc["ctr"].as_f64().unwrap_or(0.0);
        let avg_position = gsc["avg_position"].as_f64().unwrap_or(0.0);

        // Extract current MDX state
        let (current_title, meta_description, first_paragraph, h1, has_faq_schema, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(project_path, &file_ref);

        // Compute content hash for change detection (includes FAQ/schema state)
        let content_hash = crate::engine::exec::audit_health::compute_content_hash(
            &current_title,
            &meta_description,
            &first_paragraph,
            has_faq_schema,
        );

        // Check stored audit state: if hash matches and was healthy, skip
        if let Ok(Some(stored)) = crate::db::get_article_audit_state(
            conn,
            &task.project_id,
            &file_ref,
            "ctr_audit",
        ) {
            if stored.content_hash == content_hash && stored.was_healthy {
                skipped_unchanged += 1;
                continue;
            }
        }

        // Run deterministic health checks
        let health = crate::engine::exec::audit_health::check_article_health(
            &current_title,
            &meta_description,
            &first_paragraph,
            &target_keyword,
            has_faq_schema,
            file_found,
        );

        // Persist the audit state immediately (healthy or not)
        let _ = crate::db::set_article_audit_state(
            conn,
            &task.project_id,
            &file_ref,
            "ctr_audit",
            health.all_ok(),
            &content_hash,
            &health.issues,
        );

        if health.all_ok() {
            skipped_healthy += 1;
            continue;
        }

        // Compute clicks_lost using position-aware target CTR
        let target_ctr = target_ctr_for_position(avg_position);
        let clicks_lost = impressions * (target_ctr - ctr).max(0.0);

        article_records.push(serde_json::json!({
            "id": id,
            "url_slug": url_slug,
            "title": current_title,
            "target_keyword": target_keyword,
            "meta_description": meta_description,
            "first_paragraph": first_paragraph,
            "h1": h1,
            "file": file_ref,
            "gsc": {
                "impressions": impressions,
                "clicks": clicks,
                "ctr": ctr,
                "avg_position": avg_position,
            },
            "clicks_lost": clicks_lost,
            "target_ctr": target_ctr,
            "issues_detected": {
                "file_not_found": !health.file_found,
                "title_too_long": !health.title_ok,
                "meta_too_short": !health.meta_ok,
                "snippet_suboptimal": !health.snippet_ok,
                "missing_faq_schema": !health.faq_ok,
            },
            "top_queries": serde_json::Value::Null,
        }));
    }

    if skipped_healthy > 0 || skipped_unchanged > 0 {
        log::info!(
            "[ctr_audit] Skipped {} healthy + {} unchanged articles",
            skipped_healthy,
            skipped_unchanged
        );
    }

    // ── Optional: fetch query-level GSC data for top candidates ────────────────
    let query_enriched = if !article_records.is_empty() {
        enrich_with_query_metrics(
            &task.project_id,
            project_path,
            gsc_token,
            conn,
            &mut article_records,
        )
    } else {
        0
    };

    // Sort by clicks_lost descending
    article_records.sort_by(|a, b| {
        let ca = a["clicks_lost"].as_f64().unwrap_or(0.0);
        let cb = b["clicks_lost"].as_f64().unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_20: Vec<&serde_json::Value> = article_records.iter().take(20).collect();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let full_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": article_records.len(),
        "articles": article_records,
        "top_20_by_clicks_lost": top_20,
    });

    // Write full context to automation dir for reference
    let out_path = paths.automation_dir.join("ctr_audit_context.json");
    let full_str = serde_json::to_string_pretty(&full_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &full_str) {
        log::warn!("[ctr_audit] Failed to write ctr_audit_context.json: {}", e);
    }

    // Return only the top 20 as step output to keep the agentic prompt small
    let summary_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": article_records.len(),
        "top_20_by_clicks_lost": top_20,
        "cleaned_stale_entries": cleaned_summary.len(),
        "cleaned_files": cleaned_summary,
    });
    let summary_str = serde_json::to_string_pretty(&summary_doc).unwrap_or_default() + "\n";

    let clean_msg = if cleaned_summary.is_empty() {
        String::new()
    } else {
        format!(
            " — removed {} stale entries from articles.json",
            cleaned_summary.len()
        )
    };

    let mut msg = format!(
        "CTR context built for {} articles ({} healthy, {} unchanged){}",
        article_records.len(),
        skipped_healthy,
        skipped_unchanged,
        clean_msg
    );
    if query_enriched > 0 {
        msg.push_str(&format!(" — query data for {} articles", query_enriched));
    }

    StepResult {
        success: true,
        message: msg,
        output: Some(summary_str),
    }
}

/// Fetch top GSC queries for the top CTR candidates and attach them to article records.
///
/// Returns the number of articles successfully enriched with query data.
fn enrich_with_query_metrics(
    project_id: &str,
    project_path: &str,
    gsc_token: Option<&str>,
    conn: &rusqlite::Connection,
    article_records: &mut [serde_json::Value],
) -> usize {
    use crate::engine::project_paths::ProjectPaths;

    let paths = ProjectPaths::from_path(project_path);

    // 1. Resolve site_url from manifest.json
    let manifest_path = paths.automation_dir.join("manifest.json");
    let site_url: String = match std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("gsc_site").or_else(|| v.get("url")).and_then(|u| u.as_str()).map(String::from))
    {
        Some(u) => u,
        None => {
            log::info!("[ctr_audit] No site_url in manifest.json — skipping query fetch");
            return 0;
        }
    };

    // 2. Resolve GSC token
    let token = match resolve_gsc_token_for_queries(project_path, gsc_token) {
        Some(t) => t,
        None => {
            log::info!("[ctr_audit] No GSC token available — skipping query fetch");
            return 0;
        }
    };

    let base_url = if site_url.starts_with("sc-domain:") {
        format!("https://{}/", &site_url["sc-domain:".len()..])
    } else if !site_url.ends_with('/') {
        format!("{}/", site_url)
    } else {
        site_url
    };

    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let start = end - chrono::Duration::days(89); // 90-day window
    let start_str = start.format("%Y-%m-%d").to_string();
    let end_str = end.format("%Y-%m-%d").to_string();

    let mut enriched = 0usize;
    let max_articles = 10; // Limit API calls — only top 10 candidates
    let max_queries_per_page = 10;

    for record in article_records.iter_mut().take(max_articles) {
        let article_id = record["id"].as_i64().unwrap_or(0);
        let url_slug = record["url_slug"].as_str().unwrap_or("");
        if url_slug.is_empty() {
            continue;
        }

        let page_url = format!("{}{}", base_url, url_slug);
        let page_url_for_thread = page_url.clone();
        let token_clone = token.clone();
        let site_url_clone = base_url.clone();
        let start_clone = start_str.clone();
        let end_clone = end_str.clone();

        let query_rows = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
                crate::gsc::analytics::fetch_queries_for_page(
                    &token_clone,
                    &site_url_clone.trim_end_matches('/'),
                    &page_url_for_thread,
                    &start_clone,
                    &end_clone,
                    max_queries_per_page,
                ).await
            })
        }).join();

        let metrics = match query_rows {
            Ok(Ok(rows)) => rows,
            Ok(Err(e)) => {
                log::warn!("[ctr_audit] Failed to fetch queries for {}: {}", page_url, e);
                continue;
            }
            Err(_) => {
                log::warn!("[ctr_audit] Query fetch thread panicked for {}", page_url);
                continue;
            }
        };

        if metrics.is_empty() {
            continue;
        }

        // Build query records with intent classification
        let query_records: Vec<serde_json::Value> = metrics
            .iter()
            .map(|m| {
                let intent = classify_query_intent(&m.query);
                serde_json::json!({
                    "query": m.query,
                    "impressions": m.impressions,
                    "clicks": m.clicks,
                    "ctr": m.ctr,
                    "avg_position": m.position,
                    "intent": intent,
                })
            })
            .collect();

        // Store in DB
        let db_metrics: Vec<(String, f64, f64, f64, f64, Option<String>)> = metrics
            .iter()
            .map(|m| {
                let intent = classify_query_intent(&m.query);
                (m.query.clone(), m.impressions, m.clicks, m.ctr, m.position, Some(intent.to_string()))
            })
            .collect();

        // TODO: store query metrics in DB once the schema function is added.
        let _ = (conn, project_id, article_id, &page_url, &db_metrics, &start_str, &end_str);
        log::warn!("[ctr_audit] DB storage for query metrics not yet implemented");

        record["top_queries"] = serde_json::json!(query_records);
        enriched += 1;
    }

    enriched
}

/// Resolve a GSC token for query fetching.
/// Uses the provided token if available, otherwise falls back to service account auth.
fn resolve_gsc_token_for_queries(project_path: &str, gsc_token: Option<&str>) -> Option<String> {
    if let Some(t) = gsc_token {
        return Some(t.to_string());
    }

    let resolver = crate::config::env_resolver::EnvResolver::new(project_path);
    let sa_path = resolver
        .resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)?;

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().ok()?;
        rt.block_on(async move {
            crate::gsc::auth::get_service_account_token(&sa_path)
                .await
                .map(|t| t.access_token)
                .ok()
        })
    })
    .join()
    .ok()
    .flatten()
}
