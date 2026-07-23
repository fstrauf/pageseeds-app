use rusqlite::Connection;

use crate::models::ctr::CtrOutcome;
use crate::models::task::Task;

// ─── CTR change events (issue #152) ──────────────────────────────────────────
//
// BUSINESS RULE: dual-layer measurement model
// - Tape: append-only `gsc_page_daily` (per-page daily GSC snapshots)
// - Change events: sparse `ctr_outcomes` rows recorded when a CTR fix ships
// No per-fix `ctr_outcome_review` tasks. Classification prefers 28-day
// windows from the daily tape; live_site_pages / article_metadata are fallbacks.
// `deployed_at` stays null until live title verification (see ctr_audit::outcome).

/// Open / in-flight statuses that a re-ship for the same article supersedes.
const SUPERSEDEABLE_STATUSES: &[&str] = &[
    "pending",
    "deployed",
    "deployment_unverified",
];

/// Record a baseline CtrOutcome when a nested `fix_ctr_article` task completes.
///
/// Reads the article ID from the task's `ctr_context` artifact and delegates to
/// [`record_ctr_change_event`].
pub(crate) fn record_ctr_outcome_baseline(
    conn: &Connection,
    task: &Task,
    _project_path: &str,
) -> crate::error::Result<()> {
    let article_id = task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_context")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v.get("articles")?.as_array()?.first()?.get("id")?.as_i64())
        .ok_or_else(|| {
            crate::error::Error::Other("No article_id in ctr_context artifact".to_string())
        })?;

    record_ctr_change_event(
        conn,
        &task.project_id,
        article_id,
        &task.id,
        None,
        None,
    )
}

/// Record a CTR fix as a change event in `ctr_outcomes`.
///
/// Callable from nested post-success (`fix_ctr_article`) and Path B
/// `fix-submit` (`kind=ctr`). Supersedes prior open events for the same
/// article, leaves `deployed_at` null until deploy verification, and prefers
/// `gsc_page_daily` window metrics for the baseline.
///
/// `url` / `slug` are optional hints for page resolution (Path B typically
/// has a slug; nested fixes may resolve via rendered audits alone).
pub fn record_ctr_change_event(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    fix_task_id: &str,
    url: Option<&str>,
    slug: Option<&str>,
) -> crate::error::Result<()> {
    supersede_open_ctr_outcomes(conn, project_id, article_id, fix_task_id)?;

    let resolved_url = resolve_page_url(conn, project_id, article_id, url, slug);
    let metrics_url = resolved_url.as_deref().unwrap_or("");

    let (baseline_clicks, baseline_impressions, baseline_ctr, baseline_position) =
        crate::engine::exec::ctr_audit::fetch_article_gsc_metrics(
            conn,
            project_id,
            article_id,
            metrics_url,
        );

    let now = chrono::Utc::now();
    let baseline_end_date = now.date_naive() - chrono::Duration::days(1);
    let baseline_start_date = baseline_end_date - chrono::Duration::days(27);
    // Store RFC3339 bounds for the 28d window ending yesterday (matches daily tape).
    let baseline_start = baseline_start_date
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().to_rfc3339())
        .unwrap_or_else(|| (now - chrono::Duration::days(28)).to_rfc3339());
    let baseline_end = baseline_end_date
        .and_hms_opt(23, 59, 59)
        .map(|dt| dt.and_utc().to_rfc3339())
        .unwrap_or_else(|| now.to_rfc3339());

    let outcome = CtrOutcome {
        project_id: project_id.to_string(),
        article_id,
        fix_task_id: fix_task_id.to_string(),
        baseline_start,
        baseline_end,
        after_start: None,
        after_end: None,
        baseline_clicks,
        baseline_impressions,
        baseline_ctr,
        baseline_position,
        after_clicks: None,
        after_impressions: None,
        after_ctr: None,
        after_position: None,
        position_delta: None,
        outcome_status: "pending".to_string(),
        // Deploy clock starts only after live title verification — not at ship.
        deployed_at: None,
        reviewed_at: None,
    };

    crate::db::set_ctr_outcome(conn, &outcome)?;

    log::info!(
        "[ctr_outcome] Recorded change event for article {} (fix {}): clicks={:.1}, impressions={:.1}, ctr={:.4}, position={:.1}",
        article_id,
        fix_task_id,
        baseline_clicks,
        baseline_impressions,
        baseline_ctr,
        baseline_position
    );

    Ok(())
}

