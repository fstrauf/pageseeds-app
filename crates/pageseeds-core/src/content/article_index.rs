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

/// Catalog article that already owns a given exact `target_keyword`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KeywordCollider {
    pub id: i64,
    pub title: String,
    pub url_slug: String,
    pub page_type: Option<String>,
    pub file: String,
}

/// Find catalog articles whose normalized `target_keyword` equals `keyword`.
///
/// Empty/whitespace keyword after [`crate::content::keyword_match::normalize_keyword`]
/// returns an empty list (no collision gate). Optional `exclude_slug` /
/// `exclude_article_id` skip the article being re-submitted or re-verified so
/// self does not false-positive (issue #272).
pub fn find_target_keyword_collisions(
    conn: &Connection,
    project_id: &str,
    keyword: &str,
    exclude_slug: Option<&str>,
    exclude_article_id: Option<i64>,
) -> Result<Vec<KeywordCollider>> {
    let normalized = crate::content::keyword_match::normalize_keyword(keyword);
    if normalized.is_empty() {
        return Ok(vec![]);
    }

    let exclude_slug_norm = exclude_slug
        .map(|s| crate::content::slug::normalize_url_slug(s))
        .filter(|s| !s.is_empty());

    let articles = list_articles(conn, project_id)?;
    let mut colliders = Vec::new();
    for a in articles {
        if let Some(id) = exclude_article_id {
            if a.id == id {
                continue;
            }
        }
        if let Some(ref ex) = exclude_slug_norm {
            let article_slug = crate::content::slug::normalize_url_slug(&a.url_slug);
            if article_slug == *ex {
                continue;
            }
        }
        let Some(kw) = a.target_keyword.as_deref() else {
            continue;
        };
        let other = crate::content::keyword_match::normalize_keyword(kw);
        if other.is_empty() {
            continue;
        }
        if other == normalized {
            colliders.push(KeywordCollider {
                id: a.id,
                title: a.title,
                url_slug: a.url_slug,
                page_type: a.page_type,
                file: a.file,
            });
        }
    }
    Ok(colliders)
}

