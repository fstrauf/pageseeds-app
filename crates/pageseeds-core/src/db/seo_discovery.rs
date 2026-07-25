use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

/// Persisted SEO opportunity row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeoOpportunityRow {
    pub id: i64,
    pub project_id: String,
    pub article_id: i64,
    pub url_slug: String,
    pub generated_at: String,
    pub opportunity_score: i64,
    pub effort: String,
    pub recommended_action: String,
    pub signals_json: String,
    pub status: String,
    pub accepted_at: Option<String>,
    pub resulting_task_id: Option<String>,
}

/// Insert or replace opportunities for a given project + generated_at.
///
/// Uses `INSERT OR REPLACE` keyed by `(project_id, article_id, generated_at)`.
/// Only rows with `status = 'open'` are upserted; accepted/declined/done history
/// from prior runs is preserved.
pub fn save_opportunities(
    conn: &Connection,
    project_id: &str,
    generated_at: &str,
    opportunities: &[crate::models::seo_discovery::SeoOpportunity],
) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO seo_opportunities
         (project_id, article_id, url_slug, generated_at, opportunity_score,
          effort, recommended_action, signals_json, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open')
         ON CONFLICT(project_id, article_id, generated_at)
         DO UPDATE SET
             opportunity_score = excluded.opportunity_score,
             effort = excluded.effort,
             recommended_action = excluded.recommended_action,
             signals_json = excluded.signals_json
         WHERE seo_opportunities.status = 'open'",
    )?;

    for opp in opportunities {
        let signals_json = serde_json::to_string(&opp.signals_json)
            .unwrap_or_else(|_| "{}".to_string());
        stmt.execute(rusqlite::params![
            project_id,
            opp.article_id,
            &opp.url_slug,
            generated_at,
            opp.opportunity_score,
            &opp.effort,
            &opp.recommended_action,
            signals_json,
        ])?;
    }

    Ok(())
}

/// List open opportunities for a project, ranked by score descending.
pub fn list_open_opportunities(
    conn: &Connection,
    project_id: &str,
    limit: usize,
) -> Result<Vec<SeoOpportunityRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, article_id, url_slug, generated_at, opportunity_score,
                effort, recommended_action, signals_json, status, accepted_at, resulting_task_id
         FROM seo_opportunities
         WHERE project_id = ?1 AND status = 'open'
         ORDER BY opportunity_score DESC, article_id ASC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id, limit as i64], |row| {
        Ok(SeoOpportunityRow {
            id: row.get(0)?,
            project_id: row.get(1)?,
            article_id: row.get(2)?,
            url_slug: row.get(3)?,
            generated_at: row.get(4)?,
            opportunity_score: row.get(5)?,
            effort: row.get(6)?,
            recommended_action: row.get(7)?,
            signals_json: row.get(8)?,
            status: row.get(9)?,
            accepted_at: row.get(10)?,
            resulting_task_id: row.get(11)?,
        })
    })?;

    rows.collect()
}

/// Mark an opportunity as accepted and link it to the resulting task.
pub fn mark_accepted(
    conn: &Connection,
    id: i64,
    resulting_task_id: &str,
) -> Result<()> {
    let accepted_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE seo_opportunities
         SET status = 'accepted', accepted_at = ?1, resulting_task_id = ?2
         WHERE id = ?3",
        rusqlite::params![accepted_at, resulting_task_id, id],
    )?;
    Ok(())
}

/// Mark an opportunity as declined.
pub fn mark_declined(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE seo_opportunities SET status = 'declined' WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

/// Mark stale open opportunities (older than the given generated_at) as declined.
/// This prevents an ever-growing backlog of opportunities that were never acted on.
pub fn decline_stale_opportunities(
    conn: &Connection,
    project_id: &str,
    current_generated_at: &str,
) -> Result<usize> {
    let rows = conn.execute(
        "UPDATE seo_opportunities
         SET status = 'declined'
         WHERE project_id = ?1
           AND status = 'open'
           AND generated_at < ?2",
        rusqlite::params![project_id, current_generated_at],
    )?;
    Ok(rows)
}

/// Delete all SEO discovery data for a project (e.g. when project is deleted).
pub fn delete_project_opportunities(conn: &Connection, project_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM seo_opportunities WHERE project_id = ?1",
        [project_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.busy_timeout(std::time::Duration::from_secs(10)).ok();
        conn
    }

    fn insert_test_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, ?2, ?3, 1, 'workspace')",
            rusqlite::params![id, "Test Project", "/tmp/test"],
        )
        .unwrap();
    }

    #[test]
    fn save_and_list_opportunities() {
        let conn = in_memory_db();
        insert_test_project(&conn, "p1");

        let opps = vec![crate::models::seo_discovery::SeoOpportunity {
            article_id: 1,
            url_slug: "cold-brew".into(),
            title: "Cold Brew".into(),
            file: "cold-brew.mdx".into(),
            target_keyword: "cold brew".into(),
            opportunity_score: 1250,
            effort: "low".into(),
            recommended_action: "fix_ctr_article".into(),
            primary_signal: "ctr_opportunity".into(),
            signals_json: serde_json::json!({"clicks_lost": 42.0}),
        }];

        save_opportunities(&conn, "p1", "2026-07-09T00:00:00Z", &opps).unwrap();
        let listed = list_open_opportunities(&conn, "p1", 10).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].article_id, 1);
        assert_eq!(listed[0].recommended_action, "fix_ctr_article");
    }

    #[test]
    fn stale_opportunities_get_declined() {
        let conn = in_memory_db();
        insert_test_project(&conn, "p1");

        let opps = vec![crate::models::seo_discovery::SeoOpportunity {
            article_id: 1,
            url_slug: "a".into(),
            title: "A".into(),
            file: "a.mdx".into(),
            target_keyword: "a".into(),
            opportunity_score: 100,
            effort: "low".into(),
            recommended_action: "fix_content_article".into(),
            primary_signal: "content_health".into(),
            signals_json: serde_json::json!({}),
        }];

        save_opportunities(&conn, "p1", "2026-07-01T00:00:00Z", &opps).unwrap();
        let declined = decline_stale_opportunities(&conn, "p1", "2026-07-09T00:00:00Z").unwrap();
        assert_eq!(declined, 1);

        let open = list_open_opportunities(&conn, "p1", 10).unwrap();
        assert!(open.is_empty());
    }
}
