/// CTR outcome tracking — compare before/after metrics and generate reports.
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Minimum days after verified deployment before judging outcomes.
const MIN_POST_DEPLOY_DAYS: i64 = 14;

/// Days after the local fix write within which the live page must show the new
/// title. Past this window without a title match the outcome is marked
/// `deployment_unverified` instead of being compared against stale SERP data.
const DEPLOY_VERIFY_TIMEOUT_DAYS: i64 = 7;

/// Minimum after-period impressions for a verdict. Provisional floor — the
/// exact value is a product decision (see issue #29 open questions). Below
/// this (which includes all-zero after-metrics) the outcome is classified
/// `insufficient_data`, never `regressed`.
const MIN_AFTER_IMPRESSIONS: f64 = 100.0;

/// Per-day click-loss threshold that counts as a regression (~3 clicks/month).
const REGRESSED_CLICKS_PER_DAY: f64 = -0.1;

/// Load deployed outcomes, fetch current GSC metrics, compare before/after.
pub(crate) fn exec_ctr_outcome_compare(
    task: &Task,
    _project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let outcomes = match crate::db::list_ctr_outcomes(conn, &task.project_id) {
        Ok(o) => o,
        Err(e) => {
            return StepResult::fail(format!("Failed to load outcomes: {}", e));
        }
    };

    if outcomes.is_empty() {
        return StepResult {
            success: true,
            message: "No CTR outcomes to review".to_string(),
            output: Some("[]".to_string()),
            artifact_key: None,
        };
    }

    let articles = crate::engine::task_store::list_articles(conn, &task.project_id)
        .unwrap_or_default();

    let now = chrono::Utc::now();
    let mut ready = Vec::new();
    let mut waiting = Vec::new();

    for mut outcome in outcomes {
        // ── Deployment verification gate ────────────────────────────────────
        // `pending` outcomes have not been confirmed live yet;
        // `deployment_unverified` ones are re-checked each run so a late
        // deploy still self-heals. Only compare metrics once the live page
        // renders the new source title.
        if matches!(
            outcome.outcome_status.as_str(),
            "pending" | "deployment_unverified"
        ) {
            let url = lookup_rendered_audit_url(conn, &task.project_id, outcome.article_id);
            let source_title = articles
                .iter()
                .find(|a| a.id == outcome.article_id)
                .map(|a| a.title.clone());

            let verified = match (url, source_title) {
                (Some(u), Some(t)) => deployment_verified(&u, &t),
                // Baseline recording guarantees a rendered-audit URL and the
                // article always exists; a miss here means we cannot verify,
                // so treat as unverified rather than comparing blindly.
                _ => false,
            };

            if verified {
                // The outcome clock starts at verified deployment, not at the
                // local file write.
                outcome.deployed_at = Some(now.to_rfc3339());
                outcome.outcome_status = "deployed".to_string();
                outcome.reviewed_at = Some(now.to_rfc3339());
                if let Err(e) = crate::db::set_ctr_outcome(conn, &outcome) {
                    log::warn!(
                        "[ctr_outcome] Failed to mark deployment verified for article {}: {}",
                        outcome.article_id,
                        e
                    );
                }
                waiting.push(serde_json::json!({
                    "article_id": outcome.article_id,
                    "fix_task_id": outcome.fix_task_id,
                    "status": "deployed",
                    "reason": "Deployment verified — outcome clock started",
                }));
                continue;
            }

            // Not verified: measure the timeout from the fix time (baseline_end
            // is recorded at fix completion).
            let days_since_fix = chrono::DateTime::parse_from_rfc3339(&outcome.baseline_end)
                .map(|fix| (now - fix.with_timezone(&chrono::Utc)).num_days())
                .unwrap_or(0);

            if days_since_fix >= DEPLOY_VERIFY_TIMEOUT_DAYS {
                if outcome.outcome_status != "deployment_unverified" {
                    outcome.outcome_status = "deployment_unverified".to_string();
                    outcome.reviewed_at = Some(now.to_rfc3339());
                    if let Err(e) = crate::db::set_ctr_outcome(conn, &outcome) {
                        log::warn!(
                            "[ctr_outcome] Failed to mark deployment_unverified for article {}: {}",
                            outcome.article_id,
                            e
                        );
                    }
                }
                ready.push(serde_json::json!({
                    "article_id": outcome.article_id,
                    "fix_task_id": outcome.fix_task_id,
                    "status": "deployment_unverified",
                    "days_since_fix": days_since_fix,
                }));
            } else {
                waiting.push(serde_json::json!({
                    "article_id": outcome.article_id,
                    "fix_task_id": outcome.fix_task_id,
                    "days_since_fix": days_since_fix,
                    "status": "awaiting_deployment",
                    "reason": "Live page does not show the new title yet",
                }));
            }
            continue;
        }

        // ── Outcome clock ───────────────────────────────────────────────────
        let deployed_at = match outcome.deployed_at.as_deref() {
            Some(d) => match chrono::DateTime::parse_from_rfc3339(d) {
                Ok(dt) => dt.with_timezone(&chrono::Utc),
                Err(_) => continue,
            },
            None => continue,
        };

        let days_since_deploy = (now - deployed_at).num_days();
        if days_since_deploy < MIN_POST_DEPLOY_DAYS {
            waiting.push(serde_json::json!({
                "article_id": outcome.article_id,
                "fix_task_id": outcome.fix_task_id,
                "days_since_deploy": days_since_deploy,
                "status": "waiting",
                "reason": format!("Need {} more days", MIN_POST_DEPLOY_DAYS - days_since_deploy)
            }));
            continue;
        }

        // ── After-metrics + classification ──────────────────────────────────
        let (after_clicks, after_impressions, after_ctr, after_position) =
            fetch_after_metrics(conn, &task.project_id, outcome.article_id);

        let after_start = deployed_at.to_rfc3339();
        let after_end = now.to_rfc3339();
        outcome.after_start = Some(after_start.clone());
        outcome.after_end = Some(after_end.clone());
        outcome.after_clicks = Some(after_clicks);
        outcome.after_impressions = Some(after_impressions);
        outcome.after_ctr = Some(after_ctr);
        outcome.after_position = Some(after_position);
        outcome.position_delta = Some(after_position - outcome.baseline_position);

        let baseline_days = window_days(&outcome.baseline_start, &outcome.baseline_end);
        let after_days = window_days(&after_start, &after_end);
        let baseline_clicks_per_day = outcome.baseline_clicks / baseline_days as f64;
        let after_clicks_per_day = after_clicks / after_days as f64;

        let status = classify_outcome(
            outcome.baseline_ctr,
            outcome.baseline_clicks,
            baseline_days,
            after_ctr,
            after_clicks,
            after_impressions,
            after_days,
        );
        outcome.outcome_status = status.to_string();
        outcome.reviewed_at = Some(now.to_rfc3339());

        if let Err(e) = crate::db::set_ctr_outcome(conn, &outcome) {
            log::warn!(
                "[ctr_outcome] Failed to update outcome for article {}: {}",
                outcome.article_id,
                e
            );
        }

        ready.push(serde_json::json!({
            "article_id": outcome.article_id,
            "fix_task_id": outcome.fix_task_id,
            "status": status,
            "baseline_days": baseline_days,
            "after_days": after_days,
            "baseline_clicks": outcome.baseline_clicks,
            "after_clicks": after_clicks,
            "baseline_clicks_per_day": baseline_clicks_per_day,
            "after_clicks_per_day": after_clicks_per_day,
            "click_gain_per_day": after_clicks_per_day - baseline_clicks_per_day,
            "baseline_ctr": outcome.baseline_ctr,
            "after_ctr": after_ctr,
            "ctr_delta": after_ctr - outcome.baseline_ctr,
            "position_delta": outcome.position_delta,
        }));
    }

    let summary = serde_json::json!({
        "ready": ready.len(),
        "waiting": waiting.len(),
        "results": ready,
        "pending": waiting,
    });

    StepResult {
        success: true,
        message: format!(
            "CTR outcome review: {} ready, {} waiting for more data",
            ready.len(),
            waiting.len()
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
        artifact_key: None,
    }
}

/// Generate a structured report artifact from the outcome comparison.
pub(crate) fn exec_ctr_outcome_report(
    task: &Task,
    _project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let outcomes = match crate::db::list_ctr_outcomes(conn, &task.project_id) {
        Ok(o) => o,
        Err(e) => {
            return StepResult::fail(format!("Failed to load outcomes: {}", e));
        }
    };

    let mut improved = 0usize;
    let mut regressed = 0usize;
    let mut neutral = 0usize;
    let mut insufficient_data = 0usize;
    let mut deployment_unverified = 0usize;
    let mut pending = 0usize;
    let mut total_baseline_clicks = 0.0;
    let mut total_after_clicks = 0.0;

    for o in &outcomes {
        match o.outcome_status.as_str() {
            "improved" => improved += 1,
            "regressed" => regressed += 1,
            "neutral" => neutral += 1,
            "insufficient_data" => insufficient_data += 1,
            "deployment_unverified" => deployment_unverified += 1,
            "pending" | "deployed" => pending += 1,
            _ => {}
        }
        total_baseline_clicks += o.baseline_clicks;
        if let Some(after) = o.after_clicks {
            total_after_clicks += after;
        }
    }

    let report = serde_json::json!({
        "project_id": task.project_id,
        "reviewed_at": chrono::Utc::now().to_rfc3339(),
        "summary": {
            "total_tracked": outcomes.len(),
            "improved": improved,
            "regressed": regressed,
            "neutral": neutral,
            "insufficient_data": insufficient_data,
            "deployment_unverified": deployment_unverified,
            "pending": pending,
            "total_baseline_clicks": total_baseline_clicks,
            "total_after_clicks": total_after_clicks,
            "total_click_gain": total_after_clicks - total_baseline_clicks,
        },
        "outcomes": outcomes,
    });

    StepResult {
        success: true,
        message: format!(
            "CTR outcome report: {} improved, {} regressed, {} neutral, {} insufficient data, {} deployment unverified, {} pending",
            improved, regressed, neutral, insufficient_data, deployment_unverified, pending
        ),
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
        artifact_key: None,
    }
}

/// Classify an outcome from baseline/after metrics over their explicit windows.
///
/// Pure function — unit-tested without DB or network. Clicks are normalized to
/// per-day rates over each window so a 28-day baseline and a 60-day after
/// window compare fairly; CTR is already a rate. Anything below the
/// impressions floor (including all-zero after-metrics) is `insufficient_data`
/// and can never classify as `regressed`.
fn classify_outcome(
    baseline_ctr: f64,
    baseline_clicks: f64,
    baseline_days: i64,
    after_ctr: f64,
    after_clicks: f64,
    after_impressions: f64,
    after_days: i64,
) -> &'static str {
    if after_impressions < MIN_AFTER_IMPRESSIONS {
        return "insufficient_data";
    }

    let baseline_clicks_per_day = baseline_clicks / baseline_days.max(1) as f64;
    let after_clicks_per_day = after_clicks / after_days.max(1) as f64;
    let ctr_delta = after_ctr - baseline_ctr;
    let click_gain_per_day = after_clicks_per_day - baseline_clicks_per_day;

    if ctr_delta > 0.001 && click_gain_per_day > 0.0 {
        "improved"
    } else if ctr_delta < -0.001 || click_gain_per_day < REGRESSED_CLICKS_PER_DAY {
        "regressed"
    } else {
        "neutral"
    }
}

