//! Deterministic content outcome review (issue #23).
//!
//! Compares pre/post GSC daily snapshot windows for an article after a
//! `write_article`, `fix_content_article`, or `consolidate_cluster` task
//! succeeded, and classifies the outcome as
//! improved/regressed/neutral/insufficient_data.
//!
//! This step is deliberately DETERMINISTIC — no LLM call. The classification
//! is a computable mapping from structured snapshot rows (sums over fixed
//! date windows + fixed thresholds) to a label; there is no intent to weigh
//! and no prose to generate. Modelled on
//! `engine/exec/gsc/recovery/outcome_review.rs`.

use rusqlite::Connection;

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Length of the compared windows (days). Matches the +30d review delay:
/// the recent window is ~4 weeks of post-change data.
const WINDOW_DAYS: i64 = 28;

/// Minimum days of snapshot data required in the recent window before any
/// classification is made. Below this the data is too thin to judge.
const MIN_RECENT_DAYS: i64 = 7;

/// Relative change threshold (±20%) for improved/regressed classification.
const CHANGE_THRESHOLD: f64 = 0.20;

/// Window totals used by the classifier. `clicks`/`impressions` are sums over
/// the window; `position` is the impressions-weighted average.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct OutcomeWindow {
    pub clicks: f64,
    pub impressions: f64,
    pub position: f64,
}

/// Pure classification of a baseline vs. recent window.
///
/// Rules (fixed thresholds, no judgment):
/// - baseline has no traffic at all: any recent traffic → `improved`
///   (the new-article case), no recent traffic → `insufficient_data`.
/// - clicks up >20% → `improved`; clicks down >20% → `regressed`.
/// - clicks flat but impressions moved >20% in a direction → that direction.
/// - otherwise → `neutral`.
pub fn classify_outcome(baseline: &OutcomeWindow, recent: &OutcomeWindow) -> &'static str {
    let baseline_has_traffic = baseline.clicks > 0.0 || baseline.impressions > 0.0;
    let recent_has_traffic = recent.clicks > 0.0 || recent.impressions > 0.0;

    if !baseline_has_traffic {
        return if recent_has_traffic {
            "improved"
        } else {
            "insufficient_data"
        };
    }

    let clicks_ratio = if baseline.clicks > 0.0 {
        Some(recent.clicks / baseline.clicks)
    } else {
        // Baseline had impressions but no clicks.
        if recent.clicks > 0.0 {
            return "improved";
        }
        None
    };
    let impr_ratio = if baseline.impressions > 0.0 {
        Some(recent.impressions / baseline.impressions)
    } else {
        None
    };

    if let Some(r) = clicks_ratio {
        if r > 1.0 + CHANGE_THRESHOLD {
            return "improved";
        }
        if r < 1.0 - CHANGE_THRESHOLD {
            return "regressed";
        }
    }
    if let Some(r) = impr_ratio {
        if r > 1.0 + CHANGE_THRESHOLD {
            return "improved";
        }
        if r < 1.0 - CHANGE_THRESHOLD {
            return "regressed";
        }
    }
    "neutral"
}

/// True when a GSC page URL belongs to the given article slug.
///
/// Both sides go through the canonical helpers in `content::slug`: the page
/// URL is reduced to its normalized final path segment (`/blog/02_foo` →
/// `foo`, numeric prefixes stripped) and compared against the normalized
/// slug, mirroring the article↔GSC matching in `exec/gsc/sync.rs`.
pub fn page_matches_slug(page_url: &str, slug: &str) -> bool {
    let normalized_slug = crate::content::slug::normalize_url_slug(slug);
    if normalized_slug.is_empty() {
        return false;
    }
    crate::content::slug::extract_slug_from_url(page_url) == normalized_slug
}

