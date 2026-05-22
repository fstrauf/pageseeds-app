/// Content audit database storage.
///
/// Replaces the content_audit.json file with queryable SQLite tables.
use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

/// A single content audit run (one row per project per audit execution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentAuditRun {
    pub id: i64,
    pub project_id: String,
    pub run_at: String,
    pub total_audited: i64,
    pub good_count: i64,
    pub needs_improvement_count: i64,
    pub poor_count: i64,
    pub duplicate_groups_json: String,
}

/// A single article's audit result within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleContentAudit {
    pub run_id: i64,
    pub article_id: i64,
    pub article_file: String,
    pub title: String,
    pub url_slug: String,
    pub health: String,
    pub health_score: i64,
    pub priority_score: i64,
    /// Full JSON of the article audit result (checks, quality, readability, seo, etc.)
    pub data_json: String,
}

/// Save a new content audit run and its per-article results.
/// Returns the run_id of the newly created run.
pub fn save_audit_run(
    conn: &Connection,
    project_id: &str,
    run_at: &str,
    total_audited: i64,
    good_count: i64,
    needs_improvement_count: i64,
    poor_count: i64,
    duplicate_groups_json: &str,
    articles: Vec<ArticleContentAudit>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO content_audit_runs
         (project_id, run_at, total_audited, good_count, needs_improvement_count, poor_count, duplicate_groups_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            project_id,
            run_at,
            total_audited,
            good_count,
            needs_improvement_count,
            poor_count,
            duplicate_groups_json,
        ],
    )?;
    let run_id = conn.last_insert_rowid();

    let mut stmt = conn.prepare(
        "INSERT INTO article_content_audits
         (run_id, article_id, article_file, title, url_slug, health, health_score, priority_score, data_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for article in &articles {
        stmt.execute(rusqlite::params![
            run_id,
            article.article_id,
            &article.article_file,
            &article.title,
            &article.url_slug,
            &article.health,
            article.health_score,
            article.priority_score,
            &article.data_json,
        ])?;
    }
    drop(stmt);

    Ok(run_id)
}

/// Get the latest content audit run for a project.
pub fn get_latest_audit_run(conn: &Connection, project_id: &str) -> Result<Option<ContentAuditRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, run_at, total_audited, good_count, needs_improvement_count, poor_count, duplicate_groups_json
         FROM content_audit_runs
         WHERE project_id = ?1
         ORDER BY run_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query([project_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ContentAuditRun {
            id: row.get(0)?,
            project_id: row.get(1)?,
            run_at: row.get(2)?,
            total_audited: row.get(3)?,
            good_count: row.get(4)?,
            needs_improvement_count: row.get(5)?,
            poor_count: row.get(6)?,
            duplicate_groups_json: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

/// Get all article audits for a specific run.
pub fn get_articles_for_run(conn: &Connection, run_id: i64) -> Result<Vec<ArticleContentAudit>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, article_id, article_file, title, url_slug, health, health_score, priority_score, data_json
         FROM article_content_audits
         WHERE run_id = ?1
         ORDER BY priority_score DESC, article_id ASC",
    )?;
    let rows = stmt.query_map([run_id], |row| {
        Ok(ArticleContentAudit {
            run_id: row.get(0)?,
            article_id: row.get(1)?,
            article_file: row.get(2)?,
            title: row.get(3)?,
            url_slug: row.get(4)?,
            health: row.get(5)?,
            health_score: row.get(6)?,
            priority_score: row.get(7)?,
            data_json: row.get(8)?,
        })
    })?;
    rows.collect()
}

/// Get unhealthy articles (needs_improvement or poor) for the latest run.
pub fn get_unhealthy_articles(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<ArticleContentAudit>> {
    let mut stmt = conn.prepare(
        "SELECT a.run_id, a.article_id, a.article_file, a.title, a.url_slug, a.health, a.health_score, a.priority_score, a.data_json
         FROM article_content_audits a
         JOIN content_audit_runs r ON a.run_id = r.id
         WHERE r.project_id = ?1 AND a.health IN ('poor', 'needs_improvement')
           AND r.run_at = (SELECT MAX(run_at) FROM content_audit_runs WHERE project_id = ?1)
         ORDER BY a.priority_score DESC, a.article_id ASC",
    )?;
    let rows = stmt.query_map([project_id, project_id], |row| {
        Ok(ArticleContentAudit {
            run_id: row.get(0)?,
            article_id: row.get(1)?,
            article_file: row.get(2)?,
            title: row.get(3)?,
            url_slug: row.get(4)?,
            health: row.get(5)?,
            health_score: row.get(6)?,
            priority_score: row.get(7)?,
            data_json: row.get(8)?,
        })
    })?;
    rows.collect()
}

/// Get the full content audit report as a JSON value matching the old content_audit.json format.
/// This allows existing consumers to migrate gradually.
pub fn get_audit_report_as_json(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<serde_json::Value>> {
    let run = match get_latest_audit_run(conn, project_id)? {
        Some(r) => r,
        None => return Ok(None),
    };

    let articles = get_articles_for_run(conn, run.id)?;
    let articles_json: Vec<serde_json::Value> = articles
        .iter()
        .filter_map(|a| serde_json::from_str(&a.data_json).ok())
        .collect();

    let duplicate_groups: serde_json::Value =
        serde_json::from_str(&run.duplicate_groups_json).unwrap_or_else(|_| serde_json::json!([]));

    Ok(Some(serde_json::json!({
        "generated_at": run.run_at,
        "total_audited": run.total_audited,
        "health_summary": {
            "good": run.good_count,
            "needs_improvement": run.needs_improvement_count,
            "poor": run.poor_count,
        },
        "duplicate_groups": duplicate_groups,
        "duplicate_articles": duplicate_groups.as_array().map(|a| a.iter().map(|g| g["article_count"].as_u64().unwrap_or(0)).sum::<u64>()).unwrap_or(0),
        "articles": articles_json,
    })))
}

/// Count total articles with issues (needs_improvement + poor) for the latest run.
pub fn count_unhealthy_articles(conn: &Connection, project_id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM article_content_audits a
         JOIN content_audit_runs r ON a.run_id = r.id
         WHERE r.project_id = ?1 AND a.health IN ('poor', 'needs_improvement')
           AND r.run_at = (SELECT MAX(run_at) FROM content_audit_runs WHERE project_id = ?1)",
        [project_id, project_id],
        |row| row.get(0),
    )
}

/// Outstanding issue counts across all audit types for a project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub content_poor: i64,
    pub content_needs_improvement: i64,
    pub content_good: i64,
    pub indexing_not_indexed: i64,
    pub ctr_issue_count: i64,
    pub cannibalization_clusters: i64,
    pub fix_completed: i64,
    pub fix_failed: i64,
    pub fix_pending: i64,
    pub last_audit_days: i64,
}

/// Build a comprehensive health snapshot showing what still needs attention.
pub fn get_health_snapshot(conn: &Connection, project_id: &str) -> Result<HealthSnapshot> {
    let mut snap = HealthSnapshot::default();

    // Content audit counts from latest run
    if let Ok(Some(run)) = get_latest_audit_run(conn, project_id) {
        snap.content_good = run.good_count;
        snap.content_needs_improvement = run.needs_improvement_count;
        snap.content_poor = run.poor_count;
    }

    // Indexing: count not-indexed URLs from latest campaign plan
    if let Ok(Some(plan_json)) = get_latest_audit_artifact(conn, project_id, "indexing_campaign_plan") {
        if let Some(targets) = plan_json["targets"].as_array() {
            snap.indexing_not_indexed = targets
                .iter()
                .filter(|t| t["reason_code"].as_str().unwrap_or("").starts_with("not_indexed"))
                .count() as i64;
        }
    }

    // CTR: count articles with issues from latest context
    if let Ok(Some(ctr_json)) = get_latest_audit_artifact(conn, project_id, "ctr_audit_context") {
        if let Some(articles) = ctr_json["articles"].as_array() {
            snap.ctr_issue_count = articles
                .iter()
                .filter(|a| {
                    a["issues_detected"].as_array().map(|issues| !issues.is_empty()).unwrap_or(false)
                })
                .count() as i64;
        }
    }

    // Cannibalization: count clusters from latest clusters artifact
    if let Ok(Some(clusters_json)) = get_latest_audit_artifact(conn, project_id, "cannibalization_clusters") {
        if let Some(clusters) = clusters_json["clusters"].as_array() {
            snap.cannibalization_clusters = clusters.len() as i64;
        }
    }

    // Fix task progress since last audit run
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

    conn.query_row(&sql, [project_id], |row| {
        snap.fix_completed = row.get::<_, Option<i64>>(0)?.unwrap_or(0);
        snap.fix_failed = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
        snap.fix_pending = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
        Ok(())
    }).unwrap_or(());

    // Days since last audit
    snap.last_audit_days = conn.query_row(
        "SELECT COALESCE(MAX(run_at), '') FROM content_audit_runs WHERE project_id = ?1",
        [project_id],
        |row| {
            let run_at: String = row.get(0)?;
            if run_at.is_empty() {
                return Ok(-1i64);
            }
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&run_at) {
                Ok(chrono::Utc::now().signed_duration_since(dt).num_days())
            } else {
                Ok(-1i64)
            }
        },
    ).unwrap_or(-1);

    Ok(snap)
}

/// Delete all content audit data for a project (e.g. when project is deleted).
pub fn delete_project_audit_data(conn: &Connection, project_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM content_audit_runs WHERE project_id = ?1",
        [project_id],
    )?;
    conn.execute(
        "DELETE FROM audit_artifacts WHERE project_id = ?1",
        [project_id],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Generic Audit Artifact Storage (replaces per-type JSON files)
// ═══════════════════════════════════════════════════════════════════════════════

/// Save an audit artifact JSON to the database.
pub fn save_audit_artifact(
    conn: &Connection,
    project_id: &str,
    artifact_type: &str,
    created_at: &str,
    data_json: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO audit_artifacts (project_id, artifact_type, created_at, data_json)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![project_id, artifact_type, created_at, data_json],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get the latest audit artifact of a specific type for a project.
pub fn get_latest_audit_artifact(
    conn: &Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT data_json FROM audit_artifacts
         WHERE project_id = ?1 AND artifact_type = ?2
         ORDER BY created_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query([project_id, artifact_type])?;
    if let Some(row) = rows.next()? {
        let data: String = row.get(0)?;
        Ok(serde_json::from_str(&data).ok())
    } else {
        Ok(None)
    }
}

/// Check if an artifact of the given type exists and is fresher than max_age_hours.
pub fn is_artifact_fresh(
    conn: &Connection,
    project_id: &str,
    artifact_type: &str,
    max_age_hours: i64,
) -> Result<bool> {
    let row: Option<(String,)> = conn
        .query_row(
            "SELECT created_at FROM audit_artifacts
             WHERE project_id = ?1 AND artifact_type = ?2
             ORDER BY created_at DESC
             LIMIT 1",
            [project_id, artifact_type],
            |row| Ok((row.get::<_, String>(0)?,)),
        )
        .ok();

    if let Some((created_at,)) = row {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&created_at) {
            let hours_old = chrono::Utc::now().signed_duration_since(dt).num_hours();
            return Ok(hours_old < max_age_hours);
        }
    }
    Ok(false)
}
