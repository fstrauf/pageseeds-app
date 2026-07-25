use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::models::task::Task;

// ─── Project overview stats ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusCounts {
    pub todo: i64,
    pub in_progress: i64,
    pub review: i64,
    pub done: i64,
    pub failed: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentTask {
    pub id: String,
    pub title: Option<String>,
    pub task_type: String,
    pub status: String,
    pub updated_at: String,
}

/// Landing page research task awaiting review with user context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingPageResearchPending {
    pub id: String,
    pub title: Option<String>,
    /// User-provided strategy context (parsed from description JSON)
    pub context: String,
    /// Themes being researched (parsed from description JSON or auto-derived)
    pub themes: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleStatusCounts {
    pub total: i64,
    pub published: i64,
    pub draft: i64,
    pub last_published_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowActivity {
    pub task_type: String,
    pub label: String,
    pub last_run_at: Option<String>,
    pub next_due_at: Option<String>,
    pub interval_hours: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectOverview {
    pub tasks: TaskStatusCounts,
    pub recent_tasks: Vec<RecentTask>,
    pub articles: ArticleStatusCounts,
    pub ready_task_count: i64,
    pub workflow_activity: Vec<WorkflowActivity>,
    /// Landing page research tasks in 'review' status awaiting user selection
    pub pending_landing_page_research: Vec<LandingPageResearchPending>,
    /// Feature spec tasks in 'review' status awaiting user confirmation
    pub pending_feature_specs: Vec<PendingFeatureSpec>,
    /// Fix tasks completed / failed since the most recent audit
    pub fix_summary: FixSummary,
    /// Comprehensive health snapshot: what still needs attention across all audits
    pub health_snapshot: crate::db::content_audit::HealthSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSummary {
    pub completed: i64,
    pub failed: i64,
    pub pending: i64,
    /// Total articles with issues found in the most recent content audit
    /// (needs_improvement + poor). 0 if no audit exists.
    pub total_found: i64,
}

/// Comprehensive health snapshot showing outstanding issues across all audit types.
/// Replaces FixSummary as the primary health indicator in Overview.
pub use crate::db::content_audit::HealthSnapshot;

/// A generate_feature_spec task waiting for user confirmation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingFeatureSpec {
    pub id: String,
    pub title: Option<String>,
    pub updated_at: String,
}

pub fn get_project_overview(conn: &Connection, project_id: &str) -> Result<ProjectOverview> {
    // Task counts by status
    let counts: TaskStatusCounts = {
        let mut stmt = conn.prepare(
            "SELECT
               SUM(CASE WHEN status = 'todo' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'review' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END),
               COUNT(*)
             FROM tasks WHERE project_id = ?1",
        )?;
        stmt.query_row([project_id], |row| {
            Ok(TaskStatusCounts {
                todo: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                in_progress: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                review: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                done: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                failed: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                total: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            })
        })?
    };

    // 8 most recently updated tasks
    let recent_tasks: Vec<RecentTask> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, type, status, updated_at
             FROM tasks WHERE project_id = ?1
             ORDER BY updated_at DESC LIMIT 8",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok(RecentTask {
                id: row.get(0)?,
                title: row.get(1)?,
                task_type: row.get(2)?,
                status: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Todo tasks with no unresolved depends_on
    let ready_task_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND status = 'todo' AND depends_on = '[]'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Article stats
    let articles = {
        let mut stmt = conn.prepare(
            "SELECT
               COUNT(*),
               SUM(CASE WHEN status = 'published' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'draft' THEN 1 ELSE 0 END),
               MAX(CASE WHEN status = 'published' THEN published_date ELSE NULL END)
             FROM articles WHERE project_id = ?1",
        )?;
        stmt.query_row([project_id], |row| {
            Ok(ArticleStatusCounts {
                total: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                published: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                draft: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                last_published_date: row.get(3)?,
            })
        })?
    };

    // Workflow activity: last completed run per key workflow type + scheduler interval
    // Must stay in sync with QUICK_ACTIONS in src/components/overview/Overview.tsx
    let key_workflows: &[(&str, &str)] = &[
        ("collect_gsc", "GSC Collection"),
        ("research_keywords", "Keyword Research"),
        ("research_landing_pages", "Landing Page Research"),
        ("reddit_opportunity_search", "Reddit Search"),
        ("content_review", "Content Review"),
        ("cannibalization_audit", "Cannibalization Audit"),
        ("ctr_audit", "CTR Audit"),
        ("indexing_health_campaign", "Indexing Health Campaign"),
        ("content_cleanup", "Content Cleanup"),
        ("sanitize_content", "Sanitize Content"),
    ];

    // Build a map of task_type → last successful run finished_at from task_runs.
    // Using task_runs.finished_at is more accurate than tasks.updated_at,
    // which is also modified by edits, retries, and follow-up task creation.
    let mut last_run_map: std::collections::HashMap<String, String> = {
        let mut stmt = conn.prepare(
            "SELECT t.type, MAX(tr.finished_at)
             FROM tasks t
             JOIN task_runs tr ON t.id = tr.task_id
             WHERE t.project_id = ?1 AND tr.success = 1
             GROUP BY t.type",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Build a map of task_type → interval_hours from scheduler_rules
    let mut interval_map: std::collections::HashMap<String, i64> = {
        let mut stmt = conn.prepare(
            "SELECT task_type, interval_hours FROM scheduler_rules WHERE project_id = ?1",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let workflow_activity: Vec<WorkflowActivity> = key_workflows
        .iter()
        .map(|(task_type, label)| {
            let last_run_at = last_run_map.remove(*task_type);
            let interval_hours = interval_map.remove(*task_type);
            let next_due_at = last_run_at
                .as_ref()
                .zip(interval_hours)
                .and_then(|(ts, hrs)| {
                    ts.parse::<chrono::DateTime<chrono::Utc>>()
                        .ok()
                        .map(|dt| (dt + chrono::Duration::hours(hrs)).to_rfc3339())
                });
            WorkflowActivity {
                task_type: task_type.to_string(),
                label: label.to_string(),
                last_run_at,
                next_due_at,
                interval_hours,
            }
        })
        .collect();

    // Pending landing page research tasks in review status
    let pending_landing_page_research: Vec<LandingPageResearchPending> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, description, updated_at
             FROM tasks 
             WHERE project_id = ?1 AND type = 'research_landing_pages' AND status = 'review'
             ORDER BY updated_at DESC LIMIT 5",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            let description: Option<String> = row.get(2)?;
            let (context, themes) = parse_landing_page_description(description.as_deref());
            Ok(LandingPageResearchPending {
                id: row.get(0)?,
                title: row.get(1)?,
                context,
                themes,
                updated_at: row.get(3)?,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Pending feature spec tasks in review status — show only the most recent one
    // to avoid cluttering the Overview with old specs from before dedup fixes.
    let pending_feature_specs: Vec<PendingFeatureSpec> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, updated_at
             FROM tasks
             WHERE project_id = ?1 AND type = 'generate_feature_spec' AND status = 'review'
             ORDER BY updated_at DESC LIMIT 1",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok(PendingFeatureSpec {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Count fix tasks since the most recent audit run
    let fix_summary: FixSummary = {
        let last_audit_at: Option<String> = conn.query_row(
            "SELECT MAX(tr.finished_at)
             FROM tasks t
             JOIN task_runs tr ON t.id = tr.task_id
             WHERE t.project_id = ?1 AND t.type IN ('content_review', 'indexing_health_campaign', 'ctr_audit') AND tr.success = 1",
            [project_id],
            |row| row.get(0),
        ).ok().flatten();

        let since_clause = match &last_audit_at {
            Some(ts) => format!("AND updated_at > '{}'", ts),
            None => String::new(),
        };

        let sql = format!(
            "SELECT
               SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status IN ('todo', 'queued', 'in_progress') THEN 1 ELSE 0 END)
             FROM tasks
             WHERE project_id = ?1 AND type LIKE 'fix_%' {}",
            since_clause
        );

        let mut summary = conn.query_row(&sql, [project_id], |row| {
            Ok(FixSummary {
                completed: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                failed: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                pending: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                total_found: 0,
            })
        }).unwrap_or(FixSummary { completed: 0, failed: 0, pending: 0, total_found: 0 });

        // Read latest content audit from DB to get total articles with issues
        if let Ok(count) = crate::db::content_audit::count_unhealthy_articles(conn, project_id) {
            summary.total_found = count;
        }

        summary
    };

    let health_snapshot = crate::db::content_audit::get_health_snapshot(conn, project_id)
        .unwrap_or_default();

    log::info!(
        "[project_overview] tasks: total={} done={} todo={} in_progress={} review={} failed={} ready={}",
        counts.total, counts.done, counts.todo, counts.in_progress, counts.review, counts.failed,
        ready_task_count,
    );
    log::info!(
        "[project_overview] health_snapshot: poor={} needs={} good={} not_indexed={} ctr={} cann={} fix_done={} fix_failed={} fix_pending={} cooldown={} fix_review={}",
        health_snapshot.content_poor,
        health_snapshot.content_needs_improvement,
        health_snapshot.content_good,
        health_snapshot.indexing_not_indexed,
        health_snapshot.ctr_issue_count,
        health_snapshot.cannibalization_clusters,
        health_snapshot.fix_completed,
        health_snapshot.fix_failed,
        health_snapshot.fix_pending,
        health_snapshot.fix_on_cooldown,
        health_snapshot.fix_needs_review,
    );

    Ok(ProjectOverview {
        tasks: counts,
        recent_tasks,
        articles,
        ready_task_count,
        workflow_activity,
        pending_landing_page_research,
        pending_feature_specs,
        fix_summary,
        health_snapshot,
    })
}

/// Parse landing page research description JSON to extract context and themes.
///
/// The Overview landing-page dialog writes the task description as
/// `{"context": "...", "themes": ["...", ...]}`. Shared by the overview
/// projection, the research pipeline (user themes honored as seeds), and the
/// seed-extraction/validation prompts (context as a labeled section).
pub(crate) fn parse_landing_page_description(desc: Option<&str>) -> (String, Vec<String>) {
    let desc = desc.unwrap_or("");

    // Try JSON format first
    if desc.trim().starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(desc) {
            let context = parsed
                .get("context")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let themes = parsed
                .get("themes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            return (context, themes);
        }
    }

    // Fall back to treating entire description as context
    (desc.to_string(), vec![])
}

/// Unpack the landing-page strategy payload for a task, or `None` when the
/// task is not a `research_landing_pages` task. Single gate so call sites
/// don't each repeat the task-type check + description parse.
pub(crate) fn landing_page_strategy(task: &Task) -> Option<(String, Vec<String>)> {
    if task.task_type == "research_landing_pages" {
        Some(parse_landing_page_description(task.description.as_deref()))
    } else {
        None
    }
}
