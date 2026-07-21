/// Article date analysis and future-date correction.
///
/// Mirrors `packages/seo-content-cli/src/seo_content_mcp/date_distributor.py`
/// and `date_utils.py`. Duplicate dates are reported informationally but never
/// rewritten.
use std::collections::HashMap;

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::content::date_policy::{self, DatePolicyConfig};
use crate::models::article::Article;

#[derive(Debug, Clone, Serialize)]
pub struct DateIssue {
    pub article_id: i64,
    pub issue_type: String, // future_date | invalid_format | missing_date
    pub description: String,
    pub current_date: String,
}

#[derive(Debug, Serialize)]
pub struct DateAnalysis {
    pub total_articles: usize,
    pub published_count: usize,
    pub future_count: usize,
    pub duplicate_count: usize,
    pub missing_count: usize,
    pub issues: Vec<DateIssue>,
    /// (date, vec of article_ids) for dates that appear more than once
    pub duplicate_dates: Vec<(String, Vec<i64>)>,
}

#[derive(Debug, Serialize)]
pub struct DateFix {
    pub article_id: i64,
    pub old_date: String,
    pub new_date: String,
}

#[derive(Debug, Serialize)]
pub struct DateFixResult {
    pub fixes: Vec<DateFix>,
    pub articles_fixed: usize,
    pub dry_run: bool,
}

/// Analyse article dates. Detects future dates and missing values; duplicate
/// dates are reported informationally via `duplicate_count`/`duplicate_dates`
/// but are not issues.
pub fn analyse_dates(articles: &[Article]) -> DateAnalysis {
    let mut issues = Vec::new();
    let mut missing_count = 0;
    let mut published_count = 0;

    let report = date_policy::validate_dates(articles, &DatePolicyConfig::default());
    let future_count = report.future_count;

    for article in articles {
        match &article.published_date {
            None => {
                missing_count += 1;
                issues.push(DateIssue {
                    article_id: article.id,
                    issue_type: "missing_date".into(),
                    description: "No published_date set".into(),
                    current_date: String::new(),
                });
            }
            Some(ds) if ds.is_empty() => {
                missing_count += 1;
                issues.push(DateIssue {
                    article_id: article.id,
                    issue_type: "missing_date".into(),
                    description: "published_date is empty".into(),
                    current_date: String::new(),
                });
            }
            Some(ds) => match NaiveDate::parse_from_str(ds, "%Y-%m-%d") {
                Err(_) => {
                    issues.push(DateIssue {
                        article_id: article.id,
                        issue_type: "invalid_format".into(),
                        description: format!("Cannot parse date '{ds}'"),
                        current_date: ds.clone(),
                    });
                }
                Ok(d) => {
                    if d <= Utc::now().date_naive() {
                        published_count += 1;
                    }
                }
            },
        }
    }

    issues.extend(report.issues.iter().cloned().map(|issue| DateIssue {
        article_id: issue.article_id,
        issue_type: issue.issue_type,
        description: issue.description,
        current_date: issue.current_date,
    }));

    DateAnalysis {
        total_articles: articles.len(),
        published_count,
        future_count,
        duplicate_count: report.duplicate_count,
        missing_count,
        issues,
        duplicate_dates: report.duplicate_dates,
    }
}

