/// Publish articles workflow — deterministic pre-flight + apply.
///
/// Replaces the agentic `PublishingRunner` from the Python CLI with a fully
/// deterministic Rust implementation. The only agentic call retained is for
/// title/year mismatch resolution, where editorial judgment is genuinely needed.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{Datelike, NaiveDate, Utc};
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::content::{cleaner, dates};
use crate::engine::task_store;
use crate::models::article::Article;

// ─── Public result types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ArticleWithIssue {
    pub article: Article,
    pub issue: String,
}

#[derive(Debug, Serialize)]
pub struct YearMismatch {
    pub article_id: i64,
    pub title: String,
    pub title_year: i32,
    pub publish_year: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct YearMismatchResolution {
    pub article_id: i64,
    /// "update_title" or "backdate"
    pub action: String,
    /// New title string (for update_title) or new date string YYYY-MM-DD (for backdate)
    pub new_value: String,
}

#[derive(Debug, Serialize)]
pub struct PublishPreflightResult {
    pub ready: Vec<Article>,
    pub needs_date_fix: Vec<ArticleWithIssue>,
    pub year_mismatches: Vec<YearMismatch>,
    pub blocked: Vec<ArticleWithIssue>,
    pub structural_issue_count: usize,
}

#[derive(Debug, Serialize)]
pub struct PublishedArticle {
    pub id: i64,
    pub title: String,
    pub published_date: String,
}

#[derive(Debug, Serialize)]
pub struct PublishResult {
    pub published: Vec<PublishedArticle>,
    pub skipped: Vec<ArticleWithIssue>,
    pub errors: Vec<String>,
}

// ─── Pre-flight ───────────────────────────────────────────────────────────────

/// Run all pre-flight checks. Never writes anything.
///
/// Accepts the articles to check (already filtered to draft/ready_to_publish)
/// and the full project article list (for duplicate-date detection).
pub fn preflight(
    candidates: &[Article],
    all_articles: &[Article],
    content_dir: &Path,
) -> PublishPreflightResult {
    // Structural scan (dry-run only).
    let structural_issues =
        cleaner::scan_and_clean(content_dir, true).unwrap_or_else(|_| cleaner::CleaningResult {
            files_checked: 0,
            issues: vec![],
            issues_fixed: 0,
        });
    let structural_issue_count = structural_issues.issues.len();

    // Date analysis for ALL articles — needed to detect future-dated articles.
    let date_analysis = dates::analyse_dates(all_articles);

    // Collect article_ids that have date issues.
    let date_issue_ids: HashSet<i64> = date_analysis
        .issues
        .iter()
        .filter(|i| i.issue_type == "future_date")
        .map(|i| i.article_id)
        .collect();

    // Build content-file map: basename → exists.
    let content_files: HashSet<String> =
        crate::content::locator::collect_markdown_files(content_dir)
            .into_iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
            .collect();

    let mut ready = Vec::new();
    let mut needs_date_fix = Vec::new();
    let mut year_mismatches = Vec::new();
    let mut blocked = Vec::new();

    for article in candidates {
        // 1. File existence check.
        let basename = std::path::Path::new(&article.file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if basename.is_empty() || !content_files.contains(&basename) {
            blocked.push(ArticleWithIssue {
                article: article.clone(),
                issue: format!("Content file not found: {}", article.file),
            });
            continue;
        }

        // 2. Date issue check.
        if date_issue_ids.contains(&article.id) {
            let issue = date_analysis
                .issues
                .iter()
                .find(|i| i.article_id == article.id)
                .map(|i| i.description.clone())
                .unwrap_or_else(|| "Date issue".into());

            needs_date_fix.push(ArticleWithIssue {
                article: article.clone(),
                issue,
            });
            continue;
        }

        // 3. Year mismatch check.
        if let Some(mismatch) = detect_year_mismatch(article) {
            year_mismatches.push(mismatch);
            continue;
        }

        // 4. No issues — ready to publish.
        ready.push(article.clone());
    }

    PublishPreflightResult {
        ready,
        needs_date_fix,
        year_mismatches,
        blocked,
        structural_issue_count,
    }
}

// ─── Apply publish ────────────────────────────────────────────────────────────

/// Apply all fixes and transition statuses to "published".
///
/// `date_fixes` maps article_id (as string) → new date string.
/// `resolutions` is the list of agent-supplied year-mismatch resolutions.
/// After updating SQLite, patches MDX frontmatter dates and writes articles.json.
pub fn apply_publish(
    conn: &Connection,
    project_id: &str,
    article_ids: &[i64],
    date_fixes: &HashMap<String, String>,
    resolutions: &[YearMismatchResolution],
    content_dir: &Path,
    project_path: &Path,
) -> PublishResult {
    let mut published = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();

    // Resolve year-mismatch map keyed by article_id.
    let resolution_map: HashMap<i64, &YearMismatchResolution> =
        resolutions.iter().map(|r| (r.article_id, r)).collect();

    // Apply date fixes to SQLite first.
    for (id_str, new_date) in date_fixes {
        if let Ok(id) = id_str.parse::<i64>() {
            if let Err(e) = conn.execute(
                "UPDATE articles SET published_date = ?1 WHERE id = ?2 AND project_id = ?3",
                rusqlite::params![new_date, id, project_id],
            ) {
                errors.push(format!("Failed to apply date fix for article {id}: {e}"));
            }
        }
    }

    // Apply year-mismatch resolutions to SQLite.
    for resolution in resolutions {
        let id = resolution.article_id;
        match resolution.action.as_str() {
            "update_title" => {
                if let Err(e) = conn.execute(
                    "UPDATE articles SET title = ?1 WHERE id = ?2 AND project_id = ?3",
                    rusqlite::params![resolution.new_value, id, project_id],
                ) {
                    errors.push(format!("Failed to update title for article {id}: {e}"));
                }
            }
            "backdate" => {
                if let Err(e) = conn.execute(
                    "UPDATE articles SET published_date = ?1 WHERE id = ?2 AND project_id = ?3",
                    rusqlite::params![resolution.new_value, id, project_id],
                ) {
                    errors.push(format!("Failed to backdate article {id}: {e}"));
                }
            }
            other => {
                errors.push(format!(
                    "Unknown year mismatch action '{other}' for article {id}"
                ));
            }
        }
    }

    // Reload all articles to compute safe dates for those without a date.
    let all_articles = task_store::list_articles(conn, project_id).unwrap_or_default();
    let today = Utc::now().date_naive();

    // Collect occupied dates (from articles NOT being processed here).
    let mut occupied: HashSet<NaiveDate> = all_articles
        .iter()
        .filter(|a| !article_ids.contains(&a.id))
        .filter_map(|a| a.published_date.as_deref())
        .filter_map(|ds| NaiveDate::parse_from_str(ds, "%Y-%m-%d").ok())
        .collect();

    // Identify batch articles that still have date issues (future dates) after
    // any explicit date_fixes have been applied. These must be auto-reassigned
    // rather than using their stored (bad) date — which would publish
    // future-dated articles as-is and block the articles.json export.
    let date_analysis = dates::analyse_dates(&all_articles);
    let needs_reassign: HashSet<i64> = date_analysis
        .issues
        .iter()
        .filter(|i| i.issue_type == "future_date")
        .filter(|i| article_ids.contains(&i.article_id))
        .map(|i| i.article_id)
        .collect();

    // Track dates we assign during this publish run to avoid self-collisions.
    let mut assigned_dates: HashSet<NaiveDate> = HashSet::new();

    // Publish each article.
    for &id in article_ids {
        let article = match all_articles.iter().find(|a| a.id == id) {
            Some(a) => a,
            None => {
                errors.push(format!("Article {id} not found"));
                continue;
            }
        };

        // Determine the final published_date.
        let publish_date: String = if needs_reassign.contains(&id) {
            // Date is problematic (future) — auto-assign the most
            // recent free past date, skipping everything already occupied.
            assign_free_date(today, &occupied, &assigned_dates)
        } else if let Some(d_str) = article.published_date.as_deref().filter(|s| !s.is_empty()) {
            // Already has a clean date (not flagged as future).
            d_str.to_string()
        } else if let Some(resolution) = resolution_map.get(&id) {
            if resolution.action == "backdate" {
                resolution.new_value.clone()
            } else {
                // update_title resolution — still need a date
                assign_free_date(today, &occupied, &assigned_dates)
            }
        } else {
            // No date at all — assign the most recent free past date.
            assign_free_date(today, &occupied, &assigned_dates)
        };

        // Register the date as used so subsequent articles don't collide.
        if let Ok(d) = NaiveDate::parse_from_str(&publish_date, "%Y-%m-%d") {
            occupied.insert(d);
            assigned_dates.insert(d);
        }

        // Update SQLite: set status = "published" and ensure date is set.
        if let Err(e) = conn.execute(
            "UPDATE articles SET status = 'published', published_date = ?1 WHERE id = ?2 AND project_id = ?3",
            rusqlite::params![publish_date, id, project_id],
        ) {
            skipped.push(ArticleWithIssue {
                article: article.clone(),
                issue: format!("DB update failed: {e}"),
            });
            continue;
        }

        published.push(PublishedArticle {
            id,
            title: article.title.clone(),
            published_date: publish_date,
        });
    }

    // Fix structural issues in content files.
    let _ = cleaner::scan_and_clean(content_dir, false);

    // Patch MDX frontmatter dates from SQLite (canonical runtime source of truth).
    let automation_dir = project_path.join(".github").join("automation");
    if let Err(e) = crate::content::ops::sync_and_validate(
        &automation_dir,
        project_path,
        true,
        conn,
        project_id,
    ) {
        errors.push(format!("MDX frontmatter sync warning: {e}"));
    }

    // Export the updated SQLite state to articles.json so the projection stays in sync.
    if let Err(e) = crate::content::article_index::export_projection(conn, project_id, project_path)
    {
        errors.push(format!("Failed to write articles.json: {e}"));
    }

    PublishResult {
        published,
        skipped,
        errors,
    }
}

// ─── Slug-oriented publish entry (CLI Path B second step, issue #257) ─────────

/// Per-slug outcome when a candidate is not applied (skip / block / year mismatch).
#[derive(Debug, Serialize)]
pub struct PublishSlugItem {
    pub slug: String,
    pub article_id: Option<i64>,
    pub title: Option<String>,
    pub catalog_status: Option<String>,
    pub reason: String,
}

/// Successfully published (or already-published no-op when listed under skipped).
#[derive(Debug, Serialize)]
pub struct PublishSlugPublished {
    pub slug: String,
    pub article_id: i64,
    pub title: String,
    pub published_date: String,
    pub catalog_status: String,
}

/// Title/year mismatch left unresolved in v1 (status unchanged).
#[derive(Debug, Serialize)]
pub struct PublishSlugYearMismatch {
    pub slug: String,
    pub article_id: i64,
    pub title: String,
    pub title_year: i32,
    pub publish_year: i32,
    pub catalog_status: String,
    pub reason: String,
}

/// JSON-friendly result of [`publish_by_slugs`].
#[derive(Debug, Serialize)]
pub struct PublishBySlugsResult {
    pub ok: bool,
    pub published: Vec<PublishSlugPublished>,
    pub skipped: Vec<PublishSlugItem>,
    pub blocked: Vec<PublishSlugItem>,
    pub year_mismatches: Vec<PublishSlugYearMismatch>,
    pub errors: Vec<String>,
}

/// Publish catalog articles by URL slug: resolve → preflight → apply.
///
/// Accepts only `draft` / `ready_to_publish` candidates. Already-`published`
/// is a no-op skip (not an error). Missing slugs and non-publishable statuses
/// are structured errors. Year mismatches are reported and left unchanged
/// (no LLM resolution in v1). `needs_date_fix` articles are passed to
/// [`apply_publish`] with empty date_fixes so apply auto-assigns dates.
///
/// Content dir is `project_path.join("content")`.
pub fn publish_by_slugs(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    slugs: &[String],
) -> Result<PublishBySlugsResult, String> {
    let mut cleaned: Vec<String> = Vec::new();
    for raw in slugs {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        // Allow comma-separated values inside a single list entry.
        for part in s.split(',') {
            let p = part.trim();
            if !p.is_empty() {
                cleaned.push(p.to_string());
            }
        }
    }
    if cleaned.is_empty() {
        return Err("at least one slug is required".to_string());
    }

    let content_dir = project_path.join("content");
    let all_articles = task_store::list_articles(conn, project_id).map_err(|e| e.to_string())?;

    let mut published = Vec::new();
    let mut skipped = Vec::new();
    let mut blocked = Vec::new();
    let mut year_mismatches = Vec::new();
    let mut errors = Vec::new();
    let mut candidates: Vec<Article> = Vec::new();
    // Preserve first occurrence order; skip duplicate slug requests.
    let mut seen_ids: HashSet<i64> = HashSet::new();
    let mut seen_request_slugs: HashSet<String> = HashSet::new();

    for slug in &cleaned {
        let slug_norm = crate::content::slug::normalize_url_slug(slug);
        if !seen_request_slugs.insert(slug_norm.clone()) {
            continue;
        }

        let article = all_articles.iter().find(|a| {
            a.url_slug == *slug
                || crate::content::slug::normalize_url_slug(&a.url_slug) == slug_norm
        });

        let Some(article) = article else {
            errors.push(format!("No article found for slug '{slug}'"));
            continue;
        };

        if !seen_ids.insert(article.id) {
            continue;
        }

        match article.status.as_str() {
            "published" => {
                skipped.push(PublishSlugItem {
                    slug: article.url_slug.clone(),
                    article_id: Some(article.id),
                    title: Some(article.title.clone()),
                    catalog_status: Some(article.status.clone()),
                    reason: "already published".to_string(),
                });
            }
            "draft" | "ready_to_publish" => {
                candidates.push(article.clone());
            }
            other => {
                errors.push(format!(
                    "slug '{}' has catalog status '{other}' (only draft or ready_to_publish can be published)",
                    article.url_slug
                ));
            }
        }
    }

    if !candidates.is_empty() {
        let preflight_result = preflight(&candidates, &all_articles, &content_dir);

        for ym in &preflight_result.year_mismatches {
            let art = candidates
                .iter()
                .find(|a| a.id == ym.article_id)
                .or_else(|| all_articles.iter().find(|a| a.id == ym.article_id));
            year_mismatches.push(PublishSlugYearMismatch {
                slug: art.map(|a| a.url_slug.clone()).unwrap_or_default(),
                article_id: ym.article_id,
                title: ym.title.clone(),
                title_year: ym.title_year,
                publish_year: ym.publish_year,
                catalog_status: art
                    .map(|a| a.status.clone())
                    .unwrap_or_else(|| "draft".into()),
                reason: format!(
                    "title year {} vs publish year {} — leave status unchanged (no agent resolution in v1)",
                    ym.title_year, ym.publish_year
                ),
            });
        }

        for b in &preflight_result.blocked {
            blocked.push(PublishSlugItem {
                slug: b.article.url_slug.clone(),
                article_id: Some(b.article.id),
                title: Some(b.article.title.clone()),
                catalog_status: Some(b.article.status.clone()),
                reason: b.issue.clone(),
            });
        }

        // ready + needs_date_fix → apply (empty date_fixes / resolutions).
        let mut apply_ids: Vec<i64> = preflight_result.ready.iter().map(|a| a.id).collect();
        apply_ids.extend(preflight_result.needs_date_fix.iter().map(|a| a.article.id));

        if !apply_ids.is_empty() {
            let apply_result = apply_publish(
                conn,
                project_id,
                &apply_ids,
                &HashMap::new(),
                &[],
                &content_dir,
                project_path,
            );

            // Map published rows back to slugs.
            let post_articles =
                task_store::list_articles(conn, project_id).unwrap_or_else(|_| all_articles.clone());
            let by_id: HashMap<i64, &Article> =
                post_articles.iter().map(|a| (a.id, a)).collect();

            for p in apply_result.published {
                let slug = by_id
                    .get(&p.id)
                    .map(|a| a.url_slug.clone())
                    .unwrap_or_default();
                let catalog_status = by_id
                    .get(&p.id)
                    .map(|a| a.status.clone())
                    .unwrap_or_else(|| "published".into());
                published.push(PublishSlugPublished {
                    slug,
                    article_id: p.id,
                    title: p.title,
                    published_date: p.published_date,
                    catalog_status,
                });
            }

            for s in apply_result.skipped {
                skipped.push(PublishSlugItem {
                    slug: s.article.url_slug.clone(),
                    article_id: Some(s.article.id),
                    title: Some(s.article.title.clone()),
                    catalog_status: Some(s.article.status.clone()),
                    reason: s.issue,
                });
            }

            errors.extend(apply_result.errors);
        }
    }

    let ok = errors.is_empty()
        && blocked.is_empty()
        && year_mismatches.is_empty()
        && (!published.is_empty() || !skipped.is_empty());

    Ok(PublishBySlugsResult {
        ok,
        published,
        skipped,
        blocked,
        year_mismatches,
        errors,
    })
}

// ─── Agent call for year mismatch ─────────────────────────────────────────────

/// Call the configured LLM agent to decide how to resolve a title/year mismatch.
///
/// Returns a `YearMismatchResolution` with `action = "update_title" | "backdate"`.
pub fn resolve_year_mismatch_with_agent(
    provider: &str,
    article_id: i64,
    title: &str,
    title_year: i32,
    publish_year: i32,
    project_path: &Path,
    all_articles: &[Article],
) -> Result<YearMismatchResolution, String> {
    let gap = publish_year - title_year;

    // Build existing occupied dates for backdate safety note.
    let occupied: Vec<String> = all_articles
        .iter()
        .filter(|a| a.id != article_id)
        .filter_map(|a| a.published_date.clone())
        .collect();
    let occupied_note = if occupied.is_empty() {
        String::new()
    } else {
        format!("\nOccupied dates (do not use): {}", occupied.join(", "))
    };

    let prompt = format!(
        r#"You are resolving a year mismatch for an SEO article.

Article title: "{title}"
Title mentions year: {title_year}
Intended publish date year: {publish_year}
Year gap: {gap} year(s){occupied_note}

Choose one action:
A) Update the title to use year {publish_year} (update_title)
B) Backdate the publish date to {title_year}-01-01 or another date in {title_year} (backdate)

Rules:
- Prefer update_title if the content is evergreen or the topic is still current in {publish_year}
- Prefer backdate if the article is specifically about events or data from {title_year}
- The backdated date must not conflict with any occupied date listed above
- For backdate, pick a specific YYYY-MM-DD date in {title_year}

Respond with ONLY valid JSON and nothing else:
{{"action": "update_title", "new_value": "updated title here"}}
OR
{{"action": "backdate", "new_value": "YYYY-MM-DD"}}"#
    );

