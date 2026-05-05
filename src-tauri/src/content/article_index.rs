/// Article Index Service — single backend boundary for workspace article metadata.
///
/// SQLite is the canonical runtime store. This module provides:
///   - Import/export of the `.github/automation/articles.json` projection
///   - Stale-file cleanup that updates SQLite first
///   - Orphan-file ingestion that updates SQLite first
///   - Metadata sync from MDX frontmatter back to SQLite
///
/// Workflow executors should call this module instead of reading `articles.json`
/// directly. The only approved direct JSON access is in `db::export` (used by
/// this service) and setup diagnostics.
use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::error::Result;
use crate::models::article::Article;

// ═══════════════════════════════════════════════════════════════════════════════
// Summary types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub imported: usize,
}

#[derive(Debug, Serialize)]
pub struct ExportSummary {
    pub exported: usize,
}

#[derive(Debug, Serialize)]
pub struct CleanSummary {
    pub removed: Vec<String>,
    pub json_cleaned: bool,
    pub db_cleaned: bool,
}

#[derive(Debug, Serialize)]
pub struct IngestSummary {
    pub ingested: usize,
    pub files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncSummary {
    pub updated: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Read
// ═══════════════════════════════════════════════════════════════════════════════

/// List all articles for a project from SQLite.
pub fn list_articles(conn: &Connection, project_id: &str) -> Result<Vec<Article>> {
    crate::engine::task_store::list_articles(conn, project_id)
}

/// Get existing target keywords for a project (used for deduplication).
pub fn existing_keywords(conn: &Connection, project_id: &str) -> Result<HashSet<String>> {
    let articles = list_articles(conn, project_id)?;
    let mut set = HashSet::new();
    for a in &articles {
        if let Some(kw) = a.target_keyword.as_deref() {
            if !kw.is_empty() {
                set.insert(kw.to_lowercase());
            }
        }
    }
    Ok(set)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Import / Export projection
// ═══════════════════════════════════════════════════════════════════════════════

/// Export SQLite article records to `articles.json` in the repo.
/// Preserves unknown/custom fields from the existing JSON file.
pub fn export_projection(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<ExportSummary> {
    crate::db::export::write_articles_to_repo(conn, project_id, project_path)?;
    let count: usize = conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE project_id = ?1",
        [project_id],
        |row| row.get(0),
    )?;
    Ok(ExportSummary { exported: count })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stale cleanup (SQLite-first)
// ═══════════════════════════════════════════════════════════════════════════════

/// Remove articles whose content files no longer exist.
///
/// 1. Deletes rows from SQLite.
/// 2. Re-exports the projection so `articles.json` stays in sync.
pub fn clean_stale_articles(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<CleanSummary> {
    let automation_dir = project_path.join(".github").join("automation");

    // Determine which files are missing from disk.
    let content_dir = crate::content::ops::resolve_content_dir(&automation_dir, project_path)
        .map_err(|e| crate::error::Error::Other(e))?;

    let content_files: HashSet<String> =
        crate::content::locator::collect_markdown_files(&content_dir)
            .into_iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_string())
            })
            .collect();

    // Load articles from SQLite so we evaluate against the canonical store.
    let articles = list_articles(conn, project_id)?;
    let mut removed = Vec::new();

    for article in &articles {
        let basename = std::path::Path::new(&article.file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if basename.is_empty() {
            continue;
        }
        if !content_files.contains(basename) {
            removed.push(format!("{} ({})", article.title, article.file));
            conn.execute(
                "DELETE FROM articles WHERE id = ?1 AND project_id = ?2",
                rusqlite::params![article.id, project_id],
            )?;
        }
    }

    let db_cleaned = !removed.is_empty();
    let json_cleaned = if db_cleaned {
        crate::db::export::write_articles_to_repo(conn, project_id, project_path).is_ok()
    } else {
        true
    };

    Ok(CleanSummary {
        removed,
        json_cleaned,
        db_cleaned,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Orphan ingestion (SQLite-first)
// ═══════════════════════════════════════════════════════════════════════════════

/// Ingest MDX files on disk that are not yet tracked in SQLite.
///
/// 1. Scans the content directory for files missing from SQLite.
/// 2. Inserts new rows into SQLite.
/// 3. Re-exports the projection so `articles.json` stays in sync.
#[allow(dead_code)]
pub fn ingest_orphans(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<IngestSummary> {
    let automation_dir = project_path.join(".github").join("automation");
    let content_dir = crate::content::ops::resolve_content_dir(&automation_dir, project_path)
        .map_err(|e| crate::error::Error::Other(e))?;

    // Build a map of all content files: basename → full path.
    let content_files: std::collections::HashMap<String, std::path::PathBuf> =
        crate::content::locator::collect_markdown_files(&content_dir)
            .into_iter()
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?.to_string();
                Some((name, p))
            })
            .collect();

    // Existing tracked basenames from SQLite.
    let articles = list_articles(conn, project_id)?;
    let mut tracked_basenames = HashSet::new();
    for article in &articles {
        if let Some(name) = std::path::Path::new(&article.file)
            .file_name()
            .and_then(|n| n.to_str())
        {
            tracked_basenames.insert(name.to_string());
        }
    }

    // Find orphans.
    let mut orphans: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (basename, path) in &content_files {
        if !tracked_basenames.contains(basename) {
            orphans.push((basename.clone(), path.clone()));
        }
    }

    if orphans.is_empty() {
        return Ok(IngestSummary {
            ingested: 0,
            files: vec![],
        });
    }

    // Compute safe next ID.
    let max_existing_id: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM articles WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let meta_next_id: i64 = conn
        .query_row(
            "SELECT next_article_id FROM articles_meta WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let mut next_id = std::cmp::max(max_existing_id + 1, meta_next_id.max(1));

    let mut ingested_files = Vec::new();

    // Collect existing dates for duplicate detection.
    let existing_dates: std::collections::HashSet<String> = articles
        .iter()
        .filter_map(|a| a.published_date.clone())
        .filter(|d| !d.is_empty())
        .collect();
    let today = chrono::Utc::now().date_naive();

    for (basename, file_path) in orphans {
        let meta = match crate::content::ops::read_file_metadata(&file_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Warn if the ingested date is duplicate or future.
        if let Some(ref date_str) = meta.published_date {
            if !date_str.is_empty() {
                let is_duplicate = existing_dates.contains(date_str);
                let is_future = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    .map(|d| d > today)
                    .unwrap_or(false);
                if is_duplicate || is_future {
                    log::warn!(
                        "[ingest_orphans] {} has {} date: {} (will be fixed by post-step enforcement)",
                        basename,
                        if is_duplicate { "duplicate" } else { "future" },
                        date_str
                    );
                }
            }
        }

        let url_slug = derive_url_slug(&basename);
        let title = meta.title.unwrap_or_else(|| url_slug.replace('-', " "));
        let content_rel = content_dir
            .strip_prefix(project_path)
            .unwrap_or(std::path::Path::new("content"))
            .to_string_lossy()
            .replace('\\', "/");
        let file_ref = format!("./{}/{}", content_rel, basename);

        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                content_gaps_addressed, estimated_traffic_monthly, project_id
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![
                next_id,
                title,
                url_slug,
                file_ref,
                Option::<String>::None,
                Option::<String>::None,
                0i64,
                meta.published_date,
                meta.word_count as i64,
                "published",
                "[]",
                Option::<String>::None,
                project_id,
            ],
        )?;

        ingested_files.push(basename);
        next_id += 1;
    }

    if ingested_files.is_empty() {
        return Ok(IngestSummary {
            ingested: 0,
            files: vec![],
        });
    }

    // Update articles_meta.
    conn.execute(
        "INSERT INTO articles_meta (project_id, next_article_id)
         VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET next_article_id = excluded.next_article_id",
        rusqlite::params![project_id, next_id],
    )?;

    // Export projection.
    crate::db::export::write_articles_to_repo(conn, project_id, project_path)?;

    Ok(IngestSummary {
        ingested: ingested_files.len(),
        files: ingested_files,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sidecar metadata
// ═══════════════════════════════════════════════════════════════════════════════

/// Store sidecar metadata for an article under a namespace.
///
/// Example namespace: `"gsc"`, `"quality"`, `"analytics"`, `"custom"`.
/// The payload must be valid JSON.
pub fn set_metadata(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    namespace: &str,
    payload: &str,
) -> Result<()> {
    crate::db::set_article_metadata(conn, project_id, article_id, namespace, payload)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Derive a URL slug from an MDX filename.
/// "242_pour_over_coffee_cafes_auckland.mdx" → "pour-over-coffee-cafes-auckland"
#[allow(dead_code)]
fn derive_url_slug(filename: &str) -> String {
    let base = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    crate::content::slug::strip_numeric_prefix(base)
        .to_lowercase()
        .replace('_', "-")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn setup_project(conn: &Connection, project_id: &str, path: &std::path::Path) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, ?2, ?3, 1, 'workspace')",
            [project_id, "Test Project", path.to_str().unwrap()],
        )
        .unwrap();
    }

    fn write_mdx(path: &std::path::Path, title: &str, date: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = format!(
            "---\ntitle: \"{}\"\ndate: \"{}\"\n---\n\nBody text.\n",
            title, date
        );
        std::fs::write(path, content).unwrap();
    }

    fn write_seo_workspace(automation_dir: &std::path::Path, content_dir: &str) {
        std::fs::create_dir_all(automation_dir).unwrap();
        let cfg = format!(r#"{{"content_dir":"{}"}}"#, content_dir);
        std::fs::write(automation_dir.join("seo_workspace.json"), cfg).unwrap();
    }

    #[test]
    fn clean_stale_removes_from_db_and_exports_json() {
        let dir = unique_temp_dir("ps_ai_clean");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        write_seo_workspace(&auto_dir, "content");

        // Article exists in DB but file is missing on disk
        let conn = in_memory_db();
        setup_project(&conn, "p1", &dir);
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, content_gaps_addressed, project_id)
             VALUES (1, 'Gone', 'gone', './content/001_gone.mdx', 'draft', '[]', 'p1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES ('p1', 2)",
            [],
        )
        .unwrap();

        // Write a stale articles.json that still has the article
        std::fs::write(
            auto_dir.join("articles.json"),
            r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Gone","file":"./content/001_gone.mdx","status":"draft"}]}"#,
        )
        .unwrap();

        let summary = clean_stale_articles(&conn, "p1", &dir).unwrap();
        assert_eq!(summary.removed.len(), 1);
        assert!(summary.db_cleaned);
        assert!(summary.json_cleaned);

        // DB row should be gone
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles WHERE project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        // JSON should also be clean
        let json = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(doc["articles"].as_array().unwrap().is_empty());
    }

    #[test]
    fn ingest_orphans_inserts_into_db_and_exports_json() {
        let dir = unique_temp_dir("ps_ai_ingest");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        write_seo_workspace(&auto_dir, "content");

        // An MDX file exists but is not in DB
        write_mdx(
            &content_dir.join("001_new.mdx"),
            "New Article",
            "2026-02-01",
        );

        let conn = in_memory_db();
        setup_project(&conn, "p1", &dir);
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES ('p1', 1)",
            [],
        )
        .unwrap();

        let summary = ingest_orphans(&conn, "p1", &dir).unwrap();
        assert_eq!(summary.ingested, 1);
        assert_eq!(summary.files, vec!["001_new.mdx"]);

        // DB should have the article
        let title: String = conn
            .query_row(
                "SELECT title FROM articles WHERE project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "New Article");

        // JSON should have it too
        let json = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(doc["articles"].as_array().unwrap().len(), 1);
    }
}