/// Produce a fix plan that reassigns future dates into the past without
/// touching already-published articles that are fine.
///
/// Duplicate dates are intentionally NOT fixed: sharing a date is acceptable
/// and rewriting existing dates would sacrifice freshness signals.
///
/// If `dry_run` is false, the caller is responsible for persisting the changes.
pub fn calculate_fixes(articles: &[Article]) -> DateFixResult {
    let analysis = analyse_dates(articles);
    let today = Utc::now().date_naive();

    // Collect articles that need a new date (future only — duplicates are fine).
    // We re-assign them evenly spaced, working backward from today.
    let mut bad_ids: Vec<i64> = analysis
        .issues
        .iter()
        .filter(|i| i.issue_type == "future_date")
        .map(|i| i.article_id)
        .collect();
    bad_ids.sort();
    bad_ids.dedup();

    if bad_ids.is_empty() {
        return DateFixResult {
            fixes: vec![],
            articles_fixed: 0,
            dry_run: true,
        };
    }

    // Collect occupied dates from articles we're NOT touching
    let occupied: std::collections::HashSet<NaiveDate> = articles
        .iter()
        .filter(|a| !bad_ids.contains(&a.id))
        .filter_map(|a| a.published_date.as_deref())
        .filter_map(|ds| NaiveDate::parse_from_str(ds, "%Y-%m-%d").ok())
        .collect();

    // Assign each bad article to the next-available past date, working backward
    // from yesterday.
    let mut cursor = today - Duration::days(1);
    let mut assigned: Vec<(i64, NaiveDate)> = Vec::new();

    for &id in bad_ids.iter().rev() {
        // Find a free date
        while occupied.contains(&cursor) || assigned.iter().any(|(_, d)| *d == cursor) {
            cursor -= Duration::days(1);
        }
        assigned.push((id, cursor));
        cursor -= Duration::days(1);
    }

    let old_date_map: HashMap<i64, String> = articles
        .iter()
        .filter_map(|a| a.published_date.clone().map(|d| (a.id, d)))
        .collect();

    let fixes: Vec<DateFix> = assigned
        .into_iter()
        .map(|(id, new_d)| DateFix {
            article_id: id,
            old_date: old_date_map.get(&id).cloned().unwrap_or_default(),
            new_date: new_d.format("%Y-%m-%d").to_string(),
        })
        .collect();

    let articles_fixed = fixes.len();

    DateFixResult {
        fixes,
        articles_fixed,
        dry_run: true, // caller decides whether to persist
    }
}

/// Apply date fixes to the database and export updated articles to the repo.
///
/// Updates `published_date` in the SQLite `articles` table for each fix, then
/// writes the full articles list back to `articles.json` in the project.
pub fn apply_fixes_to_db_and_export(
    conn: &rusqlite::Connection,
    project_id: &str,
    project_path: &std::path::Path,
    fixes: &[DateFix],
) -> Result<(), crate::error::Error> {
    for fix in fixes {
        conn.execute(
            "UPDATE articles SET published_date = ?1 WHERE id = ?2 AND project_id = ?3",
            rusqlite::params![fix.new_date, fix.article_id, project_id],
        )?;
    }
    crate::content::article_index::export_projection(conn, project_id, project_path).map(|_| ())
}

/// Deterministic post-write date enforcement.
///
/// Loads all articles from SQLite, detects future dates,
/// patches MDX frontmatter, updates SQLite, and exports articles.json.
///
/// This is a safety net that runs after any content-modifying task to ensure
/// no agent mistake or race condition leaves the project with a future date.
/// Duplicate dates are acceptable and are never rewritten.
pub fn enforce_safe_dates(
    conn: &rusqlite::Connection,
    project_id: &str,
    project_path: &std::path::Path,
) -> Result<DateFixResult, crate::error::Error> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| crate::error::Error::Other(e.to_string()))?;

    let mut result = calculate_fixes(&articles);
    if result.articles_fixed == 0 {
        return Ok(result);
    }

    // Patch MDX frontmatter for each fix before updating DB.
    for fix in &result.fixes {
        if let Some(article) = articles.iter().find(|a| a.id == fix.article_id) {
            let file_path = project_path.join(&article.file);
            if let Ok(text) = std::fs::read_to_string(&file_path) {
                if let Some(patched) = patch_mdx_date(&text, &fix.new_date) {
                    if std::fs::write(&file_path, patched).is_ok() {
                        log::info!(
                            "[enforce_safe_dates] Patched date for article {} ({}): {} -> {}",
                            fix.article_id,
                            article.file,
                            fix.old_date,
                            fix.new_date
                        );
                    } else {
                        log::warn!(
                            "[enforce_safe_dates] Failed to write patched file for article {}: {}",
                            fix.article_id,
                            article.file
                        );
                    }
                } else {
                    log::warn!(
                        "[enforce_safe_dates] Could not patch frontmatter for article {}: {}",
                        fix.article_id,
                        article.file
                    );
                }
            } else {
                log::warn!(
                    "[enforce_safe_dates] Could not read file for article {}: {}",
                    fix.article_id,
                    article.file
                );
            }
        }
    }

    // Update SQLite and export articles.json
    apply_fixes_to_db_and_export(conn, project_id, project_path, &result.fixes)?;
    result.dry_run = false;
    Ok(result)
}

