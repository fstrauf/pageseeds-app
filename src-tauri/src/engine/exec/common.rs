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
            return Err(StepResult {
                success: false,
                message: format!("{}: failed to read {}: {}", context, path.display(), e),
                output: None,
            });
        }
    };
    match serde_json::from_str(&content) {
        Ok(v) => Ok(v),
        Err(e) => Err(StepResult {
            success: false,
            message: format!("{}: invalid JSON in {}: {}", context, path.display(), e),
            output: None,
        }),
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
            return Err(StepResult {
                success: false,
                message: format!("{}: failed to serialize: {}", context, e),
                output: None,
            });
        }
    };
    match std::fs::write(path, json) {
        Ok(()) => Ok(()),
        Err(e) => Err(StepResult {
            success: false,
            message: format!("{}: failed to write {}: {}", context, path.display(), e),
            output: None,
        }),
    }
}
