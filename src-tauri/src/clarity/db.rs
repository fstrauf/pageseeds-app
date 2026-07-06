use crate::clarity::models::ClarityExportRow;
use crate::error::Result;
use rusqlite::Connection;
use std::collections::HashMap;

/// Insert a batch of export rows for a project.
pub fn insert_rows(
    conn: &Connection,
    project_id: &str,
    exported_at: &str,
    rows: &[ClarityExportRow],
) -> Result<usize> {
    let mut stmt = conn.prepare(
        "INSERT INTO clarity_export_rows
         (project_id, exported_at, clarity_date, dimension_set, metric_name, dimension_json, value_json, raw_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut count = 0;
    for row in rows {
        stmt.execute(rusqlite::params![
            project_id,
            exported_at,
            row.clarity_date,
            row.dimension_set,
            row.metric_name,
            serde_json::to_string(&row.dimensions)?,
            serde_json::to_string(&row.values)?,
            serde_json::to_string(row)?,
            &now,
        ])?;
        count += 1;
    }
    Ok(count)
}

/// List all rows for a project within a date range.
pub fn list_rows(
    conn: &Connection,
    project_id: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<ClarityExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT clarity_date, dimension_set, metric_name, dimension_json, value_json
         FROM clarity_export_rows
         WHERE project_id = ?1 AND clarity_date >= ?2 AND clarity_date <= ?3
         ORDER BY clarity_date DESC, dimension_set ASC, metric_name ASC",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![project_id, start_date, end_date], |row| {
            let dimension_json: String = row.get(3)?;
            let value_json: String = row.get(4)?;
            Ok(ClarityExportRow {
                clarity_date: row.get(0)?,
                dimension_set: row.get(1)?,
                metric_name: row.get(2)?,
                dimensions: serde_json::from_str(&dimension_json)
                    .unwrap_or_else(|_| HashMap::new()),
                values: serde_json::from_str(&value_json).unwrap_or_else(|_| HashMap::new()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

/// Delete rows older than a cutoff date to keep the table bounded.
pub fn prune_old_rows(conn: &Connection, project_id: &str, cutoff_date: &str) -> Result<usize> {
    let rows = conn.execute(
        "DELETE FROM clarity_export_rows WHERE project_id = ?1 AND clarity_date < ?2",
        rusqlite::params![project_id, cutoff_date],
    )?;
    Ok(rows)
}

/// Count rows for a project on a given clarity_date.
#[allow(dead_code)]
pub fn count_rows_for_date(conn: &Connection, project_id: &str, clarity_date: &str) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM clarity_export_rows WHERE project_id = ?1 AND clarity_date = ?2",
        rusqlite::params![project_id, clarity_date],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}
