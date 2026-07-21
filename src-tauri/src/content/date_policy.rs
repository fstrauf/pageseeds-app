use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::models::article::Article;

/// Maximum number of days `find_first_free_past_date` walks backward looking
/// for an unoccupied date. Backdating new articles further than this sacrifices
/// the freshness signal, so beyond the cap we accept a duplicate on the most
/// recent date instead.
pub const MAX_LOOKBACK_DAYS: i64 = 7;

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

    // Duplicate dates are informational only — they are reported via
    // `duplicate_dates`/`duplicate_count` but are NOT validation errors:
    // sharing a date is acceptable (see `MAX_LOOKBACK_DAYS`), and rewriting
    // existing dates would sacrifice freshness signals on re-indexing.

    DatePolicyReport {
        total_checked,
        future_count,
        duplicate_count,
        issues,
        duplicate_dates,
    }
}

/// Find the most recent past date that is NOT in the `occupied` set.
///
/// Starting from the day before `today`, walks backward at most
/// `MAX_LOOKBACK_DAYS` days looking for an unoccupied date. If every date in
/// that window is occupied, returns the day before `today` anyway (accepting a
/// duplicate) rather than backdating further into the past. This is the single
/// source of truth for date assignment across all date-computing code paths.
pub fn find_first_free_past_date(today: NaiveDate, occupied: &HashSet<NaiveDate>) -> NaiveDate {
    let mut cursor = today - Duration::days(1);
    for _ in 0..MAX_LOOKBACK_DAYS {
        if !occupied.contains(&cursor) {
            return cursor;
        }
        cursor -= Duration::days(1);
    }
    today - Duration::days(1)
}

/// Suggest the publication date for a new article.
///
/// Prefers the most recent unoccupied past date, but never walks back further
/// than `MAX_LOOKBACK_DAYS` — if that whole window is occupied, the day before
/// today is reused so new articles keep a fresh publication date.
pub fn suggest_next_safe_date(articles: &[Article]) -> String {
    let today = Utc::now().date_naive();
    let occupied: HashSet<NaiveDate> = articles
        .iter()
        .filter_map(|a| a.published_date.as_deref())
        .filter_map(parse_iso_date)
        .collect();

    find_first_free_past_date(today, &occupied)
        .format("%Y-%m-%d")
        .to_string()
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
    // Duplicate dates are no longer issues, so there is nothing to strip —
    // just keep the historical behavior of not reporting duplicate details.
    DatePolicyReport {
        duplicate_count: 0,
        duplicate_dates: vec![],
        ..report
    }
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

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn find_first_free_past_date_caps_lookback_and_returns_yesterday() {
        // 8+ consecutive occupied days: the walk is capped at MAX_LOOKBACK_DAYS
        // and falls back to yesterday instead of backdating further.
        let today = date(2026, 7, 20);
        let occupied: HashSet<NaiveDate> = (1..=30).map(|i| today - Duration::days(i)).collect();

        let result = find_first_free_past_date(today, &occupied);

        assert_eq!(result, today - Duration::days(1));
    }

    #[test]
    fn find_first_free_past_date_returns_free_date_within_window() {
        let today = date(2026, 7, 20);
        let occupied: HashSet<NaiveDate> = [1, 2, 4]
            .into_iter()
            .map(|i| today - Duration::days(i))
            .collect();

        let result = find_first_free_past_date(today, &occupied);

        assert_eq!(result, today - Duration::days(3));
    }

    #[test]
    fn find_first_free_past_date_uses_last_window_day_when_free() {
        // Occupy the first MAX_LOOKBACK_DAYS - 1 days; the 7th day is still used.
        let today = date(2026, 7, 20);
        let occupied: HashSet<NaiveDate> = (1..MAX_LOOKBACK_DAYS)
            .map(|i| today - Duration::days(i))
            .collect();

        let result = find_first_free_past_date(today, &occupied);

        assert_eq!(result, today - Duration::days(MAX_LOOKBACK_DAYS));
    }

    #[test]
    fn validate_dates_reports_duplicates_informationally_without_issues() {
        let today = Utc::now().date_naive();
        let d = (today - Duration::days(1)).format("%Y-%m-%d").to_string();
        let articles = vec![
            article(1, Some(&d)),
            article(2, Some(&d)),
            article(3, Some(&(today - Duration::days(2)).format("%Y-%m-%d").to_string())),
        ];

        let report = validate_dates(&articles, &DatePolicyConfig::default());

        assert!(
            report.issues.iter().all(|i| i.issue_type != "duplicate_date"),
            "duplicate dates must not produce issues: {:?}",
            report
                .issues
                .iter()
                .map(|i| &i.issue_type)
                .collect::<Vec<_>>()
        );
        assert!(report.is_valid());
        assert_eq!(report.duplicate_count, 1);
        assert_eq!(report.duplicate_dates, vec![(d, vec![1, 2])]);
    }

    #[test]
    fn suggest_next_safe_date_caps_backdating_for_dense_history() {
        // A site that published daily for 60 days gets yesterday (a duplicate),
        // not a date 60 days in the past.
        let today = Utc::now().date_naive();
        let articles: Vec<Article> = (1..=60)
            .map(|i| {
                article(
                    i,
                    Some(
                        &(today - Duration::days(i64::from(i)))
                            .format("%Y-%m-%d")
                            .to_string(),
                    ),
                )
            })
            .collect();

        let result = suggest_next_safe_date(&articles);

        assert_eq!(
            result,
            (today - Duration::days(1)).format("%Y-%m-%d").to_string()
        );
    }
}