/// Inclusive day count between two RFC3339 timestamps, minimum 1.
fn window_days(start: &str, end: &str) -> i64 {
    let parsed = || {
        let s = chrono::DateTime::parse_from_rfc3339(start).ok()?;
        let e = chrono::DateTime::parse_from_rfc3339(end).ok()?;
        Some((e - s).num_days())
    };
    parsed().unwrap_or(0).max(1)
}

/// Fetch the rendered <title> of the live page and check it against the new
/// source title. Any fetch failure counts as unverified (the compare loop
/// applies the timeout policy).
fn deployment_verified(url: &str, source_title: &str) -> bool {
    match super::rendered::fetch_rendered_title(url) {
        Ok(rendered) => {
            super::rendered::rendered_title_matches_source(source_title, &rendered)
        }
        Err(e) => {
            log::warn!("[ctr_outcome] Deployment check failed for {}: {}", url, e);
            false
        }
    }
}

/// The live URL recorded for an article by the rendered SERP audit.
pub(crate) fn lookup_rendered_audit_url(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
) -> Option<String> {
    conn.query_row(
        "SELECT url FROM ctr_rendered_page_audits WHERE project_id = ?1 AND article_id = ?2",
        rusqlite::params![project_id, article_id],
        |row| row.get(0),
    )
    .ok()
}

