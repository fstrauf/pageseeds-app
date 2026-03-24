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
use crate::db::export;
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
    let structural_issues = cleaner::scan_and_clean(content_dir, true).unwrap_or_else(|_| cleaner::CleaningResult {
        files_checked: 0,
        issues: vec![],
        issues_fixed: 0,
    });
    let structural_issue_count = structural_issues.issues.len();

    // Date analysis for ALL articles — needed for duplicate detection.
    let date_analysis = dates::analyse_dates(all_articles);

    // Collect article_ids that have date issues.
    let date_issue_ids: HashSet<i64> = date_analysis
        .issues
        .iter()
        .filter(|i| i.issue_type == "future_date" || i.issue_type == "duplicate_date")
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
                errors.push(format!("Unknown year mismatch action '{other}' for article {id}"));
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

    // Identify batch articles that still have date issues (future or duplicate)
    // after any explicit date_fixes have been applied. These must be auto-reassigned
    // rather than using their stored (bad) date — which would cause duplicates or
    // future-dated articles to be published as-is and block the articles.json export.
    let date_analysis = dates::analyse_dates(&all_articles);
    let needs_reassign: HashSet<i64> = date_analysis
        .issues
        .iter()
        .filter(|i| i.issue_type == "future_date" || i.issue_type == "duplicate_date")
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
            // Date is problematic (future or duplicate) — auto-assign the most
            // recent free past date, skipping everything already occupied.
            assign_free_date(today, &occupied, &assigned_dates)
        } else if let Some(d_str) = article.published_date.as_deref().filter(|s| !s.is_empty()) {
            // Already has a clean date (not flagged as future/duplicate).
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

    // Patch MDX frontmatter dates from the updated SQLite state via sync_and_validate.
    let automation_dir = project_path.join(".github").join("automation");
    if let Err(e) = crate::content::ops::sync_and_validate(&automation_dir, project_path, true) {
        errors.push(format!("MDX frontmatter sync warning: {e}"));
    }

    // Write articles.json.
    if let Err(e) = export::write_articles_to_repo(conn, project_id, project_path) {
        errors.push(format!("Failed to write articles.json: {e}"));
    }

    PublishResult {
        published,
        skipped,
        errors,
    }
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
    let json_str = extract_json_object(&raw).ok_or_else(|| {
        format!("Agent response did not contain a JSON object. Got: {}", raw.trim())
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
        return Err(format!("Unknown action '{action}' from agent. Expected update_title or backdate."));
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
fn assign_free_date(
    today: NaiveDate,
    occupied: &HashSet<NaiveDate>,
    assigned: &HashSet<NaiveDate>,
) -> String {
    let mut cursor = today - chrono::Duration::days(1);
    while occupied.contains(&cursor) || assigned.contains(&cursor) {
        cursor -= chrono::Duration::days(1);
    }
    cursor.format("%Y-%m-%d").to_string()
}

/// Extract the first JSON object `{...}` from a string (handles agent prose wrapping).
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end >= start {
        Some(text[start..=end].to_string())
    } else {
        None
    }
}
