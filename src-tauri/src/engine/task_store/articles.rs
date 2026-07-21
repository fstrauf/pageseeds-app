use rusqlite::Connection;

use crate::error::Result;

// ─── Article queries ──────────────────────────────────────────────────────────

use crate::models::article::Article;

pub fn list_articles(conn: &Connection, project_id: &str) -> Result<Vec<Article>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                review_status, review_started_at, last_reviewed_at, review_count,
                content_gaps_addressed, estimated_traffic_monthly, page_type,
                content_hash, last_edited_at
         FROM articles WHERE project_id = ?1 ORDER BY id ASC",
    )?;
    let articles: Vec<Article> = stmt
        .query_map([project_id], |row| {
            let gaps_str: String = row.get(14)?;
            let gaps: Vec<String> = serde_json::from_str(&gaps_str).unwrap_or_default();
            Ok(Article {
                id: row.get(0)?,
                title: row.get(1)?,
                url_slug: row.get(2)?,
                file: row.get(3)?,
                target_keyword: row.get(4)?,
                keyword_difficulty: row.get(5)?,
                target_volume: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                published_date: row.get(7)?,
                word_count: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                status: row.get(9)?,
                review_status: row.get(10)?,
                review_started_at: row.get(11)?,
                last_reviewed_at: row.get(12)?,
                review_count: row.get::<_, Option<i64>>(13)?.unwrap_or(0),
                content_gaps_addressed: gaps,
                estimated_traffic_monthly: row.get(15)?,
                page_type: row.get(16)?,
                content_hash: row.get(17)?,
                last_edited_at: row.get(18)?,
                project_id: project_id.to_string(),
                quality_score: None,
                quality_grade: None,
                quality_rated_at: None,
                publishing_ready: None,
                quality_breakdown: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(articles)
}

/// Load all article url_slugs for a project as a normalized, lowercased HashSet.
///
/// This is the single source of truth for "does this slug exist in the project?"
/// checks. Use it instead of re-implementing the list_articles → collect pattern
/// in every module that validates internal link targets.
///
/// Slugs are normalized via [`crate::content::slug::normalize_url_slug`] so that
/// callers can match against canonical values regardless of whether the database
/// stores raw slugs (`hub-coffee`), prefixed slugs (`blog/hub-coffee`), or slugs
/// with leading/trailing slashes.
///
/// # Example
/// ```no_run
/// use pageseeds_lib::engine::task_store::load_project_slug_set;
///
/// let conn = rusqlite::Connection::open_in_memory().unwrap();
/// let slugs = load_project_slug_set(&conn, "proj-1").unwrap();
/// assert!(slugs.contains("my-post"));
/// ```
pub fn load_project_slug_set(
    conn: &Connection,
    project_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let articles = list_articles(conn, project_id)?;
    Ok(articles
        .into_iter()
        .map(|a| crate::content::slug::normalize_url_slug(&a.url_slug))
        .collect())
}

/// Load the set of slugs that are valid internal link targets for a project.
///
/// This is [`load_project_slug_set`] minus the slugs that have been redirected
/// away by a consolidation (sources in `.github/automation/redirects.csv`):
/// a redirected slug may still have a row in SQLite and a file on disk, but it
/// no longer resolves to a live article, so nothing may link to it.
///
/// This is the single place the "valid link target" set is computed — every
/// link validator (cluster_link apply, fix_generate, indexing_link apply,
/// link verify, content audit) must use it instead of the raw slug set.
pub fn load_valid_link_targets(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<std::collections::HashSet<String>> {
    let mut slugs = load_project_slug_set(conn, project_id)?;
    let redirected = crate::content::redirects::load_redirect_source_slugs(project_path);
    if !redirected.is_empty() {
        slugs.retain(|slug| !redirected.contains(slug));
    }
    Ok(slugs)
}