/// Conversion evidence status embedded in baseline/recent JSON (issue #308).
///
/// Evidence only — does **not** change GSC-led classification thresholds.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ConversionEvidence {
    /// `"ok"` | `"insufficient_data"` | `"missing"` | `"error"` (DB failure; not absent tape)
    pub status: &'static str,
    pub days_with_data: i64,
    pub events: std::collections::HashMap<String, f64>,
}

impl ConversionEvidence {
    pub fn missing() -> Self {
        Self {
            status: "missing",
            days_with_data: 0,
            events: std::collections::HashMap::new(),
        }
    }

    /// DB failure — distinct from absent tape (`missing`) so operators can tell them apart (#319).
    pub fn error() -> Self {
        Self {
            status: "error",
            days_with_data: 0,
            events: std::collections::HashMap::new(),
        }
    }

    pub fn from_window(days_with_data: i64, events: std::collections::HashMap<String, f64>) -> Self {
        let total: f64 = events.values().sum();
        let status = if days_with_data == 0 || (total == 0.0 && days_with_data < MIN_RECENT_DAYS) {
            if days_with_data == 0 {
                "missing"
            } else {
                "insufficient_data"
            }
        } else if days_with_data < MIN_RECENT_DAYS {
            "insufficient_data"
        } else {
            "ok"
        };
        Self {
            status,
            days_with_data,
            events,
        }
    }
}

/// Aggregate PostHog conversion tape for pages matching `slug` over a window.
///
/// Uses the same `page_matches_slug` helper so pathnames like `/blog/foo`
/// align with GSC page matching.
pub fn conversion_window_for_slug(
    conn: &Connection,
    project_id: &str,
    slug: &str,
    start_date: &str,
    end_date: &str,
) -> ConversionEvidence {
    let pages = match crate::posthog::db::list_pages(conn, project_id) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "[content_outcome] list_pages failed for project {project_id}: {e}"
            );
            return ConversionEvidence::error();
        }
    };
    let matching: Vec<String> = pages
        .into_iter()
        .filter(|p| page_matches_slug(p, slug))
        .collect();
    if matching.is_empty() {
        return ConversionEvidence::missing();
    }
    match crate::posthog::db::window_event_totals(
        conn,
        project_id,
        &matching,
        start_date,
        end_date,
    ) {
        Ok((days, events)) => ConversionEvidence::from_window(days, events),
        Err(e) => {
            log::warn!(
                "[content_outcome] window_event_totals failed for project {project_id} slug {slug}: {e}"
            );
            ConversionEvidence::error()
        }
    }
}

