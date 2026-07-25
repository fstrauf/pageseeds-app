//! Shared GSC page-daily join helpers.
//!
//! Single source of truth for:
//! - page index: normalized slug → all GSC page URL variants
//! - recent/previous date windows (ending yesterday)
//! - slug rollup of window metrics (sum across URL variants)
//!
//! Used by desk Site State builders and territory analysis (issue #167 / PR #176).

use std::collections::HashMap;

use chrono::{Duration, Utc};
use rusqlite::Connection;

use crate::content::slug::extract_slug_from_url;
use crate::error::Result;

use super::GscDailyWindowMetrics;

/// Normalized slug → all GSC page URLs that extract to that slug.
///
/// Built once per read; O(1) lookup replaces a linear scan over inventory.
pub fn build_page_index(
    conn: &Connection,
    project_id: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let pages = super::list_gsc_page_daily_pages(conn, project_id)?;
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for page in pages {
        let slug = extract_slug_from_url(&page);
        if slug.is_empty() {
            continue;
        }
        index.entry(slug).or_default().push(page);
    }
    Ok(index)
}

/// Page URLs for a catalog slug (empty slice = no GSC inventory).
pub fn pages_for_slug<'a>(page_index: &'a HashMap<String, Vec<String>>, slug: &str) -> &'a [String] {
    page_index.get(slug).map(Vec::as_slice).unwrap_or(&[])
}

/// Merge bulk window metrics for every GSC page URL that maps to `slug`.
///
/// Returns `(metrics, url_variants)` where `url_variants` is the page count when
/// any metrics exist, else 0. Callers must **sum all page keys that normalize to
/// the catalog slug** so first-only matching does not undercount when GSC stores
/// underscore vs hyphen or trailing-slash URL variants as separate pages.
pub fn rollup_for_slug(
    page_index: &HashMap<String, Vec<String>>,
    metrics_by_page: &HashMap<String, GscDailyWindowMetrics>,
    slug: &str,
) -> (Option<GscDailyWindowMetrics>, usize) {
    let pages = pages_for_slug(page_index, slug);
    let metrics = merge_window_metrics(pages.iter().filter_map(|p| metrics_by_page.get(p).copied()));
    let url_variants = if metrics.is_some() { pages.len() } else { 0 };
    (metrics, url_variants)
}

/// Sum impressions only (territory analysis convenience over [`rollup_for_slug`]).
pub fn rollup_impressions_for_slug(
    page_index: &HashMap<String, Vec<String>>,
    metrics_by_page: &HashMap<String, GscDailyWindowMetrics>,
    slug: &str,
) -> Option<f64> {
    rollup_for_slug(page_index, metrics_by_page, slug)
        .0
        .map(|m| m.impressions)
}

/// Merge window metrics from multiple GSC page keys into one rollup.
pub fn merge_window_metrics(
    iter: impl IntoIterator<Item = GscDailyWindowMetrics>,
) -> Option<GscDailyWindowMetrics> {
    let mut clicks = 0.0_f64;
    let mut impressions = 0.0_f64;
    let mut position_weight = 0.0_f64;
    let mut days_with_data = 0_i64;
    let mut any = false;

    for m in iter {
        any = true;
        clicks += m.clicks;
        impressions += m.impressions;
        position_weight += m.position * m.impressions;
        days_with_data = days_with_data.max(m.days_with_data);
    }

    if !any {
        return None;
    }

    let position = if impressions > 0.0 {
        position_weight / impressions
    } else {
        0.0
    };

    Some(GscDailyWindowMetrics {
        days_with_data,
        clicks,
        impressions,
        position,
    })
}

/// Inclusive date window of `period_days` ending yesterday (UTC calendar day).
pub fn recent_window(period_days: i64) -> (String, String) {
    let end = Utc::now().date_naive() - Duration::days(1);
    let start = end - Duration::days(period_days - 1);
    (
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

/// Inclusive previous window of the same length, immediately before [`recent_window`].
pub fn previous_window(period_days: i64) -> (String, String) {
    let recent_end = Utc::now().date_naive() - Duration::days(1);
    let recent_start = recent_end - Duration::days(period_days - 1);
    let prev_end = recent_start - Duration::days(1);
    let prev_start = prev_end - Duration::days(period_days - 1);
    (
        prev_start.format("%Y-%m-%d").to_string(),
        prev_end.format("%Y-%m-%d").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::GscDailyWindowMetrics;

    #[test]
    fn merge_window_metrics_sums_and_weights_position() {
        let a = GscDailyWindowMetrics {
            days_with_data: 2,
            clicks: 1.0,
            impressions: 100.0,
            position: 10.0,
        };
        let b = GscDailyWindowMetrics {
            days_with_data: 3,
            clicks: 4.0,
            impressions: 300.0,
            position: 5.0,
        };
        let m = merge_window_metrics([a, b]).unwrap();
        assert_eq!(m.clicks, 5.0);
        assert_eq!(m.impressions, 400.0);
        assert_eq!(m.days_with_data, 3);
        // (10*100 + 5*300) / 400 = 6.25
        assert!((m.position - 6.25).abs() < 1e-9);
    }

    #[test]
    fn rollup_for_slug_sums_url_variants() {
        let mut page_index = HashMap::new();
        page_index.insert(
            "my-post".to_string(),
            vec![
                "https://ex.com/blog/my-post".to_string(),
                "https://ex.com/blog/my-post/".to_string(),
            ],
        );
        let mut metrics = HashMap::new();
        metrics.insert(
            "https://ex.com/blog/my-post".to_string(),
            GscDailyWindowMetrics {
                days_with_data: 1,
                clicks: 2.0,
                impressions: 100.0,
                position: 8.0,
            },
        );
        metrics.insert(
            "https://ex.com/blog/my-post/".to_string(),
            GscDailyWindowMetrics {
                days_with_data: 1,
                clicks: 3.0,
                impressions: 50.0,
                position: 4.0,
            },
        );
        let (m, variants) = rollup_for_slug(&page_index, &metrics, "my-post");
        let m = m.unwrap();
        assert_eq!(variants, 2);
        assert_eq!(m.impressions, 150.0);
        assert_eq!(m.clicks, 5.0);
        assert_eq!(
            rollup_impressions_for_slug(&page_index, &metrics, "my-post"),
            Some(150.0)
        );
        assert_eq!(
            rollup_impressions_for_slug(&page_index, &metrics, "missing"),
            None
        );
    }

    #[test]
    fn recent_window_is_period_days_ending_yesterday() {
        let (start, end) = recent_window(28);
        let end_d = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap();
        let start_d = chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d").unwrap();
        let yesterday = Utc::now().date_naive() - Duration::days(1);
        assert_eq!(end_d, yesterday);
        assert_eq!((end_d - start_d).num_days(), 27);
    }
}
