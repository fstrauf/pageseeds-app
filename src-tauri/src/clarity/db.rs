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

/// Latest snapshot date present for a dimension set within a date range.
pub fn latest_snapshot_date(
    conn: &Connection,
    project_id: &str,
    dimension_set: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Option<String>> {
    let date = conn.query_row(
        "SELECT MAX(clarity_date) FROM clarity_export_rows
         WHERE project_id = ?1 AND dimension_set = ?2
           AND clarity_date >= ?3 AND clarity_date <= ?4",
        rusqlite::params![project_id, dimension_set, start_date, end_date],
        |row| row.get(0),
    )?;
    Ok(date)
}

/// List rows for a project within a date range, optionally restricted to one
/// dimension set and/or a single snapshot date.
pub fn list_rows(
    conn: &Connection,
    project_id: &str,
    start_date: &str,
    end_date: &str,
    dimension_set: Option<&str>,
    clarity_date: Option<&str>,
) -> Result<Vec<ClarityExportRow>> {
    let mut sql = String::from(
        "SELECT clarity_date, dimension_set, metric_name, dimension_json, value_json
         FROM clarity_export_rows
         WHERE project_id = :project_id AND clarity_date >= :start_date AND clarity_date <= :end_date",
    );
    if dimension_set.is_some() {
        sql.push_str(" AND dimension_set = :dimension_set");
    }
    if clarity_date.is_some() {
        sql.push_str(" AND clarity_date = :clarity_date");
    }
    sql.push_str(" ORDER BY clarity_date DESC, dimension_set ASC, metric_name ASC");

    let mut params: Vec<(&str, &dyn rusqlite::ToSql)> = vec![
        (":project_id", &project_id),
        (":start_date", &start_date),
        (":end_date", &end_date),
    ];
    if let Some(ds) = &dimension_set {
        params.push((":dimension_set", ds));
    }
    if let Some(d) = &clarity_date {
        params.push((":clarity_date", d));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(&*params, |row| {
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