/// Compare pre/post snapshot windows for a review task's article and persist
/// the classification. Output contract: JSON report with slug, page, window
/// metrics, classification — persisted as the task's
/// `content_outcome_compare` artifact and inserted into
/// `content_outcome_results` for queryable outcome history.
pub(crate) fn exec_content_outcome_compare(
    task: &Task,
    _project_path: &str,
    conn: &Connection,
) -> StepResult {
    // 1. Read the spawn-time context artifact.
    let target = task
        .artifacts
        .iter()
        .find(|a| a.key == "content_outcome_target")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());

    let target = match target {
        Some(t) => t,
        None => {
            return StepResult::fail("No content_outcome_target artifact on review task".to_string())
        }
    };

    let slug = target["slug"].as_str().unwrap_or("").to_string();
    if slug.is_empty() {
        return StepResult::fail("content_outcome_target artifact has no slug".to_string());
    }
    let parent_task_type = target["parent_task_type"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let parent_task_id = target["parent_task_id"].as_str().unwrap_or("").to_string();

    // 2. Resolve the GSC page for this slug from the snapshot table.
    let pages = crate::db::list_gsc_page_daily_pages(conn, &task.project_id).unwrap_or_default();
    let page = pages.iter().find(|p| page_matches_slug(p, &slug)).cloned();

    // 3. Compute windows. Baseline window ends at the anchor date (parent
    // completion ≈ spawn time); recent window ends yesterday.
    let anchor = target["anchor_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.date_naive())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let baseline_end = anchor - chrono::Duration::days(1);
    let baseline_start = baseline_end - chrono::Duration::days(WINDOW_DAYS - 1);
    let recent_end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let recent_start = recent_end - chrono::Duration::days(WINDOW_DAYS - 1);

    let (baseline_window, recent_window, recent_days) = match &page {
        Some(p) => {
            let b = crate::db::gsc_page_daily_window_metrics(
                conn,
                &task.project_id,
                p,
                &baseline_start.format("%Y-%m-%d").to_string(),
                &baseline_end.format("%Y-%m-%d").to_string(),
            )
            .ok()
            .flatten();
            let r = crate::db::gsc_page_daily_window_metrics(
                conn,
                &task.project_id,
                p,
                &recent_start.format("%Y-%m-%d").to_string(),
                &recent_end.format("%Y-%m-%d").to_string(),
            )
            .ok()
            .flatten();
            let days = r.map(|m| m.days_with_data).unwrap_or(0);
            (b, r, days)
        }
        None => (None, None, 0),
    };

    // 4. Classify. Fall back to the spawn-time baseline artifact when the
    // snapshot table has no pre-window rows yet (snapshots only started
    // accumulating with issue #23).
    let artifact_baseline = OutcomeWindow {
        clicks: target["baseline"]["clicks"].as_f64().unwrap_or(0.0),
        impressions: target["baseline"]["impressions"].as_f64().unwrap_or(0.0),
        position: target["baseline"]["position"].as_f64().unwrap_or(0.0),
    };

    let classification = if page.is_none() || recent_days < MIN_RECENT_DAYS {
        "insufficient_data"
    } else {
        let baseline = baseline_window
            .map(|m| OutcomeWindow {
                clicks: m.clicks,
                impressions: m.impressions,
                position: m.position,
            })
            .unwrap_or(artifact_baseline);
        let recent = recent_window
            .map(|m| OutcomeWindow {
                clicks: m.clicks,
                impressions: m.impressions,
                position: m.position,
            })
            .unwrap_or(OutcomeWindow {
                clicks: 0.0,
                impressions: 0.0,
                position: 0.0,
            });
        classify_outcome(&baseline, &recent)
    };

    // Conversion tape evidence (issue #308) — second dimension only; does not
    // alter GSC-led classification above.
    let baseline_start_s = baseline_start.format("%Y-%m-%d").to_string();
    let baseline_end_s = baseline_end.format("%Y-%m-%d").to_string();
    let recent_start_s = recent_start.format("%Y-%m-%d").to_string();
    let recent_end_s = recent_end.format("%Y-%m-%d").to_string();
    let conversion_baseline = conversion_window_for_slug(
        conn,
        &task.project_id,
        &slug,
        &baseline_start_s,
        &baseline_end_s,
    );
    let conversion_recent = conversion_window_for_slug(
        conn,
        &task.project_id,
        &slug,
        &recent_start_s,
        &recent_end_s,
    );

    let reviewed_at = chrono::Utc::now().to_rfc3339();
    let baseline_json = serde_json::json!({
        "window_start": baseline_start_s,
        "window_end": baseline_end_s,
        "snapshot": baseline_window.map(|m| serde_json::json!({
            "days_with_data": m.days_with_data,
            "clicks": m.clicks,
            "impressions": m.impressions,
            "position": m.position,
        })),
        "spawn_artifact": artifact_baseline,
        "conversion": conversion_baseline,
    });
    let recent_json = serde_json::json!({
        "window_start": recent_start_s,
        "window_end": recent_end_s,
        "days_with_data": recent_days,
        "snapshot": recent_window.map(|m| serde_json::json!({
            "days_with_data": m.days_with_data,
            "clicks": m.clicks,
            "impressions": m.impressions,
            "position": m.position,
        })),
        "conversion": conversion_recent,
    });

    // 5. Persist to the queryable outcome history table. Best-effort: the
    // report artifact below is the primary output.
    let result_row = crate::db::ContentOutcomeResult {
        project_id: task.project_id.clone(),
        slug: slug.clone(),
        parent_task_type: parent_task_type.clone(),
        parent_task_id: parent_task_id.clone(),
        classification: classification.to_string(),
        baseline_json: baseline_json.to_string(),
        recent_json: recent_json.to_string(),
        reviewed_at: reviewed_at.clone(),
    };
    if let Err(e) = crate::db::insert_content_outcome_result(conn, &result_row) {
        log::warn!(
            "[content_outcome] failed to persist outcome result for {}: {}",
            slug,
            e
        );
    }

    let report = serde_json::json!({
        "slug": slug,
        "page": page,
        "parent_task_type": parent_task_type,
        "parent_task_id": parent_task_id,
        "classification": classification,
        "baseline_window": baseline_json,
        "recent_window": recent_json,
        "reviewed_at": reviewed_at,
        "note": "Outcome history for this slug is persisted in content_outcome_results \
                 and attached to this task as the content_outcome_compare artifact. \
                 Desk aggregates appear on site-overview.outcomes (content_* counts); \
                 use that rollup for closed-loop measurement, not research prompts.",
    });

    StepResult {
        success: true,
        message: format!("Content outcome for {}: {}", slug, classification),
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
        artifact_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskReviewSurface, TaskRun,
        TaskRunPolicy, TaskStatus,
    };

    fn window(clicks: f64, impressions: f64) -> OutcomeWindow {
        OutcomeWindow {
            clicks,
            impressions,
            position: 10.0,
        }
    }

    #[test]
    fn classify_new_article_earning_traffic_is_improved() {
        // Brand-new article: zero baseline, any recent traffic → improved.
        assert_eq!(
            classify_outcome(&window(0.0, 0.0), &window(5.0, 200.0)),
            "improved"
        );
        assert_eq!(
            classify_outcome(&window(0.0, 0.0), &window(0.0, 50.0)),
            "improved"
        );
    }

    #[test]
    fn classify_no_traffic_either_side_is_insufficient_data() {
        assert_eq!(
            classify_outcome(&window(0.0, 0.0), &window(0.0, 0.0)),
            "insufficient_data"
        );
    }

    #[test]
    fn classify_clicks_thresholds() {
        // +50% clicks → improved; -50% → regressed; +5% → neutral.
        assert_eq!(
            classify_outcome(&window(10.0, 100.0), &window(15.0, 100.0)),
            "improved"
        );
        assert_eq!(
            classify_outcome(&window(10.0, 100.0), &window(5.0, 100.0)),
            "regressed"
        );
        assert_eq!(
            classify_outcome(&window(10.0, 100.0), &window(10.5, 100.0)),
            "neutral"
        );
    }

    #[test]
    fn classify_impressions_movement_with_flat_clicks() {
        // Clicks flat but impressions up >20% → improved; down >20% → regressed.
        assert_eq!(
            classify_outcome(&window(10.0, 100.0), &window(10.0, 130.0)),
            "improved"
        );
        assert_eq!(
            classify_outcome(&window(10.0, 100.0), &window(10.0, 70.0)),
            "regressed"
        );
    }

    #[test]
    fn classify_first_click_from_impressions_only_baseline() {
        // Baseline had impressions but no clicks; first clicks → improved.
        assert_eq!(
            classify_outcome(&window(0.0, 500.0), &window(2.0, 500.0)),
            "improved"
        );
    }

    #[test]
    fn page_matches_slug_variants() {
        assert!(page_matches_slug("https://example.com/foo", "foo"));
        assert!(page_matches_slug("https://example.com/blog/foo/", "foo"));
        assert!(page_matches_slug("https://example.com/blog/02_foo", "foo"));
        assert!(page_matches_slug("https://example.com/blog/Foo_Bar", "foo-bar"));
        assert!(!page_matches_slug("https://example.com/blog/foobar", "foo"));
        assert!(!page_matches_slug("https://example.com/blog/foo", ""));
        // PostHog pathnames are often bare paths without host.
        assert!(page_matches_slug("/blog/foo", "foo"));
        assert!(page_matches_slug("/foo", "foo"));
    }

    #[test]
    fn conversion_evidence_missing_when_no_tape() {
        let conn = in_memory_db();
        let ev = conversion_window_for_slug(&conn, "proj1", "foo", "2026-01-01", "2026-01-31");
        assert_eq!(ev.status, "missing");
        assert_eq!(ev.days_with_data, 0);
    }

    #[test]
    fn conversion_evidence_insufficient_when_thin() {
        let conn = in_memory_db();
        crate::posthog::db::insert_rows(
            &conn,
            "proj1",
            &[crate::posthog::models::PosthogPageDailyRow {
                page: "/blog/foo".into(),
                event: "signup_started".into(),
                date: "2026-07-01".into(),
                count: 2.0,
            }],
        )
        .unwrap();
        let ev = conversion_window_for_slug(&conn, "proj1", "foo", "2026-07-01", "2026-07-31");
        assert_eq!(ev.status, "insufficient_data");
        assert_eq!(ev.days_with_data, 1);
        assert_eq!(ev.events.get("signup_started").copied().unwrap_or(0.0), 2.0);
    }

    #[test]
    fn conversion_evidence_ok_with_enough_days() {
        let conn = in_memory_db();
        let mut rows = Vec::new();
        for i in 0..10i64 {
            let d = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap() + chrono::Duration::days(i);
            rows.push(crate::posthog::models::PosthogPageDailyRow {
                page: "/blog/foo".into(),
                event: "signup_started".into(),
                date: d.format("%Y-%m-%d").to_string(),
                count: 1.0,
            });
        }
        crate::posthog::db::insert_rows(&conn, "proj1", &rows).unwrap();
        let ev = conversion_window_for_slug(&conn, "proj1", "foo", "2026-07-01", "2026-07-31");
        assert_eq!(ev.status, "ok");
        assert_eq!(ev.days_with_data, 10);
        assert_eq!(ev.events.get("signup_started").copied().unwrap_or(0.0), 10.0);
    }

    #[test]
    fn conversion_does_not_change_gsc_classification() {
        // Even with conversion tape present, classification remains GSC-only.
        let conn = in_memory_db();
        let page = "https://example.com/blog/foo";
        let anchor = chrono::Utc::now().date_naive() - chrono::Duration::days(30);
        for i in 1..=28i64 {
            let d = anchor - chrono::Duration::days(i);
            insert_snapshot(&conn, page, &d.format("%Y-%m-%d").to_string(), 1.0, 10.0);
        }
        for i in 1..=28i64 {
            let d = chrono::Utc::now().date_naive() - chrono::Duration::days(i);
            insert_snapshot(&conn, page, &d.format("%Y-%m-%d").to_string(), 2.0, 20.0);
            crate::posthog::db::insert_rows(
                &conn,
                "proj1",
                &[crate::posthog::models::PosthogPageDailyRow {
                    page: "/blog/foo".into(),
                    event: "signup_started".into(),
                    date: d.format("%Y-%m-%d").to_string(),
                    count: 0.0, // zero conversions — must not veto improved
                }],
            )
            .unwrap();
        }
        let task = make_review_task(
            "proj1",
            serde_json::json!({
                "slug": "foo",
                "parent_task_type": "fix_content_article",
                "parent_task_id": "parent-1",
                "anchor_date": anchor.format("%Y-%m-%dT00:00:00Z").to_string(),
                "baseline": {"clicks": 28.0, "impressions": 280.0, "position": 10.0},
            }),
        );
        let result = exec_content_outcome_compare(&task, "/tmp/test", &conn);
        assert!(result.success, "step failed: {}", result.message);
        let report: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(report["classification"], "improved");
        assert!(report["baseline_window"]["conversion"].is_object());
        assert!(report["recent_window"]["conversion"].is_object());
    }

    fn make_review_task(project_id: &str, target: serde_json::Value) -> Task {
        Task {
            id: "review-1".to_string(),
            task_type: "content_outcome_review".to_string(),
            phase: "verification".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::ArtifactReview,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: None,
            description: None,
            project_id: project_id.to_string(),
            depends_on: vec![],
            artifacts: vec![TaskArtifact {
                key: "content_outcome_target".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("post_actions".to_string()),
                content: Some(target.to_string()),
            }],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES ('proj1', 'Test', '/tmp/test')",
            [],
        )
        .unwrap();
        conn
    }

    fn insert_snapshot(conn: &Connection, page: &str, date: &str, clicks: f64, impressions: f64) {
        crate::db::insert_gsc_page_daily_snapshots(
            conn,
            "proj1",
            &[crate::models::gsc::PageDailyMetrics {
                page: page.to_string(),
                date: date.to_string(),
                clicks,
                impressions,
                ctr: 0.0,
                position: 10.0,
            }],
        )
        .unwrap();
    }

    #[test]
    fn exec_classifies_improved_from_snapshot_windows() {
        let conn = in_memory_db();
        let page = "https://example.com/blog/foo";
        let anchor = chrono::Utc::now().date_naive() - chrono::Duration::days(30);

        // Baseline window: 28 days ending the day before the anchor.
        for i in 1..=28i64 {
            let d = anchor - chrono::Duration::days(i);
            insert_snapshot(&conn, page, &d.format("%Y-%m-%d").to_string(), 1.0, 10.0);
        }
        // Recent window: 28 days ending yesterday — double the traffic.
        for i in 1..=28i64 {
            let d = chrono::Utc::now().date_naive() - chrono::Duration::days(i);
            insert_snapshot(&conn, page, &d.format("%Y-%m-%d").to_string(), 2.0, 20.0);
        }

        let task = make_review_task(
            "proj1",
            serde_json::json!({
                "slug": "foo",
                "parent_task_type": "fix_content_article",
                "parent_task_id": "parent-1",
                "anchor_date": anchor.format("%Y-%m-%dT00:00:00Z").to_string(),
                "baseline": {"clicks": 28.0, "impressions": 280.0, "position": 10.0},
            }),
        );

        let result = exec_content_outcome_compare(&task, "/tmp/test", &conn);
        assert!(result.success, "step failed: {}", result.message);
        let report: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(report["classification"], "improved");
        assert_eq!(report["page"], page);

        // Persisted to the queryable history table.
        let rows = crate::db::list_content_outcome_results(&conn, "proj1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].classification, "improved");
        assert_eq!(rows[0].slug, "foo");
        assert_eq!(rows[0].parent_task_type, "fix_content_article");
    }

    #[test]
    fn exec_reports_insufficient_data_without_snapshots() {
        let conn = in_memory_db();
        let task = make_review_task(
            "proj1",
            serde_json::json!({
                "slug": "brand-new-article",
                "parent_task_type": "write_article",
                "parent_task_id": "parent-2",
                "anchor_date": chrono::Utc::now().to_rfc3339(),
                "baseline": {"clicks": 0.0, "impressions": 0.0, "position": 0.0},
            }),
        );

        let result = exec_content_outcome_compare(&task, "/tmp/test", &conn);
        assert!(result.success);
        let report: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(report["classification"], "insufficient_data");

        let rows = crate::db::list_content_outcome_results(&conn, "proj1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].classification, "insufficient_data");
    }

    #[test]
    fn exec_fails_without_target_artifact() {
        let conn = in_memory_db();
        let mut task = make_review_task("proj1", serde_json::json!({}));
        task.artifacts.clear();

        let result = exec_content_outcome_compare(&task, "/tmp/test", &conn);
        assert!(!result.success);
    }
}