/// Mark prior open/pending change events for the same article as `superseded`.
/// Terminal classifications (improved/regressed/neutral/insufficient_data) are kept.
fn supersede_open_ctr_outcomes(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    keep_fix_task_id: &str,
) -> crate::error::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let placeholders = SUPERSEDEABLE_STATUSES
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE ctr_outcomes
         SET outcome_status = 'superseded', reviewed_at = ?1
         WHERE project_id = ?2 AND article_id = ?3
           AND fix_task_id != ?4
           AND outcome_status IN ({placeholders})"
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(now));
    params.push(Box::new(project_id.to_string()));
    params.push(Box::new(article_id));
    params.push(Box::new(keep_fix_task_id.to_string()));
    for s in SUPERSEDEABLE_STATUSES {
        params.push(Box::new(s.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let n = conn.execute(&sql, param_refs.as_slice())?;
    if n > 0 {
        log::info!(
            "[ctr_outcome] Superseded {} open change event(s) for article {} (new fix {})",
            n,
            article_id,
            keep_fix_task_id
        );
    }
    Ok(())
}

/// Resolve a page URL for metrics: explicit hint → rendered audit → slug↔daily page match.
fn resolve_page_url(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    url_hint: Option<&str>,
    slug_hint: Option<&str>,
) -> Option<String> {
    if let Some(u) = url_hint.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(u.to_string());
    }

    if let Some(u) =
        crate::engine::exec::ctr_audit::lookup_rendered_audit_url(conn, project_id, article_id)
    {
        return Some(u);
    }

    let slug = slug_hint
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            conn.query_row(
                "SELECT url_slug FROM articles WHERE project_id = ?1 AND id = ?2",
                rusqlite::params![project_id, article_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .filter(|s| !s.is_empty())
        })?;

    let pages = crate::db::list_gsc_page_daily_pages(conn, project_id).ok()?;
    pages
        .into_iter()
        .find(|p| crate::engine::exec::outcome_review::page_matches_slug(p, &slug))
}

/// Synthetic fix_task_id for Path B `fix-submit` (no nested task row).
///
/// Timestamped so each re-ship is a distinct PK and can supersede prior open events.
pub fn path_b_ctr_fix_task_id(project_id: &str, article_id: i64, slug: &str) -> String {
    let ts = chrono::Utc::now().timestamp_millis();
    format!("path_b_fix:{project_id}:{article_id}:{slug}:{ts}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::gsc::PageDailyMetrics;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj-test', 'Test', '/tmp/ctr_outcome_pa', 1, 'workspace')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (1, 'proj-test', 'Test Article', 'test-article', 'content/1.mdx',
                       'published', 'kw', '[]', 0, 100, 0, 'h')",
            [],
        )
        .unwrap();
        conn
    }

    fn seed_daily(conn: &Connection, page: &str, date: &str, clicks: f64, impressions: f64) {
        let rows = [PageDailyMetrics {
            page: page.to_string(),
            date: date.to_string(),
            clicks,
            impressions,
            ctr: if impressions > 0.0 {
                clicks / impressions
            } else {
                0.0
            },
            position: 8.0,
        }];
        crate::db::insert_gsc_page_daily_snapshots(conn, "proj-test", &rows).unwrap();
    }

    #[test]
    fn record_change_event_sets_pending_and_null_deployed_at() {
        let conn = test_conn();
        let yesterday = (chrono::Utc::now().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let page = "https://example.com/blog/test-article";
        seed_daily(&conn, page, &yesterday, 5.0, 500.0);

        record_ctr_change_event(
            &conn,
            "proj-test",
            1,
            "fix-task-1",
            Some(page),
            Some("test-article"),
        )
        .unwrap();

        let o = crate::db::get_ctr_outcome(&conn, "proj-test", 1, "fix-task-1")
            .unwrap()
            .expect("row");
        assert_eq!(o.outcome_status, "pending");
        assert!(o.deployed_at.is_none(), "deployed_at must stay null until verify");
        assert!(o.baseline_impressions > 0.0);
    }

    #[test]
    fn re_ship_supersedes_prior_pending_for_same_article() {
        let conn = test_conn();
        record_ctr_change_event(&conn, "proj-test", 1, "fix-a", None, Some("test-article"))
            .unwrap();
        record_ctr_change_event(&conn, "proj-test", 1, "fix-b", None, Some("test-article"))
            .unwrap();

        let a = crate::db::get_ctr_outcome(&conn, "proj-test", 1, "fix-a")
            .unwrap()
            .expect("first");
        let b = crate::db::get_ctr_outcome(&conn, "proj-test", 1, "fix-b")
            .unwrap()
            .expect("second");
        assert_eq!(a.outcome_status, "superseded");
        assert_eq!(b.outcome_status, "pending");
        assert!(b.deployed_at.is_none());
    }

    #[test]
    fn terminal_outcomes_are_not_superseded() {
        let conn = test_conn();
        let terminal = CtrOutcome {
            project_id: "proj-test".into(),
            article_id: 1,
            fix_task_id: "old-improved".into(),
            baseline_start: "2026-01-01T00:00:00Z".into(),
            baseline_end: "2026-01-28T00:00:00Z".into(),
            after_start: None,
            after_end: None,
            baseline_clicks: 10.0,
            baseline_impressions: 1000.0,
            baseline_ctr: 0.01,
            baseline_position: 5.0,
            after_clicks: Some(20.0),
            after_impressions: Some(1000.0),
            after_ctr: Some(0.02),
            after_position: Some(4.0),
            position_delta: Some(-1.0),
            outcome_status: "improved".into(),
            deployed_at: Some("2026-02-01T00:00:00Z".into()),
            reviewed_at: Some("2026-02-20T00:00:00Z".into()),
        };
        crate::db::set_ctr_outcome(&conn, &terminal).unwrap();

        record_ctr_change_event(&conn, "proj-test", 1, "fix-new", None, Some("test-article"))
            .unwrap();

        let old = crate::db::get_ctr_outcome(&conn, "proj-test", 1, "old-improved")
            .unwrap()
            .unwrap();
        assert_eq!(old.outcome_status, "improved");
    }

    #[test]
    fn path_b_synthetic_id_is_unique_per_call() {
        let a = path_b_ctr_fix_task_id("p", 1, "slug");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = path_b_ctr_fix_task_id("p", 1, "slug");
        assert_ne!(a, b);
        assert!(a.starts_with("path_b_fix:p:1:slug:"));
    }
}
