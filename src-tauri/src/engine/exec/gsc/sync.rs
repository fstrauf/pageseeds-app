use regex::Regex;
use std::collections::HashMap;

use crate::config::env_resolver::EnvResolver;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Native Rust replacement for `pageseeds automation seo gsc-sync-articles`.
///
/// Fetches page-level GSC metrics (90-day window) and writes a `gsc` block into
/// each matching article in automation/articles.json. Matching uses normalised
/// URL paths (scheme-stripped, trailing-slash removed, underscore→dash, lowercase)
/// with a secondary last-segment index.
pub(crate) fn exec_gsc_sync_articles(
    task: &Task,
    project_path: &str,
    _gsc_token: Option<&str>,
) -> StepResult {
    let _ = task;

    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. Credentials
    let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        Some(p) => p,
        None => return StepResult::fail("GSC_SERVICE_ACCOUNT_PATH not configured — add it to ~/.config/automation/secrets.env".to_string()),
    };

    // 2. Token - Always mint fresh from service account when available
    let sa_path_owned = sa_path.clone();
    let token_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            crate::gsc::auth::get_service_account_token(&sa_path_owned)
                .await
                .map(|t| t.access_token)
        })
    })
    .join();

    let token = match token_result {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            return StepResult::fail(format!("GSC auth failed: {}", e))
        }
        Err(_) => {
            return StepResult::fail("GSC auth thread panicked".to_string())
        }
    };

    // 3. Load articles from SQLite (canonical runtime store)
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return StepResult::fail(format!("Failed to open app database: {}", e));
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult::fail(format!("Failed to load articles from DB: {}", e));
        }
    };

    // 4. site_url from manifest.json
    let site_url: String = {
        let manifest_path = paths.automation_dir.join("manifest.json");
        let from_manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("gsc_site")
                    .or_else(|| v.get("url"))
                    .and_then(|u| u.as_str())
                    .map(String::from)
            });
        match from_manifest {
            Some(u) => u,
            None => {
                return StepResult::fail("No site_url found in manifest.json — add 'url' or 'gsc_site' field"
                        .to_string())
            }
        }
    };

    // `site_url` may be a GSC property ID (sc-domain:…) — convert for fetching.
    let base_url = crate::models::project::site_base_url(&site_url);
    let _ = &base_url;

    // 5. Fetch GSC metrics (90-day window) - Spawn thread with own runtime
    let days = 90i64;
    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let start = end - chrono::Duration::days(days - 1);
    let token_clone = token.clone();
    let site_url_clone = site_url.clone();
    let start_str = start.format("%Y-%m-%d").to_string();
    let end_str = end.format("%Y-%m-%d").to_string();

    let (page_rows, query_rows) = {
        let token_inner = token_clone;
        let site_url_inner = site_url_clone;
        let start_inner = start_str.clone();
        let end_inner = end_str.clone();

        let fetch_result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
                let pages = crate::gsc::analytics::fetch_page_rows(
                    &token_inner,
                    &site_url_inner,
                    &start_inner,
                    &end_inner,
                    1000,
                )
                .await?;
                let queries = crate::gsc::analytics::fetch_page_query_rows(
                    &token_inner,
                    &site_url_inner,
                    &start_inner,
                    &end_inner,
                    10000,
                )
                .await?;
                Ok::<_, crate::error::Error>((pages, queries))
            })
        })
        .join();

        match fetch_result {
            Ok(Ok((p, q))) => (p, q),
            Ok(Err(e)) => {
                return StepResult::fail(format!("GSC fetch failed: {}", e))
            }
            Err(_) => {
                return StepResult::fail("GSC fetch thread panicked".to_string())
            }
        }
    };

    // 6. Build normalised path lookup
    let num_prefix_re = Regex::new(r"^\d+[_\-]+").unwrap();

    let normalize_path = |url: &str| -> String {
        let stripped = if let Some(rest) = url.strip_prefix("https://") {
            rest
        } else if let Some(rest) = url.strip_prefix("http://") {
            rest
        } else {
            url
        };
        let path = if let Some(slash) = stripped.find('/') {
            &stripped[slash..]
        } else {
            "/"
        };
        path.trim_end_matches('/').replace('_', "-").to_lowercase()
    };

    let mut gsc_by_path: HashMap<String, &crate::models::gsc::PageMetrics> = HashMap::new();
    for row in &page_rows {
        let p = normalize_path(&row.page);
        if !p.is_empty() {
            gsc_by_path.entry(p).or_insert(row);
        }
    }
    let mut gsc_by_segment: HashMap<String, &crate::models::gsc::PageMetrics> = HashMap::new();
    for (path, m) in &gsc_by_path {
        let last = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        if !last.is_empty() {
            gsc_by_segment.entry(last.clone()).or_insert(m);
            let stripped = num_prefix_re.replace(&last, "").to_string();
            if stripped != last && !stripped.is_empty() {
                gsc_by_segment.entry(stripped).or_insert(m);
            }
        }
    }

    // 7. Match articles and store GSC metadata in SQLite sidecar table
    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut matched = 0usize;
    let mut unmatched = 0usize;

    for article in &articles {
        let slug = article.url_slug.clone();
        let file_ref = article.file.clone();

        let article_path: String = if !slug.is_empty() {
            let s = slug.trim_matches('/').replace('_', "-").to_lowercase();
            format!("/{}", s)
        } else if !file_ref.is_empty() {
            let stem = std::path::Path::new(&file_ref)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let s = num_prefix_re.replace(&stem, "").to_string();
            format!("/{}", s.replace('_', "-").to_lowercase())
        } else {
            unmatched += 1;
            continue;
        };

        let metrics = gsc_by_path
            .get(&article_path)
            .or_else(|| gsc_by_segment.get(article_path.trim_start_matches('/')));

        if let Some(m) = metrics {
            let payload = serde_json::json!({
                "impressions": m.impressions,
                "clicks": m.clicks,
                "ctr": (m.ctr * 10000.0).round() / 10000.0,
                "avg_position": (m.position * 10.0).round() / 10.0,
                "last_synced": now_iso,
                "period_days": days,
            });
            let _ = crate::content::article_index::set_metadata(
                &db,
                &task.project_id,
                article.id,
                "gsc",
                &payload.to_string(),
            );
            matched += 1;
        } else {
            unmatched += 1;
        }
    }

    // 7b. Append per-page daily snapshots (issue #23). Best-effort: the
    // snapshot pull is additive — a failure here must not fail the sync.
    // Append-only by contract (INSERT OR IGNORE); never deleted on re-sync.
    let snapshots_written = {
        let token_daily = token.clone();
        let site_daily = site_url.clone();
        let start_daily = start_str.clone();
        let end_daily = end_str.clone();
        let daily_result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
                crate::gsc::analytics::fetch_page_daily_rows(
                    &token_daily,
                    &site_daily,
                    &start_daily,
                    &end_daily,
                    25000,
                )
                .await
            })
        })
        .join();

        match daily_result {
            Ok(Ok(rows)) => {
                match crate::db::insert_gsc_page_daily_snapshots(&db, &task.project_id, &rows) {
                    Ok(n) => n,
                    Err(e) => {
                        log::warn!("[gsc_sync] Failed to write daily snapshots: {}", e);
                        0
                    }
                }
            }
            Ok(Err(e)) => {
                log::warn!("[gsc_sync] Daily snapshot fetch failed (non-fatal): {}", e);
                0
            }
            Err(_) => {
                log::warn!("[gsc_sync] Daily snapshot fetch thread panicked (non-fatal)");
                0
            }
        }
    };

    // 8. Match query-level data and store in ctr_query_metrics + target_keyword
    let mut queries_by_article: HashMap<i64, Vec<&crate::models::gsc::PageQueryMetrics>> =
        HashMap::new();
    let mut path_to_article_id: HashMap<String, i64> = HashMap::new();

    for article in &articles {
        let slug = article.url_slug.clone();
        let file_ref = article.file.clone();
        let article_path: String = if !slug.is_empty() {
            let s = slug.trim_matches('/').replace('_', "-").to_lowercase();
            format!("/{}", s)
        } else if !file_ref.is_empty() {
            let stem = std::path::Path::new(&file_ref)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let s = num_prefix_re.replace(&stem, "").to_string();
            format!("/{}", s.replace('_', "-").to_lowercase())
        } else {
            continue;
        };

        // Insert full path and segment variants for query matching
        path_to_article_id.insert(article_path.clone(), article.id);
        let last = article_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        if !last.is_empty() {
            path_to_article_id.insert(last.clone(), article.id);
            let stripped = num_prefix_re.replace(&last, "").to_string();
            if stripped != last && !stripped.is_empty() {
                path_to_article_id.insert(stripped, article.id);
            }
        }
    }

    for q in &query_rows {
        let normalized_page = normalize_path(&q.page);
        let article_id = path_to_article_id
            .get(&normalized_page)
            .copied()
            .or_else(|| {
                let last = normalized_page
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                path_to_article_id.get(&last).copied().or_else(|| {
                    let stripped = num_prefix_re.replace(&last, "").to_string();
                    path_to_article_id.get(&stripped).copied()
                })
            });

        if let Some(id) = article_id {
            queries_by_article.entry(id).or_default().push(q);
        }
    }

    let mut query_matched = 0usize;
    let mut target_keyword_updated = 0usize;

    for (article_id, queries) in &mut queries_by_article {
        // Sort by clicks descending so top query is first
        queries.sort_by(|a, b| {
            b.clicks
                .partial_cmp(&a.clicks)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let page_url = queries.first().map(|q| q.page.as_str()).unwrap_or("");
        let db_metrics: Vec<(String, f64, f64, f64, f64, Option<String>)> = queries
            .iter()
            .map(|q| {
                (
                    q.query.clone(),
                    q.impressions,
                    q.clicks,
                    q.ctr,
                    q.position,
                    None, // intent — not classified here
                )
            })
            .collect();

        if let Err(e) = crate::db::set_ctr_query_metrics(
            &db,
            &task.project_id,
            *article_id,
            page_url,
            &db_metrics,
            Some(&start_str),
            Some(&end_str),
        ) {
            log::warn!(
                "[gsc_sync] Failed to store query metrics for article {}: {}",
                article_id,
                e
            );
        } else {
            query_matched += 1;
        }

        // Update target_keyword from top query if currently empty
        if let Ok(top_query) = queries.first().map(|q| q.query.as_str()).ok_or("") {
            if !top_query.is_empty() {
                let updated = db.execute(
                    "UPDATE articles SET target_keyword = ?1
                     WHERE project_id = ?2 AND id = ?3
                       AND (target_keyword IS NULL OR target_keyword = '')",
                    rusqlite::params![top_query, task.project_id, article_id],
                );
                if let Ok(rows) = updated {
                    if rows > 0 {
                        target_keyword_updated += 1;
                    }
                } else if let Err(e) = updated {
                    log::warn!(
                        "[gsc_sync] Failed to update target_keyword for article {}: {}",
                        article_id,
                        e
                    );
                }
            }
        }
    }

    log::info!(
        "[gsc_sync] Query metrics stored for {} articles, target_keyword updated for {} articles",
        query_matched,
        target_keyword_updated
    );

    // 9. Re-export projection so articles.json gets the new GSC metadata
    if let Err(e) = crate::content::article_index::export_projection(
        &db,
        &task.project_id,
        std::path::Path::new(project_path),
    ) {
        return StepResult::fail(format!("GSC sync succeeded but projection export failed: {}", e));
    }

    // 10. Write the freshness marker consumed by the indexing-health
    // prerequisite gate (issue #25). Failing here fails the step on purpose:
    // without the marker the gate fails closed and would keep re-spawning
    // collect_gsc forever.
    if let Err(e) = write_metrics_sync_marker(&paths) {
        return StepResult::fail(format!(
                "GSC sync succeeded but failed to write the metrics freshness marker ({}): {}",
                paths.automation_dir.join(super::GSC_METRICS_SYNC_MARKER).display(),
                e
            ));
    }

    let summary = serde_json::json!({
        "matched": matched,
        "unmatched": unmatched,
        "total": matched + unmatched,
        "gsc_rows": page_rows.len(),
        "query_rows": query_rows.len(),
        "query_articles": query_matched,
        "target_keywords_updated": target_keyword_updated,
        "daily_snapshots_written": snapshots_written,
        "site": site_url,
        "period_days": days,
    });

    StepResult {
        success: true,
        message: format!(
            "GSC sync: matched {}/{} articles ({} GSC pages, {} query rows, {} articles with query data, {} target keywords updated)",
            matched,
            matched + unmatched,
            page_rows.len(),
            query_rows.len(),
            query_matched,
            target_keyword_updated
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

/// Write the `gsc_metrics_synced_at` marker (RFC3339 timestamp) into the
/// automation dir. Kept as a standalone sidecar — NOT a field inside
/// `gsc_collection.json` — because this sync also runs standalone and must not
/// know about the collection file (issue #25).
pub(crate) fn write_metrics_sync_marker(paths: &ProjectPaths) -> std::io::Result<()> {
    std::fs::create_dir_all(&paths.automation_dir)?;
    let marker_path = paths.automation_dir.join(super::GSC_METRICS_SYNC_MARKER);
    std::fs::write(marker_path, chrono::Utc::now().to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    #[test]
    fn write_metrics_sync_marker_creates_rfc3339_file() {
        let dir = unique_temp_dir("gsc_sync_marker");
        let paths = ProjectPaths::from_path(dir.to_str().unwrap());

        write_metrics_sync_marker(&paths).unwrap();

        let marker_path = paths
            .automation_dir
            .join(crate::engine::exec::gsc::GSC_METRICS_SYNC_MARKER);
        let content = std::fs::read_to_string(&marker_path).unwrap();
        chrono::DateTime::parse_from_rfc3339(content.trim())
            .expect("marker content must be a valid RFC3339 timestamp");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
