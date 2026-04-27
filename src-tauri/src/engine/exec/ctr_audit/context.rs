use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Build the CTR audit context by reading articles.json, extracting excerpts,
/// computing clicks_lost per article, and returning structured JSON.
///
/// Uses persistent `article_audit_state` to skip articles that were healthy on the
/// last audit AND have not changed since. This prevents re-flagging already-fixed
/// issues across repeated audit runs.
pub(crate) fn exec_ctr_build_context(
    task: &Task,
    project_path: &str,
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

        // Compute clicks_lost: impressions * max(0, 0.005 - actual_ctr)
        let clicks_lost = impressions * (0.005_f64 - ctr).max(0.0);

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
            "issues_detected": {
                "file_not_found": !health.file_found,
                "title_too_long": !health.title_ok,
                "meta_too_short": !health.meta_ok,
                "snippet_suboptimal": !health.snippet_ok,
                "missing_faq_schema": !health.faq_ok,
            },
        }));
    }

    if skipped_healthy > 0 || skipped_unchanged > 0 {
        log::info!(
            "[ctr_audit] Skipped {} healthy + {} unchanged articles",
            skipped_healthy,
            skipped_unchanged
        );
    }

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

    StepResult {
        success: true,
        message: format!(
            "CTR context built for {} articles ({} healthy, {} unchanged){}",
            article_records.len(),
            skipped_healthy,
            skipped_unchanged,
            clean_msg
        ),
        output: Some(summary_str),
    }
}