/// Operator-facing resolution text for exact `target_keyword` collisions (issue #272).
///
/// Names collider identity and the two allowed fixes: retarget or consolidate.
/// Does not auto-redirect, rewrite inbound, or spawn consolidate.
pub fn format_keyword_collision_message(keyword: &str, colliders: &[KeywordCollider]) -> String {
    let collider_list = colliders
        .iter()
        .map(|c| {
            let page = c.page_type.as_deref().unwrap_or("unknown");
            format!(
                "id={} slug={} title=\"{}\" page_type={}",
                c.id, c.url_slug, c.title, page
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "Exact target_keyword collision for \"{}\": already owned by [{}]. \
         Resolve by (1) Retarget the existing article or this draft to a non-colliding keyword, or \
         (2) Consolidate via cannibalization audit → approve consolidate_cluster with keep = intended winner \
         and redirect the loser. Write registration will not create a silent hub/spoke twin.",
        keyword.trim(),
        collider_list
    )
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

/// Update `articles.target_keyword` and re-export the catalog projection.
///
/// Uses [`crate::content::keyword_match::normalize_keyword`] (not GSC's 5-word
/// backfill normalizer). Empty after normalize is a no-op — does not clear the
/// existing catalog value. Returns whether a row was written.
pub fn apply_catalog_target_keyword(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    keyword: &str,
    project_path: &Path,
) -> bool {
    if article_id == 0 {
        return false;
    }
    let normalized = crate::content::keyword_match::normalize_keyword(keyword);
    if normalized.is_empty() {
        return false;
    }
    let updated = conn
        .execute(
            "UPDATE articles SET target_keyword=?1 WHERE id=?2 AND project_id=?3",
            rusqlite::params![&normalized, article_id, project_id],
        )
        .unwrap_or(0);
    if updated == 0 {
        return false;
    }
    let _ = export_projection(conn, project_id, project_path);
    true
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
            // Explicit evidence purge before article delete (belt-and-suspenders
            // with V49 ON DELETE CASCADE for soft-clean / FK-off edge cases).
            let _ = conn.execute(
                "DELETE FROM article_evidence WHERE article_id = ?1 AND project_id = ?2",
                rusqlite::params![article.id, project_id],
            );
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

        let url_slug = {
            let stem = std::path::Path::new(&basename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&basename);
            crate::content::slug::normalize_url_slug(stem)
        };
        let title = meta.title.unwrap_or_else(|| url_slug.replace('-', " "));
        let content_rel = content_dir
            .strip_prefix(project_path)
            .unwrap_or(std::path::Path::new("content"))
            .to_string_lossy()
            .replace('\\', "/");
        let file_ref = format!("./{}/{}", content_rel, basename);

        // Compute content hash from file content
        let content_hash = std::fs::read_to_string(&file_path).ok().map(|content| {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        });

        // Get file modification time for last_edited_at
        let last_edited_at = std::fs::metadata(&file_path)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| {
                chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()
            });

        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                content_gaps_addressed, estimated_traffic_monthly, project_id,
                content_hash, last_edited_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
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
                content_hash,
                last_edited_at,
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

    // Best-effort evidence facts for newly ingested articles (never fails ingest).
    for basename in &ingested_files {
        let stem = std::path::Path::new(basename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(basename);
        let slug = crate::content::slug::normalize_url_slug(stem);
        crate::content::article_evidence::maybe_reindex_article(
            conn,
            project_id,
            project_path,
            &slug,
        );
    }

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

    fn insert_article_with_keyword(
        conn: &Connection,
        id: i64,
        project_id: &str,
        slug: &str,
        title: &str,
        keyword: Option<&str>,
        page_type: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, status,
                content_gaps_addressed, project_id, page_type
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', '[]', ?6, ?7)",
            rusqlite::params![
                id,
                title,
                slug,
                format!("./content/{slug}.mdx"),
                keyword,
                project_id,
                page_type,
            ],
        )
        .unwrap();
    }

    /// Issue #272: exact normalized keyword match returns collider identity.
    #[test]
    fn find_target_keyword_collisions_matches_normalized_keyword() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_ai_kw_col");
        setup_project(&conn, "p1", &dir);

        insert_article_with_keyword(
            &conn,
            1,
            "p1",
            "hub-seo-tools",
            "SEO Tools Hub",
            Some("\"SEO Tools\""),
            Some("hub"),
        );
        insert_article_with_keyword(
            &conn,
            2,
            "p1",
            "other",
            "Other",
            Some("different keyword"),
            None,
        );

        let hits = find_target_keyword_collisions(&conn, "p1", "seo tools", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 1);
        assert_eq!(hits[0].url_slug, "hub-seo-tools");
        assert_eq!(hits[0].title, "SEO Tools Hub");
        assert_eq!(hits[0].page_type.as_deref(), Some("hub"));

        let msg = format_keyword_collision_message("seo tools", &hits);
        assert!(msg.contains("hub-seo-tools"), "msg={msg}");
        assert!(msg.contains("Retarget"), "msg={msg}");
        assert!(msg.contains("Consolidate") || msg.contains("consolidate_cluster"), "msg={msg}");
    }

    /// Issue #272: re-submit/re-verify of the same article does not false-positive.
    #[test]
    fn find_target_keyword_collisions_excludes_self_by_slug_and_id() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_ai_kw_self");
        setup_project(&conn, "p1", &dir);

        insert_article_with_keyword(
            &conn,
            10,
            "p1",
            "seo-tools",
            "SEO Tools",
            Some("seo tools"),
            Some("spoke"),
        );

        let by_slug =
            find_target_keyword_collisions(&conn, "p1", "seo tools", Some("seo-tools"), None)
                .unwrap();
        assert!(by_slug.is_empty(), "exclude_slug should skip self: {by_slug:?}");

        let by_id =
            find_target_keyword_collisions(&conn, "p1", "seo tools", None, Some(10)).unwrap();
        assert!(by_id.is_empty(), "exclude_article_id should skip self: {by_id:?}");

        // Different slug still collides.
        let other =
            find_target_keyword_collisions(&conn, "p1", "seo tools", Some("twin-slug"), None)
                .unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].id, 10);
    }

    /// Issue #272: empty/whitespace keyword never trips the collision gate.
    #[test]
    fn find_target_keyword_collisions_empty_keyword_is_noop() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_ai_kw_empty");
        setup_project(&conn, "p1", &dir);

        insert_article_with_keyword(
            &conn,
            1,
            "p1",
            "owned",
            "Owned",
            Some("seo tools"),
            None,
        );

        for empty in ["", "   ", "\"\"", "  \t  "] {
            let hits = find_target_keyword_collisions(&conn, "p1", empty, None, None).unwrap();
            assert!(
                hits.is_empty(),
                "empty keyword {empty:?} must not collide, got {hits:?}"
            );
        }
    }
}