fn fetch_after_metrics(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
) -> (f64, f64, f64, f64) {
    match lookup_rendered_audit_url(conn, project_id, article_id) {
        Some(url) => fetch_article_gsc_metrics(conn, project_id, article_id, &url),
        None => (0.0, 0.0, 0.0, 0.0),
    }
}

/// Fetch GSC metrics for an article for CTR change-event baselines / after windows.
///
/// Prefer **28-day aggregates from `gsc_page_daily`** (ending yesterday) when the
/// page maps to daily rows. Fall back to point-in-time `live_site_pages`, then
/// `article_metadata` namespace='gsc', else zeros.
///
/// Shared by the change-event recorder (`engine::post_actions`) and the
/// after-metrics fetcher so both sides of the comparison read from the same source.
pub(crate) fn fetch_article_gsc_metrics(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
    url: &str,
) -> (f64, f64, f64, f64) {
    // 1. Daily tape (source of truth for classification windows)
    if let Some(m) = metrics_from_gsc_page_daily(conn, project_id, article_id, url) {
        return m;
    }

    // 2. Live-site path lookup (point-in-time)
    if !url.is_empty() {
        let path = url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let path = if let Some(pos) = path.find('/') {
            &path[pos..]
        } else {
            "/"
        };

        let live_site_result: Result<
            (Option<f64>, Option<f64>, Option<f64>, Option<f64>),
            rusqlite::Error,
        > = conn.query_row(
            "SELECT gsc_clicks, gsc_impressions, gsc_ctr, gsc_position
                 FROM live_site_pages
                 WHERE project_id = ?1 AND path = ?2",
            rusqlite::params![project_id, path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        );

        if let Ok((Some(clicks), Some(impressions), Some(ctr), Some(position))) = live_site_result {
            return (clicks, impressions, ctr, position);
        }
    }

    // 3. Workspace fallback: article_metadata namespace='gsc'
    let meta_result: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT payload FROM article_metadata WHERE project_id = ?1 AND article_id = ?2 AND namespace = 'gsc'",
        rusqlite::params![project_id, article_id],
        |row| row.get(0),
    );

    if let Ok(payload) = meta_result {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&payload) {
            let clicks = val["clicks"].as_f64().unwrap_or(0.0);
            let impressions = val["impressions"].as_f64().unwrap_or(0.0);
            let ctr = val["ctr"].as_f64().unwrap_or(0.0);
            let position = val["avg_position"].as_f64().unwrap_or(0.0);
            if clicks > 0.0 || impressions > 0.0 {
                return (clicks, impressions, ctr, position);
            }
        }
    }

    // 4. Nothing available
    (0.0, 0.0, 0.0, 0.0)
}