    let raw = crate::engine::agent::run_agent(provider, &prompt, project_path)?;

    // Extract the JSON object from the response (agent may include prose before/after).
    let json_str = crate::engine::text::extract_json_string(&raw).ok_or_else(|| {
        format!(
            "Agent response did not contain a JSON object. Got: {}",
            raw.trim()
        )
    })?;

    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse agent JSON: {e}. Raw: {json_str}"))?;

    let action = parsed["action"]
        .as_str()
        .ok_or_else(|| format!("Missing 'action' field in agent response: {json_str}"))?
        .to_string();

    let new_value = parsed["new_value"]
        .as_str()
        .ok_or_else(|| format!("Missing 'new_value' field in agent response: {json_str}"))?
        .to_string();

    if action != "update_title" && action != "backdate" {
        return Err(format!(
            "Unknown action '{action}' from agent. Expected update_title or backdate."
        ));
    }

    Ok(YearMismatchResolution {
        article_id,
        action,
        new_value,
    })
}

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Detect a year mismatch between the article title and the publish year.
/// Only flags if:
/// - The title contains a 4-digit year >= 2000
/// - The publish year (from published_date or today's year) exceeds the title year by > 1
fn detect_year_mismatch(article: &Article) -> Option<YearMismatch> {
    let re = Regex::new(r"\b(20\d{2})\b").unwrap();
    let title_years: Vec<i32> = re
        .find_iter(&article.title)
        .filter_map(|m| m.as_str().parse::<i32>().ok())
        .collect();

    if title_years.is_empty() {
        return None;
    }

    // Use the latest year mentioned in the title.
    let title_year = *title_years.iter().max()?;

    let publish_year = article
        .published_date
        .as_deref()
        .and_then(|ds| NaiveDate::parse_from_str(ds, "%Y-%m-%d").ok())
        .map(|d| d.year())
        .unwrap_or_else(|| Utc::now().date_naive().year());

    if publish_year - title_year > 1 {
        Some(YearMismatch {
            article_id: article.id,
            title: article.title.clone(),
            title_year,
            publish_year,
        })
    } else {
        None
    }
}

