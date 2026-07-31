//! SQLite helpers for `posthog_page_daily` (conversion tape).

use crate::error::Result;
use crate::posthog::models::{PosthogPageDailyRow, PosthogPageWindow};
use rusqlite::Connection;
use std::collections::HashMap;

/// Append conversion tape rows. INSERT OR IGNORE on unique key.
/// Returns the number of newly inserted rows.
pub fn insert_rows(
    conn: &Connection,
    project_id: &str,
    rows: &[PosthogPageDailyRow],
) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut inserted = 0usize;
    for row in rows {
        inserted += conn.execute(
            "INSERT OR IGNORE INTO posthog_page_daily
             (project_id, page, event, date, count, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                project_id,
                row.page,
                row.event,
                row.date,
                row.count,
                now,
            ],
        )?;
    }
    Ok(inserted)
}

/// Latest data date present for a project (YYYY-MM-DD).
pub fn latest_date(conn: &Connection, project_id: &str) -> Result<Option<String>> {
    let date = conn.query_row(
        "SELECT MAX(date) FROM posthog_page_daily WHERE project_id = ?1",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;
    Ok(date)
}

/// Latest `fetched_at` for freshness.
pub fn latest_fetched_at(conn: &Connection, project_id: &str) -> Result<Option<String>> {
    let ts = conn.query_row(
        "SELECT MAX(fetched_at) FROM posthog_page_daily WHERE project_id = ?1",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;
    Ok(ts)
}

/// List raw rows for a project in an inclusive date window.
pub fn list_rows(
    conn: &Connection,
    project_id: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<PosthogPageDailyRow>> {
    let mut stmt = conn.prepare(
        "SELECT page, event, date, count FROM posthog_page_daily
         WHERE project_id = ?1 AND date >= ?2 AND date <= ?3
         ORDER BY date ASC, page ASC, event ASC",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![project_id, start_date, end_date], |row| {
            Ok(PosthogPageDailyRow {
                page: row.get(0)?,
                event: row.get(1)?,
                date: row.get(2)?,
                count: row.get(3)?,
            })
        })?
        .filter_map(|r| match r {
            Ok(row) => Some(row),
            Err(e) => {
                log::warn!("[posthog/db] list_rows: dropping row decode error: {e}");
                None
            }
        })
        .collect();
    Ok(rows)
}

/// Distinct days with any conversion data in the window (for one page match set).
pub fn days_with_data_for_pages(
    conn: &Connection,
    project_id: &str,
    pages: &[String],
    start_date: &str,
    end_date: &str,
) -> Result<i64> {
    if pages.is_empty() {
        return Ok(0);
    }
    let placeholders = pages
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 4))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT COUNT(DISTINCT date) FROM posthog_page_daily
         WHERE project_id = ?1 AND date >= ?2 AND date <= ?3
           AND page IN ({placeholders})"
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(project_id.to_string()));
    params.push(Box::new(start_date.to_string()));
    params.push(Box::new(end_date.to_string()));
    for p in pages {
        params.push(Box::new(p.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))?;
    Ok(count)
}