/// 28-day inclusive window ending yesterday from `gsc_page_daily`, when rows exist.
fn metrics_from_gsc_page_daily(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
    url: &str,
) -> Option<(f64, f64, f64, f64)> {
    let page = resolve_gsc_daily_page(conn, project_id, article_id, url)?;
    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let start = end - chrono::Duration::days(27);
    let start_s = start.format("%Y-%m-%d").to_string();
    let end_s = end.format("%Y-%m-%d").to_string();
    let m = crate::db::gsc_page_daily_window_metrics(conn, project_id, &page, &start_s, &end_s)
        .ok()
        .flatten()?;
    let ctr = if m.impressions > 0.0 {
        m.clicks / m.impressions
    } else {
        0.0
    };
    Some((m.clicks, m.impressions, ctr, m.position))
}

/// Map article → `gsc_page_daily.page` key (exact URL or slug match).
fn resolve_gsc_daily_page(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
    url: &str,
) -> Option<String> {
    let pages = crate::db::list_gsc_page_daily_pages(conn, project_id).ok()?;
    if pages.is_empty() {
        return None;
    }

    if !url.is_empty() {
        if pages.iter().any(|p| p == url) {
            return Some(url.to_string());
        }
        // Trailing-slash / http(s) variants: match by slug extracted from URL.
        let slug = crate::content::slug::extract_slug_from_url(url);
        if !slug.is_empty() {
            if let Some(p) = pages
                .iter()
                .find(|p| crate::engine::exec::outcome_review::page_matches_slug(p, &slug))
            {
                return Some(p.clone());
            }
        }
    }

    let slug: String = conn
        .query_row(
            "SELECT url_slug FROM articles WHERE project_id = ?1 AND id = ?2",
            rusqlite::params![project_id, article_id],
            |row| row.get(0),
        )
        .ok()
        .filter(|s: &String| !s.is_empty())?;

    pages
        .into_iter()
        .find(|p| crate::engine::exec::outcome_review::page_matches_slug(p, &slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        // FK enforcement requires a project row before sidecar inserts.
        crate::engine::exec::ctr_audit::tests::insert_test_project(&conn, "/tmp/ctr_outcome_test");
        conn
    }

    fn seed_rendered_audit_url(conn: &rusqlite::Connection, article_id: i64, url: &str) {
        conn.execute(
            "INSERT INTO ctr_rendered_page_audits (project_id, article_id, url, file, checked_at)
             VALUES ('proj-test', ?1, ?2, 'content/001_test.mdx', '2026-07-01T00:00:00Z')",
            rusqlite::params![article_id, url],
        )
        .unwrap();
    }

    fn seed_article_row(conn: &rusqlite::Connection, article_id: i64, slug: &str) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (?1, 'proj-test', 'T', ?2, 'content/t.mdx', 'published', 'kw',
                       '[]', 0, 100, 0, 'h')",
            rusqlite::params![article_id, slug],
        )
        .unwrap();
    }

    fn seed_daily_window(
        conn: &rusqlite::Connection,
        page: &str,
        clicks: f64,
        impressions: f64,
    ) {
        let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
        let start = end - chrono::Duration::days(2);
        let rows = vec![
            crate::models::gsc::PageDailyMetrics {
                page: page.to_string(),
                date: start.format("%Y-%m-%d").to_string(),
                clicks: clicks / 2.0,
                impressions: impressions / 2.0,
                ctr: 0.0,
                position: 10.0,
            },
            crate::models::gsc::PageDailyMetrics {
                page: page.to_string(),
                date: end.format("%Y-%m-%d").to_string(),
                clicks: clicks / 2.0,
                impressions: impressions / 2.0,
                ctr: 0.0,
                position: 6.0,
            },
        ];
        crate::db::insert_gsc_page_daily_snapshots(conn, "proj-test", &rows).unwrap();
    }

    /// Workspace fallback: no daily / live_site rows → article_metadata.
    #[test]
    fn fetch_after_metrics_falls_back_to_article_metadata() {
        let conn = test_conn();
        seed_rendered_audit_url(&conn, 1, "https://example.com/blog/test-article");
        conn.execute(
            "INSERT INTO article_metadata (project_id, article_id, namespace, payload, updated_at)
             VALUES ('proj-test', 1, 'gsc', ?1, '2026-07-01T00:00:00Z')",
            rusqlite::params![serde_json::json!({
                "clicks": 12.0, "impressions": 4000.0, "ctr": 0.003, "avg_position": 9.5
            }).to_string()],
        )
        .unwrap();

        let (clicks, impressions, ctr, position) = fetch_after_metrics(&conn, "proj-test", 1);
        assert_eq!(clicks, 12.0);
        assert_eq!(impressions, 4000.0);
        assert_eq!(ctr, 0.003);
        assert_eq!(position, 9.5);
    }

    /// live_site_pages when no daily tape rows exist.
    #[test]
    fn fetch_after_metrics_prefers_live_site_pages_without_daily() {
        let conn = test_conn();
        seed_rendered_audit_url(&conn, 1, "https://example.com/blog/test-article");
        conn.execute(
            "INSERT INTO live_site_pages (project_id, url, path, gsc_clicks, gsc_impressions, gsc_ctr, gsc_position, last_crawled_at)
             VALUES ('proj-test', 'https://example.com/blog/test-article', '/blog/test-article', 7.0, 900.0, 0.0078, 5.0, '2026-07-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO article_metadata (project_id, article_id, namespace, payload, updated_at)
             VALUES ('proj-test', 1, 'gsc', ?1, '2026-07-01T00:00:00Z')",
            rusqlite::params![serde_json::json!({
                "clicks": 12.0, "impressions": 4000.0, "ctr": 0.003, "avg_position": 9.5
            }).to_string()],
        )
        .unwrap();

        let (clicks, impressions, _, _) = fetch_after_metrics(&conn, "proj-test", 1);
        assert_eq!(clicks, 7.0);
        assert_eq!(impressions, 900.0);
    }

    /// `gsc_page_daily` window aggregates beat live_site / metadata point-in-time.
    #[test]
    fn fetch_article_gsc_metrics_prefers_daily_window() {
        let conn = test_conn();
        seed_article_row(&conn, 1, "test-article");
        let page = "https://example.com/blog/test-article";
        seed_rendered_audit_url(&conn, 1, page);
        seed_daily_window(&conn, page, 40.0, 2000.0);
        conn.execute(
            "INSERT INTO live_site_pages (project_id, url, path, gsc_clicks, gsc_impressions, gsc_ctr, gsc_position, last_crawled_at)
             VALUES ('proj-test', ?1, '/blog/test-article', 7.0, 900.0, 0.0078, 5.0, '2026-07-01T00:00:00Z')",
            rusqlite::params![page],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO article_metadata (project_id, article_id, namespace, payload, updated_at)
             VALUES ('proj-test', 1, 'gsc', ?1, '2026-07-01T00:00:00Z')",
            rusqlite::params![serde_json::json!({
                "clicks": 12.0, "impressions": 4000.0, "ctr": 0.003, "avg_position": 9.5
            }).to_string()],
        )
        .unwrap();

        let (clicks, impressions, ctr, position) =
            fetch_article_gsc_metrics(&conn, "proj-test", 1, page);
        assert_eq!(clicks, 40.0);
        assert_eq!(impressions, 2000.0);
        assert!((ctr - 0.02).abs() < 1e-9);
        // Impressions-weighted position: equal impr → (10+6)/2 = 8
        assert!((position - 8.0).abs() < 1e-9);
    }

    /// No data anywhere → zeros (which classify as insufficient_data).
    #[test]
    fn fetch_after_metrics_returns_zeros_without_data() {
        let conn = test_conn();
        seed_rendered_audit_url(&conn, 1, "https://example.com/blog/test-article");
        assert_eq!(fetch_after_metrics(&conn, "proj-test", 1), (0.0, 0.0, 0.0, 0.0));
    }

    /// All-zero after-metrics must classify insufficient_data, never regressed.
    #[test]
    fn all_zero_after_metrics_is_insufficient_data_not_regressed() {
        let status = classify_outcome(
            0.05,  // baseline_ctr
            100.0, // baseline_clicks — large baseline
            28,
            0.0, // after_ctr
            0.0, // after_clicks
            0.0, // after_impressions — the old code called this "regressed"
            14,
        );
        assert_eq!(status, "insufficient_data");
    }

    /// Below the impressions floor (but non-zero) is also insufficient_data.
    #[test]
    fn below_impressions_floor_is_insufficient_data() {
        let status = classify_outcome(0.05, 100.0, 28, 0.0, 0.0, 50.0, 14);
        assert_eq!(status, "insufficient_data");
    }

    /// A genuine drop above the floor still classifies as regressed.
    #[test]
    fn genuine_drop_above_floor_is_regressed() {
        let status = classify_outcome(
            0.05,   // baseline_ctr
            28.0,   // 1 click/day baseline
            28,
            0.01,   // after_ctr — large drop
            14.0,   // 1 click/day over 14 days
            5000.0, // plenty of impressions
            14,
        );
        assert_eq!(status, "regressed");
    }

    /// Window normalization: equal raw click totals over different window
    /// lengths must compare per-day rates, not raw aggregates.
    #[test]
    fn classification_normalizes_clicks_per_day_over_windows() {
        // Same 28 clicks, but the after window is half as long → 2x rate,
        // with a CTR lift → improved (raw-total comparison would say neutral).
        let status = classify_outcome(0.02, 28.0, 28, 0.03, 28.0, 5000.0, 14);
        assert_eq!(status, "improved");

        // Same 28 clicks over double the window → half the rate. CTR also
        // dropped → regressed (raw-total click_gain would be exactly 0).
        let status = classify_outcome(0.02, 28.0, 28, 0.01, 28.0, 5000.0, 56);
        assert_eq!(status, "regressed");
    }

    #[test]
    fn window_days_counts_between_timestamps_with_minimum_one() {
        assert_eq!(
            window_days("2026-07-01T00:00:00Z", "2026-07-29T00:00:00Z"),
            28
        );
        assert_eq!(
            window_days("2026-07-01T00:00:00Z", "2026-07-01T00:00:00Z"),
            1
        );
        assert_eq!(window_days("garbage", "2026-07-01T00:00:00Z"), 1);
    }
}
