/// CTR outcome tracking — compare before/after metrics and generate reports.

use crate::engine::workflows::StepResult;
use crate::models::ctr::CtrOutcome;
use crate::models::task::Task;

/// Minimum days after deployment before judging outcomes.
const MIN_POST_DEPLOY_DAYS: i64 = 14;

/// Load deployed outcomes, fetch current GSC metrics, compare before/after.
pub(crate) fn exec_ctr_outcome_compare(
    task: &Task,
    _project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let outcomes = match crate::db::list_ctr_outcomes(conn, &task.project_id) {
        Ok(o) => o,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load outcomes: {}", e),
                output: None,
            };
        }
    };

    if outcomes.is_empty() {
        return StepResult {
            success: true,
            message: "No CTR outcomes to review".to_string(),
            output: Some("[]".to_string()),
        };
    }

    let now = chrono::Utc::now();
    let mut ready = Vec::new();
    let mut waiting = Vec::new();

    for mut outcome in outcomes {
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

        // Fetch after-period metrics from live_site_pages or articles.json
        let (after_clicks, after_impressions, after_ctr, after_position) =
            fetch_after_metrics(conn, &task.project_id, outcome.article_id);

        outcome.after_start = Some(deployed_at.to_rfc3339());
        outcome.after_end = Some(now.to_rfc3339());
        outcome.after_clicks = Some(after_clicks);
        outcome.after_impressions = Some(after_impressions);
        outcome.after_ctr = Some(after_ctr);
        outcome.after_position = Some(after_position);
        outcome.position_delta = Some(after_position - outcome.baseline_position);

        let ctr_delta = after_ctr - outcome.baseline_ctr;
        let click_gain = after_clicks - outcome.baseline_clicks;

        let status = if ctr_delta > 0.001 && click_gain > 0.0 {
            "improved"
        } else if ctr_delta < -0.001 || click_gain < -1.0 {
            "regressed"
        } else {
            "neutral"
        };
        outcome.outcome_status = status.to_string();
        outcome.reviewed_at = Some(now.to_rfc3339());

        if let Err(e) = crate::db::set_ctr_outcome(conn, &outcome) {
            log::warn!("[ctr_outcome] Failed to update outcome for article {}: {}", outcome.article_id, e);
        }

        ready.push(serde_json::json!({
            "article_id": outcome.article_id,
            "fix_task_id": outcome.fix_task_id,
            "status": status,
            "baseline_clicks": outcome.baseline_clicks,
            "after_clicks": after_clicks,
            "click_gain": click_gain,
            "baseline_ctr": outcome.baseline_ctr,
            "after_ctr": after_ctr,
            "ctr_delta": ctr_delta,
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
            return StepResult {
                success: false,
                message: format!("Failed to load outcomes: {}", e),
                output: None,
            };
        }
    };

    let mut improved = 0usize;
    let mut regressed = 0usize;
    let mut neutral = 0usize;
    let mut pending = 0usize;
    let mut total_baseline_clicks = 0.0;
    let mut total_after_clicks = 0.0;

    for o in &outcomes {
        match o.outcome_status.as_str() {
            "improved" => improved += 1,
            "regressed" => regressed += 1,
            "neutral" => neutral += 1,
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
            "CTR outcome report: {} improved, {} regressed, {} neutral, {} pending",
            improved, regressed, neutral, pending
        ),
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}

fn fetch_after_metrics(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
) -> (f64, f64, f64, f64) {
    // Try to read from live_site_pages via article URL mapping
    // Fallback: return zeros if no data available
    let url_result: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT url FROM ctr_rendered_page_audits WHERE project_id = ?1 AND article_id = ?2",
        rusqlite::params![project_id, article_id],
        |row| row.get(0),
    );

    let url = match url_result {
        Ok(u) => u,
        Err(_) => return (0.0, 0.0, 0.0, 0.0),
    };

    // Extract path from URL for live_site_pages lookup
    let path = url.trim_start_matches("https://").trim_start_matches("http://");
    let path = if let Some(pos) = path.find('/') {
        &path[pos..]
    } else {
        "/"
    };

    let row_result: Result<(Option<f64>, Option<f64>, Option<f64>, Option<f64>), rusqlite::Error> =
        conn.query_row(
            "SELECT gsc_clicks, gsc_impressions, gsc_ctr, gsc_position
             FROM live_site_pages
             WHERE project_id = ?1 AND path = ?2",
            rusqlite::params![project_id, path],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            },
        );

    match row_result {
        Ok((Some(clicks), Some(impressions), Some(ctr), Some(position))) => {
            (clicks, impressions, ctr, position)
        }
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}
