/// CRUD for the research_shortlist table.
///
/// The shortlist is a persistent queue of themes/keywords to research.
/// Sources:
///   - territory_analysis: open territories / saturated themes from GSC data
///   - coverage_gap: thin clusters from keyword coverage analysis
///   - manual: user-added entries
///
/// Consumers:
///   - research_keywords: reads pending entries, validates through DataForSEO,
///     marks as researched
///   - write_article: marks entries as covered when an article is published
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResearchShortlistEntry {
    pub id: Option<i64>,
    pub project_id: String,
    pub theme: String,
    pub seeds: Vec<String>,
    pub source: String,
    pub status: String,
    pub priority: String,
    pub article_count: Option<i64>,
    pub total_impressions: Option<f64>,
    pub added_at: String,
    pub researched_at: Option<String>,
    pub covered_at: Option<String>,
}

impl ResearchShortlistEntry {
    pub fn new(
        project_id: &str,
        theme: &str,
        seeds: Vec<String>,
        source: &str,
        priority: &str,
        article_count: Option<i64>,
        total_impressions: Option<f64>,
    ) -> Self {
        Self {
            id: None,
            project_id: project_id.to_string(),
            theme: theme.to_string(),
            seeds,
            source: source.to_string(),
            status: "pending".to_string(),
            priority: priority.to_string(),
            article_count,
            total_impressions,
            added_at: chrono::Utc::now().to_rfc3339(),
            researched_at: None,
            covered_at: None,
        }
    }
}

/// Insert or update a shortlist entry. Uses (project_id, theme, source) as the
/// natural key: if an entry already exists with the same theme for this project,
/// we update its seeds and metrics rather than creating a duplicate.
pub fn upsert_entry(conn: &Connection, entry: &ResearchShortlistEntry) -> Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM research_shortlist WHERE project_id = ?1 AND theme = ?2 AND source = ?3",
            rusqlite::params![&entry.project_id, &entry.theme, &entry.source],
            |row| row.get(0),
        )
        .optional()?;

    let seeds_json = serde_json::to_string(&entry.seeds).unwrap_or_else(|_| "[]".to_string());

    if let Some(id) = existing {
        conn.execute(
            "UPDATE research_shortlist
             SET seeds = ?1,
                 priority = ?2,
                 article_count = ?3,
                 total_impressions = ?4,
                 status = CASE WHEN status = 'covered' THEN 'pending' ELSE status END,
                 added_at = ?5
             WHERE id = ?6",
            rusqlite::params![
                &seeds_json,
                &entry.priority,
                entry.article_count,
                entry.total_impressions,
                &entry.added_at,
                id,
            ],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, article_count, total_impressions, added_at, researched_at, covered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &entry.project_id,
                &entry.theme,
                &seeds_json,
                &entry.source,
                &entry.status,
                &entry.priority,
                entry.article_count,
                entry.total_impressions,
                &entry.added_at,
                entry.researched_at.as_ref(),
                entry.covered_at.as_ref(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// List entries for a project, optionally filtered by status.
pub fn list_entries(
    conn: &Connection,
    project_id: &str,
    status_filter: Option<&str>,
) -> Result<Vec<ResearchShortlistEntry>> {
    let sql = if status_filter.is_some() {
        "SELECT id, project_id, theme, seeds, source, status, priority, article_count, total_impressions, added_at, researched_at, covered_at
         FROM research_shortlist
         WHERE project_id = ?1 AND status = ?2
         ORDER BY priority DESC, total_impressions DESC"
    } else {
        "SELECT id, project_id, theme, seeds, source, status, priority, article_count, total_impressions, added_at, researched_at, covered_at
         FROM research_shortlist
         WHERE project_id = ?1
         ORDER BY priority DESC, total_impressions DESC"
    };

    let mut stmt = conn.prepare(sql)?;

    let rows = if let Some(status) = status_filter {
        stmt.query_map(rusqlite::params![project_id, status], map_row)?
    } else {
        stmt.query_map([project_id], map_row)?
    };

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

/// Mark entries as researched.
pub fn mark_researched(conn: &Connection, ids: &[i64]) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "UPDATE research_shortlist SET status = 'researched', researched_at = ?1 WHERE id IN ({})",
        placeholders.join(",")
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&now];
    for id in ids {
        params.push(id);
    }
    let affected = conn.execute(&sql, &*params)?;
    Ok(affected)
}

/// Mark a single entry as covered (article written).
pub fn mark_covered(conn: &Connection, id: i64) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let affected = conn.execute(
        "UPDATE research_shortlist SET status = 'covered', covered_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, id],
    )?;
    Ok(affected)
}

/// Delete old covered entries to prevent table bloat.
pub fn prune_covered(conn: &Connection, project_id: &str, older_than_days: i64) -> Result<usize> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days);
    let affected = conn.execute(
        "DELETE FROM research_shortlist
         WHERE project_id = ?1 AND status = 'covered' AND covered_at < ?2",
        rusqlite::params![project_id, cutoff.to_rfc3339()],
    )?;
    Ok(affected)
}

fn map_row(row: &rusqlite::Row) -> std::result::Result<ResearchShortlistEntry, rusqlite::Error> {
    let seeds_json: String = row.get(3)?;
    let seeds: Vec<String> = serde_json::from_str(&seeds_json).unwrap_or_default();
    Ok(ResearchShortlistEntry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        theme: row.get(2)?,
        seeds,
        source: row.get(4)?,
        status: row.get(5)?,
        priority: row.get(6)?,
        article_count: row.get(7)?,
        total_impressions: row.get(8)?,
        added_at: row.get(9)?,
        researched_at: row.get(10)?,
        covered_at: row.get(11)?,
    })
}
