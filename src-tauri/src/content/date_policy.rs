use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::models::article::Article;

#[derive(Debug, Clone, Serialize)]
pub struct DatePolicyIssue {
    pub article_id: i64,
    pub issue_type: String,
    pub description: String,
    pub current_date: String,
}

#[derive(Debug, Clone)]
pub struct DatePolicyConfig {
    pub allowed_future_days: i64,
    pub statuses: Option<HashSet<String>>,
}

impl Default for DatePolicyConfig {
    fn default() -> Self {
        Self {
            allowed_future_days: 0,
            statuses: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DatePolicyReport {
    pub total_checked: usize,
    pub future_count: usize,
    pub duplicate_count: usize,
    pub issues: Vec<DatePolicyIssue>,
    pub duplicate_dates: Vec<(String, Vec<i64>)>,
}

impl DatePolicyReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn status_allowed(article: &Article, cfg: &DatePolicyConfig) -> bool {
    match &cfg.statuses {
        None => true,
        Some(set) => set.contains(&article.status.to_lowercase()),
    }
}

pub fn validate_dates(articles: &[Article], cfg: &DatePolicyConfig) -> DatePolicyReport {
    let today = Utc::now().date_naive();
    let max_allowed = today + Duration::days(cfg.allowed_future_days.max(0));

    let mut issues = Vec::new();
    let mut date_map: HashMap<String, Vec<i64>> = HashMap::new();
    let mut future_count = 0usize;
    let mut total_checked = 0usize;

    for article in articles.iter().filter(|a| status_allowed(a, cfg)) {
        let Some(ds) = article.published_date.as_deref().map(str::trim) else {
            continue;
        };
        if ds.is_empty() {
            continue;
        }

        total_checked += 1;
        match parse_iso_date(ds) {
            None => {
                issues.push(DatePolicyIssue {
                    article_id: article.id,
                    issue_type: "invalid_format".into(),
                    description: format!("Cannot parse date '{ds}'"),
                    current_date: ds.to_string(),
                });
            }
            Some(d) => {
                if d > max_allowed {
                    future_count += 1;
                    issues.push(DatePolicyIssue {
                        article_id: article.id,
                        issue_type: "future_date".into(),
                        description: format!(
                            "Date {ds} is in the future (allowed max: {})",
                            max_allowed.format("%Y-%m-%d")
                        ),
                        current_date: ds.to_string(),
                    });
                }
                date_map.entry(ds.to_string()).or_default().push(article.id);
            }
        }
    }

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

    for (date, ids) in &duplicate_dates {
        for id in ids {
            issues.push(DatePolicyIssue {
                article_id: *id,
                issue_type: "duplicate_date".into(),
                description: format!("Date {date} is shared by article IDs {ids:?}"),
                current_date: date.clone(),
            });
        }
    }

    DatePolicyReport {
        total_checked,
        future_count,
        duplicate_count,
        issues,
        duplicate_dates,
    }
}

pub fn suggest_next_safe_date(articles: &[Article]) -> String {
    let today = Utc::now().date_naive();
    let occupied: HashSet<NaiveDate> = articles
        .iter()
        .filter_map(|a| a.published_date.as_deref())
        .filter_map(parse_iso_date)
        .collect();

    let mut cursor = today - Duration::days(1);
    while occupied.contains(&cursor) {
        cursor -= Duration::days(1);
    }
    cursor.format("%Y-%m-%d").to_string()
}

pub fn statuses_set(statuses: &[&str]) -> HashSet<String> {
    statuses.iter().map(|s| s.to_lowercase()).collect()
}

/// Export-time gate: only block on future dates.
///
/// Duplicate dates among already-published articles are a legacy import artefact.
/// Retroactively changing them risks Google re-indexing signals, so we treat
/// them as informational only and never block an export on their account.
pub fn validate_no_future_dates(articles: &[Article]) -> DatePolicyReport {
    let report = validate_dates(
        articles,
        &DatePolicyConfig {
            allowed_future_days: 0,
            statuses: Some(statuses_set(&["published", "ready_to_publish"])),
        },
    );
    // Strip duplicate_date issues — only keep future_date and invalid_format.
    let issues: Vec<DatePolicyIssue> = report
        .issues
        .into_iter()
        .filter(|i| i.issue_type != "duplicate_date")
        .collect();
    let future_count = issues
        .iter()
        .filter(|i| i.issue_type == "future_date")
        .count();
    DatePolicyReport {
        total_checked: report.total_checked,
        future_count,
        duplicate_count: 0,
        duplicate_dates: vec![],
        issues,
    }
}
