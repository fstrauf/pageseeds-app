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
    pub signal_score: Option<f64>,
    pub health_status: String,
    pub last_reviewed_at: Option<String>,
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
            signal_score: None,
            health_status: "unproven".to_string(),
            last_reviewed_at: None,
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
                 signal_score = ?5,
                 health_status = ?6,
                 last_reviewed_at = ?7,
                 status = CASE WHEN status = 'covered' THEN 'pending' ELSE status END,
                 added_at = ?8
             WHERE id = ?9",
            rusqlite::params![
                &seeds_json,
                &entry.priority,
                entry.article_count,
                entry.total_impressions,
                entry.signal_score,
                &entry.health_status,
                entry.last_reviewed_at.as_ref(),
                &entry.added_at,
                id,
            ],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, article_count, total_impressions, signal_score, health_status, last_reviewed_at, added_at, researched_at, covered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                &entry.project_id,
                &entry.theme,
                &seeds_json,
                &entry.source,
                &entry.status,
                &entry.priority,
                entry.article_count,
                entry.total_impressions,
                entry.signal_score,
                &entry.health_status,
                entry.last_reviewed_at.as_ref(),
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
        "SELECT id, project_id, theme, seeds, source, status, priority, article_count, total_impressions, signal_score, health_status, last_reviewed_at, added_at, researched_at, covered_at
         FROM research_shortlist
         WHERE project_id = ?1 AND status = ?2
         ORDER BY priority DESC, total_impressions DESC"
    } else {
        "SELECT id, project_id, theme, seeds, source, status, priority, article_count, total_impressions, signal_score, health_status, last_reviewed_at, added_at, researched_at, covered_at
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

/// Mark shortlist entries as covered when a picked keyword matches the entry's
/// theme or any of its seeds (normalized via the canonical keyword normalizer,
/// so stored keywords with quotes/long phrases still match).
///
/// Best-effort by contract (issue #23): a keyword that matches nothing is a
/// no-op, never an error. Returns the number of entries marked.
pub fn mark_covered_for_keywords(
    conn: &Connection,
    project_id: &str,
    keywords: &[String],
) -> Result<usize> {
    use crate::content::keyword_match::normalize_keyword;

    let picked: Vec<String> = keywords
        .iter()
        .map(|k| normalize_keyword(k))
        .filter(|k| !k.is_empty())
        .collect();
    if picked.is_empty() {
        return Ok(0);
    }

    let entries = list_entries(conn, project_id, None)?;
    let mut marked = 0usize;
    for entry in entries {
        if entry.status == "covered" {
            continue;
        }
        let theme_norm = normalize_keyword(&entry.theme);
        let matched = picked.iter().any(|k| {
            *k == theme_norm || entry.seeds.iter().any(|s| normalize_keyword(s) == *k)
        });
        if matched {
            if let Some(id) = entry.id {
                marked += mark_covered(conn, id)?;
            }
        }
    }
    Ok(marked)
}

/// Update topic health for a given theme.
pub fn update_health(
    conn: &Connection,
    project_id: &str,
    theme: &str,
    health_status: &str,
    signal_score: Option<f64>,
) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let affected = conn.execute(
        "UPDATE research_shortlist
         SET health_status = ?1,
             signal_score = ?2,
             last_reviewed_at = ?3
         WHERE project_id = ?4 AND theme = ?5",
        rusqlite::params![health_status, signal_score, &now, project_id, theme],
    )?;
    Ok(affected)
}

