/// Article date analysis and safe redistribution.
///
/// Mirrors `packages/seo-content-cli/src/seo_content_mcp/date_distributor.py`
/// and `date_utils.py`.
use std::collections::HashMap;

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::models::article::Article;

#[derive(Debug, Clone, Serialize)]
pub struct DateIssue {
    pub article_id: i64,
    pub issue_type: String, // future_date | duplicate_date | invalid_format | missing_date
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

/// Analyse article dates. Detects future dates, duplicates, and missing values.
pub fn analyse_dates(articles: &[Article]) -> DateAnalysis {
    let today = Utc::now().date_naive();
    let mut issues = Vec::new();
    let mut date_map: HashMap<String, Vec<i64>> = HashMap::new();
    let mut future_count = 0;
    let mut missing_count = 0;
    let mut published_count = 0;

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
            Some(ds) => {
                match NaiveDate::parse_from_str(ds, "%Y-%m-%d") {
                    Err(_) => {
                        issues.push(DateIssue {
                            article_id: article.id,
                            issue_type: "invalid_format".into(),
                            description: format!("Cannot parse date '{ds}'"),
                            current_date: ds.clone(),
                        });
                    }
                    Ok(d) => {
                        if d > today {
                            future_count += 1;
                            issues.push(DateIssue {
                                article_id: article.id,
                                issue_type: "future_date".into(),
                                description: format!("Date {ds} is in the future"),
                                current_date: ds.clone(),
                            });
                        } else {
                            published_count += 1;
                        }
                        date_map.entry(ds.clone()).or_default().push(article.id);
                    }
                }
            }
        }
    }

    // Detect duplicates
    let mut duplicate_dates: Vec<(String, Vec<i64>)> = date_map
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(date, mut ids)| {
            ids.sort();
            (date, ids)
        })
        .collect();
    duplicate_dates.sort_by(|a, b| a.0.cmp(&b.0));
    let duplicate_count = duplicate_dates.iter().map(|(_, ids)| ids.len() - 1).sum();

    // Add duplicate issues
    for (date, ids) in &duplicate_dates {
        for &id in &ids[1..] {
            issues.push(DateIssue {
                article_id: id,
                issue_type: "duplicate_date".into(),
                description: format!("Date {date} shared with article(s) {:?}", &ids[..ids.len() - 1]),
                current_date: date.clone(),
            });
        }
    }

    DateAnalysis {
        total_articles: articles.len(),
        published_count,
        future_count,
        duplicate_count,
        missing_count,
        issues,
        duplicate_dates,
    }
}

/// Calculate the next safe date for a new article.
///
/// Strategy: 2 days before the earliest existing date (or yesterday if none).
/// Guarantees no future dates and no overlaps.
pub fn next_article_date(articles: &[Article]) -> String {
    let today = Utc::now().date_naive();
    let earliest = articles
        .iter()
        .filter_map(|a| a.published_date.as_deref())
        .filter_map(|ds| NaiveDate::parse_from_str(ds, "%Y-%m-%d").ok())
        .min();

    let date = match earliest {
        None => today - Duration::days(1),
        Some(e) => e - Duration::days(2),
    };

    date.format("%Y-%m-%d").to_string()
}

/// Produce a fix plan that redistributes problematic dates (future or duplicate)
/// evenly in the past without touching already-published articles that are fine.
///
/// If `dry_run` is false, the caller is responsible for persisting the changes.
pub fn calculate_fixes(articles: &[Article]) -> DateFixResult {
    let analysis = analyse_dates(articles);
    let today = Utc::now().date_naive();

    // Collect articles that need a new date (future or duplicate).
    // We re-assign them evenly spaced, working backward from today.
    let mut bad_ids: Vec<i64> = analysis
        .issues
        .iter()
        .filter(|i| {
            i.issue_type == "future_date"
                || i.issue_type == "duplicate_date"
        })
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
