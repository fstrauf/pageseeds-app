use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use serde::{de::DeserializeOwned, Serialize};
/// Shared helpers for workflow step executors.
///
/// Reduces duplicated JSON file I/O and error-handling boilerplate across
/// `engine/exec/` modules.
use std::path::Path;

// ─── Article / audit loading helpers ──────────────────────────────────────────
//
// These helpers centralise the "load articles.json + build slug/file index" and
// "load content audit (DB primary, JSON fallback)" patterns that were duplicated
// across ~9 exec modules. They preserve the exact same semantics as the inline
// code they replace: read from the JSON file on disk (the export target), with
// the DB as primary source for the audit snapshot.
//
// Part of Stage B of issue #4.

use std::collections::HashMap;

/// Snapshot of project articles loaded from `articles.json` in the automation dir.
///
/// `by_slug` and `by_file` are pre-built for O(1) lookup by URL slug or file path.
pub struct ProjectArticles {
    pub doc: serde_json::Value,
    pub articles: Vec<serde_json::Value>,
    pub by_slug: HashMap<String, serde_json::Value>,
    pub by_file: HashMap<String, serde_json::Value>,
}

/// Load articles from the automation dir's `articles.json`.
///
/// Returns an empty snapshot (not an error) if the file is missing or invalid —
/// matches the inline code's `unwrap_or_else(|| json!({ "articles": [] }))` pattern.
pub fn load_project_articles(paths: &ProjectPaths) -> ProjectArticles {
    let articles_path = paths.automation_dir.join("articles.json");
    let doc: serde_json::Value = std::fs::read_to_string(&articles_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "articles": [] }));

    let articles = doc["articles"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut by_slug = HashMap::new();
    let mut by_file = HashMap::new();
    for article in &articles {
        if let Some(slug) = article["url_slug"].as_str() {
            if !slug.is_empty() {
                by_slug.insert(slug.to_string(), article.clone());
            }
        }
        if let Some(file) = article["file"].as_str() {
            if !file.is_empty() {
                by_file.insert(file.to_string(), article.clone());
            }
        }
    }

    ProjectArticles {
        doc,
        articles,
        by_slug,
        by_file,
    }
}

/// Snapshot of the content audit, loaded from DB (primary) or `content_audit.json` (fallback).
///
/// `by_slug` and `by_file` are pre-built for O(1) lookup.
pub struct AuditSnapshot {
    pub doc: serde_json::Value,
    pub articles: Vec<serde_json::Value>,
    pub by_slug: HashMap<String, serde_json::Value>,
    pub by_file: HashMap<String, serde_json::Value>,
}

/// Load the content audit from DB (primary) or `content_audit.json` (fallback).
///
/// Returns an empty snapshot (not an error) if neither source is available —
/// matches the inline code's graceful-degradation pattern.
pub fn load_audit_snapshot(project_id: &str, paths: &ProjectPaths) -> AuditSnapshot {
    let doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_audit_report_as_json(&conn, project_id)
                    .ok()
                    .flatten()
            });
        db_doc.unwrap_or_else(|| {
            let audit_path = paths.automation_dir.join("content_audit.json");
            std::fs::read_to_string(&audit_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({ "articles": [] }))
        })
    };

    let articles = doc["articles"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut by_slug = HashMap::new();
    let mut by_file = HashMap::new();
    for article in &articles {
        if let Some(slug) = article["url_slug"].as_str() {
            if !slug.is_empty() {
                by_slug.insert(slug.to_string(), article.clone());
            }
        }
        if let Some(file) = article["file"].as_str() {
            if !file.is_empty() {
                by_file.insert(file.to_string(), article.clone());
            }
        }
    }

    AuditSnapshot {
        doc,
        articles,
        by_slug,
        by_file,
    }
}

/// Read a JSON file from disk and deserialize it.
///
/// Returns a `StepResult` error on failure so callers can propagate directly
/// from a workflow step handler.
pub fn read_json<T: DeserializeOwned>(path: &Path, context: &str) -> Result<T, StepResult> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return Err(StepResult::fail(format!(
                "{}: failed to read {}: {}",
                context,
                path.display(),
                e
            )));
        }
    };
    match serde_json::from_str(&content) {
        Ok(v) => Ok(v),
        Err(e) => Err(StepResult::fail(format!(
            "{}: invalid JSON in {}: {}",
            context,
            path.display(),
            e
        ))),
    }
}

