use rusqlite::Connection;

use crate::models::ctr::CtrOutcome;
use crate::models::task::Task;

// ─── CTR Outcome Baseline Recording ──────────────────────────────────────────

/// Record a baseline CtrOutcome when a fix_ctr_article task completes.
///
/// Reads the article ID from the task's ctr_context artifact, looks up the
/// article's URL from ctr_rendered_page_audits, fetches current GSC metrics
/// from live_site_pages (or article_metadata for workspace projects), and
/// inserts a baseline record into ctr_outcomes.
/// This gives the subsequent ctr_outcome_review task data to compare against.
pub(crate) fn record_ctr_outcome_baseline(
    conn: &Connection,
    task: &Task,
    _project_path: &str,
) -> crate::error::Result<()> {
    // Extract article_id from ctr_context artifact
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

    // Look up URL from rendered page audits
    let url = crate::engine::exec::ctr_audit::lookup_rendered_audit_url(
        conn,
        &task.project_id,
        article_id,
    )
    .ok_or_else(|| {
        crate::error::Error::Other(format!("No rendered audit URL for article {}", article_id))
    })?;

    // Fetch current GSC metrics as baseline.
    // For live-site projects: read from live_site_pages.
    // For workspace projects: fall back to article_metadata (namespace='gsc').
    // Shared with the after-metrics fetcher in ctr_audit::outcome so both
    // sides of the comparison read from the same source.
    let (baseline_clicks, baseline_impressions, baseline_ctr, baseline_position) =
        crate::engine::exec::ctr_audit::fetch_article_gsc_metrics(
            conn,
            &task.project_id,
            article_id,
            &url,
        );

    let now = chrono::Utc::now();
    let baseline_start = (now - chrono::Duration::days(28)).to_rfc3339();
    let baseline_end = now.to_rfc3339();

    let outcome = CtrOutcome {
        project_id: task.project_id.clone(),
        article_id,
        fix_task_id: task.id.clone(),
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
        deployed_at: Some(now.to_rfc3339()),
        reviewed_at: None,
    };

    crate::db::set_ctr_outcome(conn, &outcome)?;

    log::info!(
        "[ctr_outcome] Recorded baseline for article {} (task {}): clicks={:.1}, impressions={:.1}, ctr={:.4}, position={:.1}",
        article_id, task.id, baseline_clicks, baseline_impressions, baseline_ctr, baseline_position
    );

    Ok(())
}