/// Find the most recent free past date (i.e. not in `occupied` or `assigned`).
///
/// Delegates to `date_policy::find_first_free_past_date` — the single source of truth.
fn assign_free_date(
    today: NaiveDate,
    occupied: &HashSet<NaiveDate>,
    assigned: &HashSet<NaiveDate>,
) -> String {
    let mut merged: HashSet<NaiveDate> = occupied.iter().copied().collect();
    merged.extend(assigned.iter().copied());
    crate::content::date_policy::find_first_free_past_date(today, &merged)
        .format("%Y-%m-%d")
        .to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regression tests for publish date consistency (Phase 5)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn in_memory_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn write_mdx(path: &std::path::Path, title: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = format!("---\ntitle: \"{}\"\n---\n\nBody text.\n", title);
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn apply_publish_keeps_mdx_json_and_db_dates_consistent() {
        let dir = unique_temp_dir("ps_publish_consistent");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        // Write articles.json with a stale/empty date
        std::fs::create_dir_all(&auto_dir).unwrap();
        std::fs::write(
            auto_dir.join("articles.json"),
            r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Test","file":"./content/001_test.mdx","published_date":"","status":"draft"}]}"#,
        )
        .unwrap();

        // Write MDX without a date
        let mdx_path = content_dir.join("001_test.mdx");
        write_mdx(&mdx_path, "Test");

        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', ?1, 1, 'workspace')",
            [dir.to_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, content_gaps_addressed, project_id)
             VALUES (1, 'Test', 'test', './content/001_test.mdx', 'draft', '[]', 'p1')",
            [],
        )
        .unwrap();

        // Publish the article
        let result = apply_publish(&conn, "p1", &[1], &HashMap::new(), &[], &content_dir, &dir);

        assert_eq!(result.published.len(), 1);
        let assigned_date = &result.published[0].published_date;

        // Verify SQLite has the assigned date
        let db_date: String = conn
            .query_row(
                "SELECT published_date FROM articles WHERE id = 1 AND project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(&db_date, assigned_date);

        // Verify articles.json has the same assigned date
        let json_on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&json_on_disk).unwrap();
        let json_date = doc["articles"][0]["published_date"].as_str().unwrap();
        assert_eq!(json_date, assigned_date);

        // Verify MDX frontmatter has the same assigned date
        let mdx_content = std::fs::read_to_string(&mdx_path).unwrap();
        assert!(
            mdx_content.contains(&format!("date: \"{}\"", assigned_date)),
            "MDX frontmatter should contain the assigned date {}. MDX content: {}",
            assigned_date,
            mdx_content
        );
    }

    fn insert_project(conn: &rusqlite::Connection, dir: &std::path::Path) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', ?1, 1, 'workspace')",
            [dir.to_str().unwrap()],
        )
        .unwrap();
    }

    fn insert_article(
        conn: &rusqlite::Connection,
        id: i64,
        title: &str,
        slug: &str,
        file: &str,
        status: &str,
        published_date: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, published_date, content_gaps_addressed, project_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]', 'p1')",
            rusqlite::params![id, title, slug, file, status, published_date],
        )
        .unwrap();
    }

    #[test]
    fn publish_by_slugs_draft_to_published_and_exports() {
        let dir = unique_temp_dir("ps_publish_by_slug_happy");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::create_dir_all(&auto_dir).unwrap();
        std::fs::write(
            auto_dir.join("articles.json"),
            r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Happy","file":"./content/001_happy.mdx","published_date":"2024-06-01","status":"draft"}]}"#,
        )
        .unwrap();
        write_mdx(&content_dir.join("001_happy.mdx"), "Happy");

        let conn = in_memory_db();
        insert_project(&conn, &dir);
        insert_article(
            &conn,
            1,
            "Happy",
            "happy",
            "./content/001_happy.mdx",
            "draft",
            Some("2024-06-01"),
        );

        let result =
            publish_by_slugs(&conn, "p1", &dir, &["happy".into()]).expect("publish_by_slugs");

        assert!(result.ok, "errors: {:?}", result.errors);
        assert_eq!(result.published.len(), 1);
        assert_eq!(result.published[0].slug, "happy");
        assert_eq!(result.published[0].catalog_status, "published");
        assert!(result.errors.is_empty());
        assert!(result.blocked.is_empty());
        assert!(result.year_mismatches.is_empty());

        let db_status: String = conn
            .query_row(
                "SELECT status FROM articles WHERE id = 1 AND project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(db_status, "published");

        let json_on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&json_on_disk).unwrap();
        assert_eq!(doc["articles"][0]["status"].as_str().unwrap(), "published");
    }

    #[test]
    fn publish_by_slugs_already_published_is_skip_noop() {
        let dir = unique_temp_dir("ps_publish_by_slug_skip");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        write_mdx(&content_dir.join("001_live.mdx"), "Live");

        let conn = in_memory_db();
        insert_project(&conn, &dir);
        insert_article(
            &conn,
            1,
            "Live",
            "live",
            "./content/001_live.mdx",
            "published",
            Some("2024-01-15"),
        );

        let result =
            publish_by_slugs(&conn, "p1", &dir, &["live".into()]).expect("publish_by_slugs");

        assert!(result.ok);
        assert!(result.published.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].reason, "already published");
        assert_eq!(
            result.skipped[0].catalog_status.as_deref(),
            Some("published")
        );
        assert!(result.errors.is_empty());
    }

    #[test]
    fn publish_by_slugs_missing_slug_is_error() {
        let dir = unique_temp_dir("ps_publish_by_slug_missing");
        std::fs::create_dir_all(dir.join("content")).unwrap();
        let conn = in_memory_db();
        insert_project(&conn, &dir);

        let result =
            publish_by_slugs(&conn, "p1", &dir, &["no-such-slug".into()]).expect("result ok");

        assert!(!result.ok);
        assert!(result.published.is_empty());
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("No article found for slug 'no-such-slug'")));
    }

    #[test]
    fn publish_by_slugs_year_mismatch_leaves_status_unchanged() {
        let dir = unique_temp_dir("ps_publish_by_slug_year");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        // Title year far behind publish year (>1) → year mismatch.
        write_mdx(&content_dir.join("001_guide_2020.mdx"), "Guide 2020");

        let conn = in_memory_db();
        insert_project(&conn, &dir);
        let today = chrono::Utc::now().date_naive();
        let publish_date = format!("{}-01-15", today.year());
        insert_article(
            &conn,
            1,
            "Guide 2020",
            "guide-2020",
            "./content/001_guide_2020.mdx",
            "draft",
            Some(&publish_date),
        );

        let result =
            publish_by_slugs(&conn, "p1", &dir, &["guide-2020".into()]).expect("publish_by_slugs");

        assert!(!result.ok);
        assert!(result.published.is_empty());
        assert_eq!(result.year_mismatches.len(), 1);
        assert_eq!(result.year_mismatches[0].title_year, 2020);
        assert_eq!(result.year_mismatches[0].catalog_status, "draft");

        let db_status: String = conn
            .query_row(
                "SELECT status FROM articles WHERE id = 1 AND project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(db_status, "draft");
    }

    #[test]
    fn publish_by_slugs_empty_slugs_err() {
        let dir = unique_temp_dir("ps_publish_by_slug_empty");
        let conn = in_memory_db();
        insert_project(&conn, &dir);
        let err = publish_by_slugs(&conn, "p1", &dir, &[]).unwrap_err();
        assert!(err.contains("slug"));
    }
}