/// Serialize a value to pretty JSON and write it to disk.
///
/// Returns a `StepResult` error on failure so callers can propagate directly
/// from a workflow step handler.
pub fn write_json<T: Serialize>(path: &Path, value: &T, context: &str) -> Result<(), StepResult> {
    let json = match serde_json::to_string_pretty(value) {
        Ok(j) => j,
        Err(e) => {
            return Err(StepResult::fail(format!(
                "{}: failed to serialize: {}",
                context, e
            )));
        }
    };
    match std::fs::write(path, json) {
        Ok(()) => Ok(()),
        Err(e) => Err(StepResult::fail(format!(
            "{}: failed to write {}: {}",
            context,
            path.display(),
            e
        ))),
    }
}

// ─── GSC metrics staleness ────────────────────────────────────────────────────

/// Maximum tolerated age of the Search Analytics metrics before downstream
/// consumers warn / the indexing-health gate fails closed (issue #25).
pub const GSC_METRICS_MAX_AGE_DAYS: i64 = 7;

/// Build a visible staleness warning when the newest row in `ctr_query_metrics`
/// is older than [`GSC_METRICS_MAX_AGE_DAYS`].
///
/// Returns `None` when metrics are fresh, when the table is empty (no data at
/// all is handled by the callers' existing fallbacks), or when the check
/// itself fails — this is a warning, never a hard failure.
pub fn ctr_metrics_staleness_warning(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Option<String> {
    let last_synced = crate::db::ctr_query_metrics_max_fetched_at(conn, project_id).ok()??;
    let synced_at = chrono::DateTime::parse_from_rfc3339(&last_synced).ok()?;
    let age = chrono::Utc::now().signed_duration_since(synced_at);
    if age > chrono::Duration::days(GSC_METRICS_MAX_AGE_DAYS) {
        Some(format!(
            "WARNING: GSC query metrics are stale (last synced {}, {} days ago — threshold is {} days). Re-run collect_gsc to refresh.",
            last_synced,
            age.num_days(),
            GSC_METRICS_MAX_AGE_DAYS
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_metric(conn: &rusqlite::Connection, project_id: &str, fetched_at: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', '/tmp/test', 1, 'workspace')",
            rusqlite::params![project_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ctr_query_metrics
             (project_id, article_id, page_url, query, impressions, clicks, ctr, avg_position, fetched_at)
             VALUES (?1, 1, '/page', 'kw', 10.0, 1.0, 0.1, 5.0, ?2)",
            rusqlite::params![project_id, fetched_at],
        )
        .unwrap();
    }

    #[test]
    fn staleness_warning_none_when_table_empty() {
        let conn = in_memory_db();
        assert!(ctr_metrics_staleness_warning(&conn, "p1").is_none());
    }

    #[test]
    fn staleness_warning_none_when_fresh() {
        let conn = in_memory_db();
        insert_metric(&conn, "p1", &chrono::Utc::now().to_rfc3339());
        assert!(ctr_metrics_staleness_warning(&conn, "p1").is_none());
    }

    #[test]
    fn staleness_warning_some_when_older_than_threshold() {
        let conn = in_memory_db();
        let old = (chrono::Utc::now()
            - chrono::Duration::days(GSC_METRICS_MAX_AGE_DAYS + 3))
        .to_rfc3339();
        insert_metric(&conn, "p1", &old);
        let warning = ctr_metrics_staleness_warning(&conn, "p1").expect("must warn on stale metrics");
        assert!(warning.contains("WARNING"));
        assert!(warning.contains("collect_gsc"));
    }

    #[test]
    fn staleness_warning_scoped_to_project() {
        let conn = in_memory_db();
        let old = (chrono::Utc::now()
            - chrono::Duration::days(GSC_METRICS_MAX_AGE_DAYS + 3))
        .to_rfc3339();
        insert_metric(&conn, "other-project", &old);
        assert!(ctr_metrics_staleness_warning(&conn, "p1").is_none());
    }
}