/// Patch the `date` field in MDX frontmatter text.
/// Returns the rebuilt MDX with the updated date, or None if no frontmatter found.
fn patch_mdx_date(text: &str, new_date: &str) -> Option<String> {
    let (fm, body) = crate::content::frontmatter::split_mdx(text)?;
    let patched_fm = crate::content::frontmatter::replace_scalar(fm, "date", new_date);
    Some(crate::content::cleaner::rebuild_mdx(&patched_fm, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(id: i64, published_date: Option<&str>) -> Article {
        Article {
            id,
            title: format!("Article {id}"),
            url_slug: format!("article-{id}"),
            file: format!("article-{id}.mdx"),
            target_keyword: None,
            keyword_difficulty: None,
            target_volume: 0,
            published_date: published_date.map(str::to_string),
            word_count: 0,
            status: "published".into(),
            review_status: None,
            review_started_at: None,
            last_reviewed_at: None,
            review_count: 0,
            content_gaps_addressed: vec![],
            estimated_traffic_monthly: None,
            project_id: "proj".into(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
            page_type: None,
            content_hash: None,
            last_edited_at: None,
        }
    }

    fn fmt(d: NaiveDate) -> String {
        d.format("%Y-%m-%d").to_string()
    }

    #[test]
    fn calculate_fixes_produces_no_fixes_for_duplicate_only_dates() {
        let today = Utc::now().date_naive();
        let shared = fmt(today - Duration::days(1));
        let articles = vec![
            article(1, Some(&shared)),
            article(2, Some(&shared)),
            article(3, Some(&fmt(today - Duration::days(2)))),
        ];

        let result = calculate_fixes(&articles);

        assert_eq!(result.articles_fixed, 0);
        assert!(result.fixes.is_empty());
    }

    #[test]
    fn calculate_fixes_still_fixes_future_dates() {
        let today = Utc::now().date_naive();
        let articles = vec![
            article(1, Some(&fmt(today - Duration::days(1)))),
            article(2, Some(&fmt(today + Duration::days(5)))),
        ];

        let result = calculate_fixes(&articles);

        assert_eq!(result.articles_fixed, 1);
        assert_eq!(result.fixes.len(), 1);
        assert_eq!(result.fixes[0].article_id, 2);
        let new_date = NaiveDate::parse_from_str(&result.fixes[0].new_date, "%Y-%m-%d").unwrap();
        assert!(new_date <= today, "fix must land in the past, got {new_date}");
    }

    #[test]
    fn analyse_dates_reports_duplicates_informationally() {
        let today = Utc::now().date_naive();
        let shared = fmt(today - Duration::days(1));
        let articles = vec![article(1, Some(&shared)), article(2, Some(&shared))];

        let analysis = analyse_dates(&articles);

        assert_eq!(analysis.duplicate_count, 1);
        assert_eq!(analysis.duplicate_dates, vec![(shared, vec![1, 2])]);
        assert!(
            analysis.issues.iter().all(|i| i.issue_type != "duplicate_date"),
            "duplicates must not surface as issues"
        );
    }
}