/// Aggregate event totals for matching pages over a window.
///
/// Returns `(days_with_data, events_map)` where events_map is event → sum(count).
pub fn window_event_totals(
    conn: &Connection,
    project_id: &str,
    pages: &[String],
    start_date: &str,
    end_date: &str,
) -> Result<(i64, HashMap<String, f64>)> {
    if pages.is_empty() {
        return Ok((0, HashMap::new()));
    }

    let days = days_with_data_for_pages(conn, project_id, pages, start_date, end_date)?;

    let placeholders = pages
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 4))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT event, COALESCE(SUM(count), 0) FROM posthog_page_daily
         WHERE project_id = ?1 AND date >= ?2 AND date <= ?3
           AND page IN ({placeholders})
         GROUP BY event"
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(project_id.to_string()));
    params.push(Box::new(start_date.to_string()));
    params.push(Box::new(end_date.to_string()));
    for p in pages {
        params.push(Box::new(p.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let mut events = HashMap::new();
    let mapped = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;
    for r in mapped {
        match r {
            Ok((event, count)) => {
                events.insert(event, count);
            }
            Err(e) => {
                log::warn!("[posthog/db] window_event_totals: dropping row decode error: {e}");
            }
        }
    }
    Ok((days, events))
}

/// Per-page totals (sum of all events) over a window, for site-overview samples.
pub fn page_totals_window(
    conn: &Connection,
    project_id: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<PosthogPageWindow>> {
    let mut stmt = conn.prepare(
        "SELECT page, event, COALESCE(SUM(count), 0), COUNT(DISTINCT date)
         FROM posthog_page_daily
         WHERE project_id = ?1 AND date >= ?2 AND date <= ?3
         GROUP BY page, event
         ORDER BY page",
    )?;
    let mut by_page: HashMap<String, PosthogPageWindow> = HashMap::new();
    let mapped = stmt.query_map(
        rusqlite::params![project_id, start_date, end_date],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )?;
    for r in mapped {
        let (page, event, count, days) = match r {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[posthog/db] page_totals_window: dropping row decode error: {e}");
                continue;
            }
        };
        let entry = by_page.entry(page.clone()).or_insert_with(|| PosthogPageWindow {
            page: page.clone(),
            days_with_data: 0,
            events: HashMap::new(),
            total: 0.0,
        });
        entry.events.insert(event, count);
        entry.total += count;
        if days > entry.days_with_data {
            entry.days_with_data = days;
        }
    }
    let mut out: Vec<PosthogPageWindow> = by_page.into_values().collect();
    out.sort_by(|a, b| {
        b.total
            .partial_cmp(&a.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// List distinct page paths that have any conversion data.
pub fn list_pages(conn: &Connection, project_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT page FROM posthog_page_daily WHERE project_id = ?1 ORDER BY page",
    )?;
    let pages = stmt
        .query_map(rusqlite::params![project_id], |row| row.get(0))?
        .filter_map(|r| match r {
            Ok(page) => Some(page),
            Err(e) => {
                log::warn!("[posthog/db] list_pages: dropping row decode error: {e}");
                None
            }
        })
        .collect();
    Ok(pages)
}

/// Delete rows older than a cutoff date.
pub fn prune_old_rows(conn: &Connection, project_id: &str, cutoff_date: &str) -> Result<usize> {
    let rows = conn.execute(
        "DELETE FROM posthog_page_daily WHERE project_id = ?1 AND date < ?2",
        rusqlite::params![project_id, cutoff_date],
    )?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::posthog::models::PosthogPageDailyRow;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES ('proj1', 'Test', '/tmp/test')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn insert_is_idempotent() {
        let conn = setup();
        let rows = vec![PosthogPageDailyRow {
            page: "/blog/foo".into(),
            event: "signup_started".into(),
            date: "2026-07-01".into(),
            count: 3.0,
        }];
        let n1 = insert_rows(&conn, "proj1", &rows).unwrap();
        assert_eq!(n1, 1);
        let n2 = insert_rows(&conn, "proj1", &rows).unwrap();
        assert_eq!(n2, 0);
        let listed = list_rows(&conn, "proj1", "2026-01-01", "2026-12-31").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].count, 3.0);
    }

    #[test]
    fn window_aggregates_events() {
        let conn = setup();
        let rows = vec![
            PosthogPageDailyRow {
                page: "/blog/foo".into(),
                event: "signup_started".into(),
                date: "2026-07-01".into(),
                count: 2.0,
            },
            PosthogPageDailyRow {
                page: "/blog/foo".into(),
                event: "signup_started".into(),
                date: "2026-07-02".into(),
                count: 3.0,
            },
            PosthogPageDailyRow {
                page: "/blog/foo".into(),
                event: "cta_clicked".into(),
                date: "2026-07-01".into(),
                count: 10.0,
            },
        ];
        insert_rows(&conn, "proj1", &rows).unwrap();
        let pages = vec!["/blog/foo".to_string()];
        let (days, events) =
            window_event_totals(&conn, "proj1", &pages, "2026-07-01", "2026-07-31").unwrap();
        assert_eq!(days, 2);
        assert_eq!(events.get("signup_started").copied().unwrap_or(0.0), 5.0);
        assert_eq!(events.get("cta_clicked").copied().unwrap_or(0.0), 10.0);
    }

    #[test]
    fn page_totals_sorted_by_total() {
        let conn = setup();
        insert_rows(
            &conn,
            "proj1",
            &[
                PosthogPageDailyRow {
                    page: "/a".into(),
                    event: "signup_started".into(),
                    date: "2026-07-01".into(),
                    count: 1.0,
                },
                PosthogPageDailyRow {
                    page: "/b".into(),
                    event: "signup_started".into(),
                    date: "2026-07-01".into(),
                    count: 5.0,
                },
            ],
        )
        .unwrap();
        let totals = page_totals_window(&conn, "proj1", "2026-07-01", "2026-07-31").unwrap();
        assert_eq!(totals[0].page, "/b");
        assert_eq!(totals[0].total, 5.0);
    }
}