/// List pending shortlist entries for a project, excluding depleted themes.
pub fn list_pending_excluding_depleted(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<ResearchShortlistEntry>> {
    let sql = "SELECT id, project_id, theme, seeds, source, status, priority, article_count, total_impressions, signal_score, health_status, last_reviewed_at, added_at, researched_at, covered_at
         FROM research_shortlist
         WHERE project_id = ?1 AND status = 'pending' AND health_status != 'depleted'
         ORDER BY priority DESC, total_impressions DESC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([project_id], map_row)?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
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
        signal_score: row.get(9)?,
        health_status: row.get(10)?,
        last_reviewed_at: row.get(11)?,
        added_at: row.get(12)?,
        researched_at: row.get(13)?,
        covered_at: row.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE research_shortlist (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                theme TEXT NOT NULL,
                seeds TEXT NOT NULL DEFAULT '[]',
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                priority TEXT NOT NULL,
                article_count INTEGER,
                total_impressions REAL,
                signal_score REAL,
                health_status TEXT NOT NULL,
                last_reviewed_at TEXT,
                added_at TEXT NOT NULL,
                researched_at TEXT,
                covered_at TEXT
            );",
        )
        .unwrap();
        conn
    }

    fn insert_entry(
        conn: &Connection,
        project_id: &str,
        theme: &str,
        status: &str,
        health_status: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES (?1, ?2, '[]', 'test', ?3, 'medium', ?4, ?5)",
            rusqlite::params![project_id, theme, status, health_status, chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn list_pending_excluding_depleted_returns_only_pending_non_depleted_entries() {
        let conn = in_memory_db();
        insert_entry(&conn, "proj1", "seo tools", "pending", "unproven");
        insert_entry(&conn, "proj1", "keyword research", "pending", "promising");
        insert_entry(&conn, "proj1", "content marketing", "pending", "depleted");
        insert_entry(&conn, "proj1", "technical seo", "researched", "unproven");
        insert_entry(&conn, "proj2", "link building", "pending", "depleted");

        let entries = list_pending_excluding_depleted(&conn, "proj1").unwrap();
        assert_eq!(entries.len(), 2);
        let themes: Vec<String> = entries.iter().map(|e| e.theme.clone()).collect();
        assert!(themes.contains(&"seo tools".to_string()));
        assert!(themes.contains(&"keyword research".to_string()));
        assert!(!themes.contains(&"content marketing".to_string()));
        assert!(!themes.contains(&"technical seo".to_string()));
        assert!(!themes.contains(&"link building".to_string()));
    }

    #[test]
    fn update_health_sets_status_signal_score_and_timestamp() {
        let conn = in_memory_db();
        insert_entry(&conn, "proj1", "seo tools", "pending", "unproven");

        let affected = update_health(&conn, "proj1", "seo tools", "promising", Some(85.0)).unwrap();
        assert_eq!(affected, 1);

        let entries = list_entries(&conn, "proj1", None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].health_status, "promising");
        assert_eq!(entries[0].signal_score, Some(85.0));
        assert!(entries[0].last_reviewed_at.is_some());
    }

    #[test]
    fn upsert_entry_prevents_duplicate_themes_for_same_project_and_source() {
        let conn = in_memory_db();
        let mut entry = ResearchShortlistEntry::new(
            "proj1",
            "seo tools",
            vec!["seo".to_string()],
            "test",
            "high",
            None,
            None,
        );
        let id1 = upsert_entry(&conn, &entry).unwrap();

        entry.seeds = vec!["seo".to_string(), "search engine optimization".to_string()];
        entry.health_status = "promising".to_string();
        let id2 = upsert_entry(&conn, &entry).unwrap();

        assert_eq!(id1, id2);

        let entries = list_entries(&conn, "proj1", None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].seeds.len(), 2);
        assert_eq!(entries[0].health_status, "promising");
    }

    fn insert_entry_with_seeds(conn: &Connection, theme: &str, seeds: &[&str], status: &str) -> i64 {
        let seeds_json = serde_json::to_string(&seeds).unwrap();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES ('proj1', ?1, ?2, 'test', ?3, 'medium', 'unproven', ?4)",
            rusqlite::params![theme, seeds_json, status, chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn mark_covered_for_keywords_matches_theme_and_seeds() {
        let conn = in_memory_db();
        insert_entry_with_seeds(&conn, "delta hedging", &["delta hedge", "hedging delta"], "pending");
        insert_entry_with_seeds(&conn, "theta decay", &["time decay"], "researched");
        insert_entry_with_seeds(&conn, "gamma scalping", &[], "pending");

        // Match by seed, by theme, and one keyword that matches nothing.
        let marked = mark_covered_for_keywords(
            &conn,
            "proj1",
            &[
                "delta hedge".to_string(),
                "Theta Decay".to_string(),
                "unrelated keyword".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(marked, 2);

        let entries = list_entries(&conn, "proj1", None).unwrap();
        let by_theme = |t: &str| entries.iter().find(|e| e.theme == t).unwrap();
        assert_eq!(by_theme("delta hedging").status, "covered");
        assert!(by_theme("delta hedging").covered_at.is_some());
        assert_eq!(by_theme("theta decay").status, "covered");
        // Unmatched entry stays pending.
        assert_eq!(by_theme("gamma scalping").status, "pending");
    }

    #[test]
    fn mark_covered_for_keywords_is_idempotent_and_never_fails_on_no_match() {
        let conn = in_memory_db();
        insert_entry_with_seeds(&conn, "delta hedging", &[], "pending");

        // No match → no-op, Ok(0).
        let marked =
            mark_covered_for_keywords(&conn, "proj1", &["something else".to_string()]).unwrap();
        assert_eq!(marked, 0);

        // Empty keyword list → no-op.
        let marked = mark_covered_for_keywords(&conn, "proj1", &[]).unwrap();
        assert_eq!(marked, 0);

        // Mark once, then again — already-covered rows are skipped.
        let marked = mark_covered_for_keywords(&conn, "proj1", &["delta hedging".to_string()]).unwrap();
        assert_eq!(marked, 1);
        let marked = mark_covered_for_keywords(&conn, "proj1", &["delta hedging".to_string()]).unwrap();
        assert_eq!(marked, 0);
    }

    #[test]
    fn mark_covered_for_keywords_normalizes_quotes_and_case() {
        let conn = in_memory_db();
        insert_entry_with_seeds(&conn, "delta hedging", &[], "pending");

        // Quoted / differently-cased picked keyword still matches via the
        // canonical normalizer.
        let marked =
            mark_covered_for_keywords(&conn, "proj1", &["\"Delta  Hedging\"".to_string()]).unwrap();
        assert_eq!(marked, 1);
    }
}
