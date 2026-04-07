/// SQLite persistence for GSC URL indexing status.
///
/// Tracks per-URL inspection history so `indexing_diagnostics` can:
///   - avoid re-checking recently-passed URLs
///   - detect regressions (was pass, now fail)
///   - detect recoveries (was fail, now pass)
///   - avoid duplicate fix-task spam

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlIndexingStatus {
    pub url: String,
    pub project_id: String,
    pub last_inspected_at: Option<String>,
    pub last_reason_code: Option<String>,
    pub last_verdict: Option<String>,
    pub last_action: Option<String>,
    pub consecutive_passes: i32,
    pub last_task_created_at: Option<String>,
    pub last_task_type: Option<String>,
    pub last_task_id: Option<String>,
    pub last_fix_summary: Option<String>,
    pub fix_attempt_count: i32,
    pub last_task_resolved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Get status for a single URL.
pub fn get_status(conn: &Connection, url: &str, project_id: &str) -> Result<Option<UrlIndexingStatus>> {
    let mut stmt = conn.prepare(
        "SELECT url, project_id, last_inspected_at, last_reason_code, last_verdict,
                last_action, consecutive_passes, last_task_created_at, last_task_type,
                last_task_id, last_fix_summary, fix_attempt_count, last_task_resolved_at,
                created_at, updated_at
         FROM gsc_url_indexing_status
         WHERE url = ?1 AND project_id = ?2"
    )?;

    let row = stmt.query_row([url, project_id], |r| {
        Ok(UrlIndexingStatus {
            url: r.get(0)?,
            project_id: r.get(1)?,
            last_inspected_at: r.get(2)?,
            last_reason_code: r.get(3)?,
            last_verdict: r.get(4)?,
            last_action: r.get(5)?,
            consecutive_passes: r.get(6)?,
            last_task_created_at: r.get(7)?,
            last_task_type: r.get(8)?,
            last_task_id: r.get(9)?,
            last_fix_summary: r.get(10)?,
            fix_attempt_count: r.get(11)?,
            last_task_resolved_at: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        })
    }).optional()?;

    Ok(row)
}

/// Get all statuses for a project.
pub fn list_by_project(conn: &Connection, project_id: &str) -> Result<Vec<UrlIndexingStatus>> {
    let mut stmt = conn.prepare(
        "SELECT url, project_id, last_inspected_at, last_reason_code, last_verdict,
                last_action, consecutive_passes, last_task_created_at, last_task_type,
                last_task_id, last_fix_summary, fix_attempt_count, last_task_resolved_at,
                created_at, updated_at
         FROM gsc_url_indexing_status
         WHERE project_id = ?1"
    )?;

    let rows = stmt.query_map([project_id], |r| {
        Ok(UrlIndexingStatus {
            url: r.get(0)?,
            project_id: r.get(1)?,
            last_inspected_at: r.get(2)?,
            last_reason_code: r.get(3)?,
            last_verdict: r.get(4)?,
            last_action: r.get(5)?,
            consecutive_passes: r.get(6)?,
            last_task_created_at: r.get(7)?,
            last_task_type: r.get(8)?,
            last_task_id: r.get(9)?,
            last_fix_summary: r.get(10)?,
            fix_attempt_count: r.get(11)?,
            last_task_resolved_at: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Insert or update status for a URL.
/// Note: last_fix_summary, fix_attempt_count, and last_task_resolved_at are 
/// managed separately by the fix task system and are NOT updated by this function.
pub fn upsert_status(conn: &Connection, status: &UrlIndexingStatus) -> Result<()> {
    conn.execute(
        "INSERT INTO gsc_url_indexing_status (
            url, project_id, last_inspected_at, last_reason_code, last_verdict,
            last_action, consecutive_passes, last_task_created_at, last_task_type,
            last_task_id, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(url, project_id) DO UPDATE SET
            last_inspected_at = excluded.last_inspected_at,
            last_reason_code = excluded.last_reason_code,
            last_verdict = excluded.last_verdict,
            last_action = excluded.last_action,
            consecutive_passes = excluded.consecutive_passes,
            last_task_created_at = excluded.last_task_created_at,
            last_task_type = excluded.last_task_type,
            last_task_id = excluded.last_task_id,
            updated_at = excluded.updated_at",
        rusqlite::params![
            status.url,
            status.project_id,
            status.last_inspected_at,
            status.last_reason_code,
            status.last_verdict,
            status.last_action,
            status.consecutive_passes,
            status.last_task_created_at,
            status.last_task_type,
            status.last_task_id,
            status.created_at,
            status.updated_at,
        ],
    )?;
    Ok(())
}

/// Record that a fix task was created for a URL.
pub fn record_task_created(
    conn: &Connection,
    url: &str,
    project_id: &str,
    task_id: &str,
    task_type: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE gsc_url_indexing_status
         SET last_task_created_at = ?1,
             last_task_type = ?2,
             last_task_id = ?3,
             updated_at = ?1
         WHERE url = ?4 AND project_id = ?5",
        rusqlite::params![now, task_type, task_id, url, project_id],
    )?;
    Ok(())
}

/// Record that a fix task was completed for a URL, with the summary of changes.
pub fn record_fix_resolved(
    conn: &Connection,
    url: &str,
    project_id: &str,
    fix_summary: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE gsc_url_indexing_status
         SET last_task_resolved_at = ?1,
             last_fix_summary = ?2,
             fix_attempt_count = fix_attempt_count + 1,
             updated_at = ?1
         WHERE url = ?3 AND project_id = ?4",
        rusqlite::params![now, fix_summary, url, project_id],
    )?;
    Ok(())
}

/// Check whether an active fix task already exists for a given URL + reason.
///
/// Active means status is 'todo', 'in_progress', or 'review'.
pub fn has_active_fix_task(conn: &Connection, project_id: &str, url: &str, _reason: &str) -> Result<bool> {
    let pattern = format!("%{}%", url);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE project_id = ?1
           AND type IN ('fix_indexing', 'fix_technical', 'fix_content', 'fix_gsc_access')
           AND status IN ('todo', 'in_progress', 'review')
           AND description LIKE ?2",
        rusqlite::params![project_id, pattern],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}
