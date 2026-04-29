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
    gsc_token: Option<&str>,
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
        None => return StepResult {
            success: false,
            message: "GSC_SERVICE_ACCOUNT_PATH not configured — add it to ~/.config/automation/secrets.env".to_string(),
            output: None,
        },
    };

    // 2. Token - Spawn thread with own runtime to avoid block_on issues
    let gsc_token_owned = gsc_token.map(|t| t.to_string());
    let sa_path_owned = sa_path.clone();
    let token_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            if let Some(token) = gsc_token_owned {
                Ok::<_, crate::error::Error>(token)
            } else {
                crate::gsc::auth::get_service_account_token(&sa_path_owned)
                    .await
                    .map(|t| t.access_token)
            }
        })
    })
    .join();

    let token = match token_result {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            return StepResult {
                success: false,
                message: format!("GSC auth failed: {}", e),
                output: None,
            }
        }
        Err(_) => {
            return StepResult {
                success: false,
                message: "GSC auth thread panicked".to_string(),
                output: None,
            }
        }
    };

    // 3. Load articles from SQLite (canonical runtime store)
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
                output: None,
            };
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load articles from DB: {}", e),
                output: None,
            };
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
                return StepResult {
                    success: false,
                    message: "No site_url found in manifest.json — add 'url' or 'gsc_site' field"
                        .to_string(),
                    output: None,
                }
            }
        }
    };

    let base_url = if site_url.starts_with("sc-domain:") {
        format!("https://{}", &site_url["sc-domain:".len()..])
    } else {
        site_url.clone()
    };
    let base_url = base_url.trim_end_matches('/').to_string();
    let _ = &base_url;

    // 5. Fetch GSC metrics (90-day window) - Spawn thread with own runtime
    let days = 90i64;
    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let start = end - chrono::Duration::days(days - 1);
    let token_clone = token.clone();
    let site_url_clone = site_url.clone();
    let start_str = start.format("%Y-%m-%d").to_string();
    let end_str = end.format("%Y-%m-%d").to_string();

    let page_rows_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            crate::gsc::analytics::fetch_page_rows(
                &token_clone,
                &site_url_clone,
                &start_str,
                &end_str,
                1000,
            )
            .await
        })
    })
    .join();

    let page_rows = match page_rows_result {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            return StepResult {
                success: false,
                message: format!("GSC fetch failed: {}", e),
                output: None,
            }
        }
        Err(_) => {
            return StepResult {
                success: false,
                message: "GSC fetch thread panicked".to_string(),
                output: None,
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

    // 8. Re-export projection so articles.json gets the new GSC metadata
    if let Err(e) = crate::content::article_index::export_projection(
        &db,
        &task.project_id,
        std::path::Path::new(project_path),
    ) {
        return StepResult {
            success: false,
            message: format!("GSC sync succeeded but projection export failed: {}", e),
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

    StepResult {
        success: true,
        message: format!(
            "GSC sync: matched {}/{} articles ({} GSC pages fetched)",
            matched,
            matched + unmatched,
            page_rows.len()
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}
