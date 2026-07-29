use std::collections::HashMap;

use crate::config::env_resolver::EnvResolver;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

// ─── GSC collection ───────────────────────────────────────────────────────────

/// Native Rust implementation of the GSC collection step.
///
/// 1. Resolves site/sitemap via shared helper (manifest → seo_workspace → DB).
/// 2. Mints a service account token.
/// 3. Fetches all sitemap URLs; sends at most [`super::GSC_INSPECTION_CAP`]
///    to the URL Inspection API.
/// 4. Calls the URL Inspection API for each capped URL.
/// 5. Classifies each result into a reason code.
/// 6. Writes `gsc_collection.json` to the automation dir, including coverage
///    meta (`sitemap_url_count`, `inspected_count`, `cap`, `truncated`) so
///    drift detection can tell cap-skipped URLs apart from URLs GSC has
///    genuinely never inspected (issue #26).
pub(crate) fn exec_collect_gsc(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. Resolve site_url + sitemap_url (manifest → seo_workspace → projects DB)
    let site_cfg = match super::resolve_site_config(&task.project_id, project_path) {
        Ok(cfg) => cfg,
        Err(msg) => return StepResult::fail(msg),
    };
    let site_url = site_cfg.site_url;
    let sitemap_url = site_cfg.sitemap_url;

    log::info!(
        "[collect_gsc] site_url={} sitemap_url={} source={}",
        site_url,
        sitemap_url,
        site_cfg.source.as_str()
    );
    let site_match_prefix = super::normalize_site_for_url_match(&site_url);

    // 2. Credentials + token
    let sa_path = match resolver
        .resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        Some(p) => p,
        None => {
            return StepResult::fail("GSC_SERVICE_ACCOUNT_PATH not configured — add it in Settings → Secrets"
                    .to_string())
        }
    };

    // 2-4. Credentials + fetch sitemap + URL Inspection API - All in one thread with own runtime
    let sa_path_owned = sa_path.clone();
    let sitemap_url_owned = sitemap_url.clone();
    let site_url_owned = site_url.clone();

    let gsc_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            // Get token — always mint fresh from service account
            let token = crate::gsc::auth::get_service_account_token(&sa_path_owned)
                .await
                .map(|t| t.access_token)?;

            // Fetch the full sitemap so the true URL count is known, then cap
            // how many URLs are sent to the URL Inspection API (issue #26).
            let entries =
                crate::gsc::sitemap::fetch_sitemap_entries(&sitemap_url_owned, usize::MAX).await?;
            if entries.is_empty() {
                return Err(crate::error::Error::Other(format!(
                    "Sitemap at '{}' is empty or unreachable",
                    sitemap_url_owned
                )));
            }
            let sitemap_url_count = entries.len();
            let urls: Vec<String> = entries
                .into_iter()
                .take(super::GSC_INSPECTION_CAP)
                .map(|e| e.url)
                .collect();

            // URL Inspection API
            let records =
                crate::gsc::indexing::inspect_batch(&token, &site_url_owned, urls).await?;

            Ok::<_, crate::error::Error>((records, token, sitemap_url_count))
        })
    })
    .join();

    let (records, _token, sitemap_url_count) = match gsc_result {
        Ok(Ok((r, t, n))) => (r, t, n),
        Ok(Err(e)) => {
            let msg = e.to_string();
            return StepResult::fail(if msg.contains("sitemap") || msg.contains("Sitemap") {
                    format!("Failed to fetch sitemap: {}", msg)
                } else if msg.contains("auth") || msg.contains("token") {
                    format!("GSC auth failed: {}", msg)
                } else {
                    format!("URL Inspection API failed: {}", msg)
                });
        }
        Err(_) => {
            return StepResult::fail("GSC collection thread panicked".to_string())
        }
    };

    log::info!("[collect_gsc] {} URLs inspected", records.len());

    // Fast-fail: check that records domain matches gsc_site
    let sample_size = records.len().min(10);

    // Normalize for comparison (strip scheme and www.)
    let site_normalized = super::normalize_url_for_comparison(&site_match_prefix);
    let sample_matches = records
        .iter()
        .take(sample_size)
        .filter(|r| super::normalize_url_for_comparison(&r.url).starts_with(&site_normalized))
        .count();

    // Debug: log the comparison
    if sample_size > 0 {
        let first_urls: Vec<&str> = records.iter().take(3).map(|s| s.url.as_str()).collect();
        log::info!(
            "[collect_gsc] site_match_prefix='{}' (normalized: '{}'), sample URLs: {:?}",
            site_match_prefix,
            site_normalized,
            first_urls
        );
        log::info!(
            "[collect_gsc] URL match check: {}/{} match normalized prefix '{}'",
            sample_matches,
            sample_size,
            site_normalized
        );
    }

    if sample_size > 0 && sample_matches == 0 {
        return StepResult::fail(format!(
                "GSC site URL mismatch: 0/{} inspected URLs match '{}'. Check gsc_site/url in manifest.json or projects.site_url.",
                sample_size, site_match_prefix
            ));
    }

    // 5. Domain validation (normalize for www. comparison)
    let site_domain_normalized = super::normalize_url_for_comparison(&site_match_prefix);
    let url_matching = records
        .iter()
        .filter(|r| {
            super::normalize_url_for_comparison(&r.url).starts_with(&site_domain_normalized)
        })
        .count();
    if records.len() > 5 && url_matching < records.len() / 2 {
        return StepResult::fail(format!(
                "GSC site URL mismatch: only {}/{} URLs match '{}'. Check gsc_site/url in manifest.json or projects.site_url.",
                url_matching,
                records.len(),
                site_match_prefix
            ));
    }

    // 6. Build output
    let mut counts: HashMap<String, u32> = HashMap::new();
    for rec in &records {
        *counts
            .entry(rec.reason_code.as_deref().unwrap_or("unknown").to_string())
            .or_insert(0) += 1;
    }

    let issues_found = records
        .iter()
        .filter(|r| {
            !crate::gsc::indexing::is_non_actionable_reason(r.reason_code.as_deref().unwrap_or(""))
        })
        .count();

    let mut items: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "url": r.url,
                "verdict": r.verdict,
                "coverage_state": r.coverage_state,
                "reason_code": r.reason_code,
                "action": r.action,
                "priority": r.priority,
            })
        })
        .collect();
    items.sort_by_key(|item| item["priority"].as_i64().unwrap_or(999));

    let now_iso = chrono::Utc::now().to_rfc3339();
    let collection = serde_json::json!({
        "meta": {
            "site_url": site_url,
            "sitemap_url": sitemap_url,
            "collected_at": now_iso,
            "total_urls": records.len(),
            "issues_found": issues_found,
            // Inspection coverage (issue #26): lets drift detection distinguish
            // URLs GSC has genuinely never inspected from URLs that were simply
            // skipped because the sitemap exceeded the inspection cap.
            "sitemap_url_count": sitemap_url_count,
            "inspected_count": records.len(),
            "cap": super::GSC_INSPECTION_CAP,
            "truncated": sitemap_url_count > super::GSC_INSPECTION_CAP,
        },
        "counts": counts,
        "items": items,
    });

    // 7. Write gsc_collection.json
    let output_path = paths.automation_dir.join("gsc_collection.json");
    if let Err(e) = std::fs::create_dir_all(&paths.automation_dir) {
        return StepResult::fail(format!("Failed to create automation dir: {}", e));
    }
    if let Err(e) =
        crate::engine::exec::common::write_json(&output_path, &collection, "gsc_collection.json")
    {
        return e;
    }

    log::info!(
        "[collect_gsc] wrote {} — {} URLs, {} issues",
        output_path.display(),
        records.len(),
        issues_found
    );

    // ── Also sync Search Analytics metrics so downstream tasks
    // (cannibalization_audit, content_review, etc.) have impression data.
    // This reuses the existing gsc_sync_articles logic rather than
    // duplicating it in a separate manual step.
    let sync_result =
        crate::engine::exec::gsc::exec_gsc_sync_articles(task, project_path, gsc_token);
    let (sync_ok, sync_msg) = (sync_result.success, sync_result.message);

    if !sync_ok {
        log::warn!(
            "[collect_gsc] analytics sync failed — failing the step so the stale-metrics problem is visible: {}",
            sync_msg
        );
        // Fail the step (issue #25): gsc_collection.json is already on disk, so
        // a retry only needs to redo the sync — which is idempotent (DELETE+INSERT).
        return StepResult::fail_with_output(format!(
                "URL inspection succeeded ({} URLs inspected, {} issues found, gsc_collection.json written), but the Search Analytics sync failed: {}. Downstream audits would run on stale metrics. Re-run collect_gsc to retry the sync.",
                records.len(),
                issues_found,
                sync_msg
            ), serde_json::to_string_pretty(&collection).unwrap_or_default());
    }

    log::info!("[collect_gsc] analytics sync succeeded: {}", sync_msg);

    StepResult {
        success: true,
        message: format!(
            "{} URLs inspected, {} issues found. Analytics synced: {}.",
            records.len(),
            issues_found,
            sync_msg
        ),
        output: Some(serde_json::to_string_pretty(&collection).unwrap_or_default()),
        artifact_key: None,
    }
}
