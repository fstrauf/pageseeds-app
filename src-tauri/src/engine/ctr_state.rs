/// Durable CTR issue state — the source of truth for "what still needs fixing".
///
/// Each row represents one CTR issue (title, meta, snippet, FAQ) for one article.
/// Status lifecycle: open → recommended → queued → applied → verified | failed | skipped
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CtrIssueStatus {
    Open,
    Recommended,
    Queued,
    Applied,
    Verified,
    Failed,
    Skipped,
    ManualReview,
}

impl CtrIssueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CtrIssueStatus::Open => "open",
            CtrIssueStatus::Recommended => "recommended",
            CtrIssueStatus::Queued => "queued",
            CtrIssueStatus::Applied => "applied",
            CtrIssueStatus::Verified => "verified",
            CtrIssueStatus::Failed => "failed",
            CtrIssueStatus::Skipped => "skipped",
            CtrIssueStatus::ManualReview => "manual_review",
        }
    }
}

impl std::str::FromStr for CtrIssueStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "open" => Ok(CtrIssueStatus::Open),
            "recommended" => Ok(CtrIssueStatus::Recommended),
            "queued" => Ok(CtrIssueStatus::Queued),
            "applied" => Ok(CtrIssueStatus::Applied),
            "verified" => Ok(CtrIssueStatus::Verified),
            "failed" => Ok(CtrIssueStatus::Failed),
            "skipped" => Ok(CtrIssueStatus::Skipped),
            "manual_review" => Ok(CtrIssueStatus::ManualReview),
            _ => Err(format!("Unknown CtrIssueStatus: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrIssueRecord {
    pub project_id: String,
    pub article_id: i64,
    pub issue_type: String,
    pub status: String,
    pub detected_at: String,
    pub last_verified_at: Option<String>,
    pub content_hash_at_detection: String,
    pub fix_task_id: Option<String>,
    pub failure_reason: Option<String>,
    pub verified_hash: Option<String>,
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

/// Upsert a CTR issue. If the row already exists, update the status and metadata.
pub fn upsert_ctr_issue(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    issue_type: &str,
    status: CtrIssueStatus,
    content_hash_at_detection: &str,
    fix_task_id: Option<&str>,
    failure_reason: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO article_ctr_issues (
            project_id, article_id, issue_type, status, detected_at,
            content_hash_at_detection, fix_task_id, failure_reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(project_id, article_id, issue_type) DO UPDATE SET
            status = excluded.status,
            detected_at = excluded.detected_at,
            content_hash_at_detection = excluded.content_hash_at_detection,
            fix_task_id = excluded.fix_task_id,
            failure_reason = excluded.failure_reason",
        rusqlite::params![
            project_id,
            article_id,
            issue_type,
            status.as_str(),
            now,
            content_hash_at_detection,
            fix_task_id,
            failure_reason,
        ],
    )?;
    Ok(())
}

/// Mark an issue as verified (fixed) and record the content hash at verification.
pub fn mark_ctr_issue_verified(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    issue_type: &str,
    verified_hash: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE article_ctr_issues
         SET status = 'verified', last_verified_at = ?1, verified_hash = ?2
         WHERE project_id = ?3 AND article_id = ?4 AND issue_type = ?5",
        rusqlite::params![now, verified_hash, project_id, article_id, issue_type],
    )?;
    if rows == 0 {
        return Err(Error::Other(format!(
            "CTR issue not found for verification: {} {} {}",
            project_id, article_id, issue_type
        )));
    }
    Ok(())
}

/// Mark an issue as failed with a reason.
pub fn mark_ctr_issue_failed(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    issue_type: &str,
    reason: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE article_ctr_issues
         SET status = 'failed', last_verified_at = ?1, failure_reason = ?2
         WHERE project_id = ?3 AND article_id = ?4 AND issue_type = ?5",
        rusqlite::params![now, reason, project_id, article_id, issue_type],
    )?;
    if rows == 0 {
        return Err(Error::Other(format!(
            "CTR issue not found for failure: {} {} {}",
            project_id, article_id, issue_type
        )));
    }
    Ok(())
}

/// Get all open CTR issues for a project.
pub fn get_open_ctr_issues(conn: &Connection, project_id: &str) -> Result<Vec<CtrIssueRecord>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, article_id, issue_type, status, detected_at,
                last_verified_at, content_hash_at_detection, fix_task_id,
                failure_reason, verified_hash
         FROM article_ctr_issues
         WHERE project_id = ?1 AND status IN ('open', 'recommended', 'queued', 'applied', 'failed')"
    )?;
    let rows = stmt.query_map([project_id], row_to_ctr_issue)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Other(format!("Failed to load open CTR issues: {}", e)))
}

/// Get all CTR issues for a specific article.
pub fn get_ctr_issues_for_article(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
) -> Result<Vec<CtrIssueRecord>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, article_id, issue_type, status, detected_at,
                last_verified_at, content_hash_at_detection, fix_task_id,
                failure_reason, verified_hash
         FROM article_ctr_issues
         WHERE project_id = ?1 AND article_id = ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id, article_id], row_to_ctr_issue)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Other(format!("Failed to load CTR issues for article: {}", e)))
}

/// Delete all CTR issues for a project (dangerous — used for reset).
pub fn clear_ctr_issues(conn: &Connection, project_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM article_ctr_issues WHERE project_id = ?1",
        [project_id],
    )?;
    Ok(())
}

/// Count open issues per project.
pub fn count_open_ctr_issues(conn: &Connection, project_id: &str) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM article_ctr_issues
         WHERE project_id = ?1 AND status IN ('open', 'recommended', 'queued', 'applied', 'failed')",
        [project_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_ctr_issue(row: &rusqlite::Row) -> rusqlite::Result<CtrIssueRecord> {
    Ok(CtrIssueRecord {
        project_id: row.get(0)?,
        article_id: row.get(1)?,
        issue_type: row.get(2)?,
        status: row.get(3)?,
        detected_at: row.get(4)?,
        last_verified_at: row.get(5)?,
        content_hash_at_detection: row.get(6)?,
        fix_task_id: row.get(7)?,
        failure_reason: row.get(8)?,
        verified_hash: row.get(9)?,
    })
}
