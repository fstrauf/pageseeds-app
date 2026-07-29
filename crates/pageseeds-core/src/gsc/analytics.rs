use serde_json::json;

use crate::error::{Error, Result};
use crate::gsc::client::GscClient;
use crate::models::gsc::{MoverMetrics, PageDailyMetrics, PageMetrics, PageQueryMetrics, QueryMetrics};

/// GSC Search Analytics API hard cap per request (`rowLimit` / page size).
pub const GSC_API_MAX_ROWS: u32 = 25_000;

/// Whether another `startRow` page is needed after receiving `page_len` rows
/// with the given page size. Stops when empty or short of a full page.
fn has_more_pages(page_len: usize, page_size: u32) -> bool {
    page_len > 0 && (page_len as u32) >= page_size
}

/// Advance `startRow` for the next Search Analytics request.
fn advance_start_row(start_row: u32, page_size: u32) -> u32 {
    start_row.saturating_add(page_size)
}

/// Clamp a requested page size into the legal GSC range `[1, GSC_API_MAX_ROWS]`.
fn clamp_page_size(row_limit: u32) -> u32 {
    row_limit.min(GSC_API_MAX_ROWS).max(1)
}

/// Resolve page size + optional total cap from a public `row_limit` argument.
///
/// - `0` → unlimited: page at [`GSC_API_MAX_ROWS`] until a short/empty page
/// - `N > 0` → collect at most `N` rows, paging in chunks of `min(N, GSC_API_MAX_ROWS)`
fn resolve_pagination(row_limit: u32) -> (u32, Option<usize>) {
    if row_limit == 0 {
        (GSC_API_MAX_ROWS, None)
    } else {
        (clamp_page_size(row_limit), Some(row_limit as usize))
    }
}

/// Fetch Search Analytics pages: loop `startRow` += page size until a response
/// returns fewer than `page_size` rows (or empty), or until `max_total` is hit.
///
/// On mid-pagination API error after some rows were collected, returns the
/// partial set and logs `partial=true` rather than discarding work.
async fn fetch_paginated_rows<T, F>(
    client: &GscClient,
    site_url: &str,
    page_size: u32,
    max_total: Option<usize>,
    log_label: &str,
    mut build_body: impl FnMut(u32, u32) -> serde_json::Value,
    parse: F,
) -> Result<Vec<T>>
where
    F: Fn(&serde_json::Value) -> Result<Vec<T>>,
{
    let page_size = clamp_page_size(page_size);
    let mut all: Vec<T> = Vec::new();
    let mut start_row: u32 = 0;
    let mut pages: u32 = 0;
    let mut partial = false;

    loop {
        let body = build_body(start_row, page_size);
        let resp = match client.search_analytics_query(site_url, &body).await {
            Ok(r) => r,
            Err(e) => {
                if all.is_empty() {
                    return Err(e);
                }
                log::warn!(
                    "[gsc::analytics] {} partial fetch after {} page(s)/{} rows: {}",
                    log_label,
                    pages,
                    all.len(),
                    e
                );
                partial = true;
                break;
            }
        };
        pages += 1;
        let page = parse(&resp)?;
        let n = page.len();
        all.extend(page);

        if let Some(max) = max_total {
            if all.len() >= max {
                all.truncate(max);
                break;
            }
        }
        if !has_more_pages(n, page_size) {
            break;
        }
        start_row = advance_start_row(start_row, page_size);
    }

    log::info!(
        "[gsc::analytics] {} fetched {} rows across {} page(s) (page_size={}, partial={})",
        log_label,
        all.len(),
        pages,
        page_size,
        partial
    );
    Ok(all)
}

/// Fetch top pages by clicks for a date range.
///
/// Paginates via `startRow`. `row_limit == 0` means full pagination until
/// exhausted (page size [`GSC_API_MAX_ROWS`]); `row_limit > 0` is a total cap
/// (top-N), still paged when larger than the API max.
pub async fn fetch_page_rows(
    token: &str,
    site_url: &str,
    start_date: &str,
    end_date: &str,
    row_limit: u32,
) -> Result<Vec<PageMetrics>> {
    let client = GscClient::new(token);
    let start_date = start_date.to_string();
    let end_date = end_date.to_string();
    let (page_size, max_total) = resolve_pagination(row_limit);
    fetch_paginated_rows(
        &client,
        site_url,
        page_size,
        max_total,
        "page",
        |start_row, page_size| {
            json!({
                "startDate": start_date,
                "endDate": end_date,
                "dimensions": ["page"],
                "rowLimit": page_size,
                "startRow": start_row,
                "orderBy": [{"fieldName": "clicks", "sortOrder": "DESCENDING"}]
            })
        },
        parse_page_rows,
    )
    .await
}

/// Fetch top queries for a specific page.
pub async fn fetch_queries_for_page(
    token: &str,
    site_url: &str,
    page_url: &str,
    start_date: &str,
    end_date: &str,
    row_limit: u32,
) -> Result<Vec<QueryMetrics>> {
    let client = GscClient::new(token);
    // Single-page query (filtered); still honor row_limit but no multi-page
    // helper needed for typical small result sets. Keep one-shot for simplicity.
    let body = json!({
        "startDate": start_date,
        "endDate": end_date,
        "dimensions": ["query"],
        "dimensionFilterGroups": [{
            "filters": [{
                "dimension": "page",
                "operator": "EQUALS",
                "expression": page_url
            }]
        }],
        "rowLimit": clamp_page_size(row_limit),
        "orderBy": [{"fieldName": "clicks", "sortOrder": "DESCENDING"}]
    });
    let resp = client.search_analytics_query(site_url, &body).await?;
    parse_query_rows(&resp)
}

/// Fetch top page + query combinations for a date range.
///
/// Same pagination contract as [`fetch_page_rows`]: `0` = full pull, else top-N.
pub async fn fetch_page_query_rows(
    token: &str,
    site_url: &str,
    start_date: &str,
    end_date: &str,
    row_limit: u32,
) -> Result<Vec<PageQueryMetrics>> {
    let client = GscClient::new(token);
    let start_date = start_date.to_string();
    let end_date = end_date.to_string();
    let (page_size, max_total) = resolve_pagination(row_limit);
    fetch_paginated_rows(
        &client,
        site_url,
        page_size,
        max_total,
        "page×query",
        |start_row, page_size| {
            json!({
                "startDate": start_date,
                "endDate": end_date,
                "dimensions": ["page", "query"],
                "rowLimit": page_size,
                "startRow": start_row,
                "orderBy": [{"fieldName": "clicks", "sortOrder": "DESCENDING"}]
            })
        },
        parse_page_query_rows,
    )
    .await
}

/// Fetch per-page daily metrics for a date range.
///
/// This is the time-series pull behind append-only snapshots (`gsc_page_daily`)
/// used for before/after outcome measurement (issue #23). Per-page daily
/// granularity (decision: not site-wide) so windows are directly comparable
/// to per-article baselines.
///
/// **Paginated** (issue #262): loops `startRow` until a short/empty page.
/// Prefer `row_limit = 0` or [`GSC_API_MAX_ROWS`] for full tape coverage.
pub async fn fetch_page_daily_rows(
    token: &str,
    site_url: &str,
    start_date: &str,
    end_date: &str,
    row_limit: u32,
) -> Result<Vec<PageDailyMetrics>> {
    let client = GscClient::new(token);
    let start_date = start_date.to_string();
    let end_date = end_date.to_string();
    // Page-daily always wants full coverage when callers pass the API max or 0.
    // A positive cap below the max is still honored (tests / narrow windows).
    let (page_size, max_total) = if row_limit == 0 || row_limit >= GSC_API_MAX_ROWS {
        (GSC_API_MAX_ROWS, None)
    } else {
        resolve_pagination(row_limit)
    };
    fetch_paginated_rows(
        &client,
        site_url,
        page_size,
        max_total,
        "page-daily",
        |start_row, page_size| {
            json!({
                "startDate": start_date,
                "endDate": end_date,
                "dimensions": ["page", "date"],
                "rowLimit": page_size,
                "startRow": start_row,
                "orderBy": [{"fieldName": "clicks", "sortOrder": "DESCENDING"}]
            })
        },
        parse_page_daily_rows,
    )
    .await
}

/// Compute traffic movers by comparing two date periods.
pub async fn compute_movers(
    token: &str,
    site_url: &str,
    curr_start: &str,
    curr_end: &str,
    prev_start: &str,
    prev_end: &str,
    row_limit: u32,
) -> Result<Vec<MoverMetrics>> {
    let curr_rows = fetch_page_rows(token, site_url, curr_start, curr_end, row_limit).await?;
    let prev_rows = fetch_page_rows(token, site_url, prev_start, prev_end, row_limit).await?;

    // Build map of prev period by page
    use std::collections::HashMap;
    let prev_map: HashMap<&str, &PageMetrics> =
        prev_rows.iter().map(|r| (r.page.as_str(), r)).collect();

    let mut movers: Vec<MoverMetrics> = curr_rows
        .iter()
        .map(|curr| {
            let prev = prev_map.get(curr.page.as_str());
            MoverMetrics {
                key: curr.page.clone(),
                current_clicks: curr.clicks,
                current_impressions: curr.impressions,
                current_position: curr.position,
                previous_clicks: prev.map(|p| p.clicks).unwrap_or(0.0),
                previous_impressions: prev.map(|p| p.impressions).unwrap_or(0.0),
                previous_position: prev.map(|p| p.position).unwrap_or(0.0),
                clicks_delta: curr.clicks - prev.map(|p| p.clicks).unwrap_or(0.0),
                impressions_delta: curr.impressions - prev.map(|p| p.impressions).unwrap_or(0.0),
                position_delta: prev.map(|p| p.position - curr.position).unwrap_or(0.0),
            }
        })
        .collect();

    // Sort by absolute clicks delta descending
    movers.sort_by(|a, b| {
        b.clicks_delta
            .abs()
            .partial_cmp(&a.clicks_delta.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(movers)
}

// ─── Parsers ──────────────────────────────────────────────────────────────────

fn parse_page_rows(resp: &serde_json::Value) -> Result<Vec<PageMetrics>> {
    let rows = match resp.get("rows").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return Ok(vec![]),
    };

    rows.iter()
        .map(|row| {
            let page = row["keys"][0]
                .as_str()
                .ok_or_else(|| Error::Other("Missing page key".to_string()))?
                .to_string();
            Ok(PageMetrics {
                page,
                clicks: row["clicks"].as_f64().unwrap_or(0.0),
                impressions: row["impressions"].as_f64().unwrap_or(0.0),
                ctr: row["ctr"].as_f64().unwrap_or(0.0),
                position: row["position"].as_f64().unwrap_or(0.0),
            })
        })
        .collect()
}

fn parse_query_rows(resp: &serde_json::Value) -> Result<Vec<QueryMetrics>> {
    let rows = match resp.get("rows").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return Ok(vec![]),
    };

    rows.iter()
        .map(|row| {
            let query = row["keys"][0]
                .as_str()
                .ok_or_else(|| Error::Other("Missing query key".to_string()))?
                .to_string();
            Ok(QueryMetrics {
                query,
                clicks: row["clicks"].as_f64().unwrap_or(0.0),
                impressions: row["impressions"].as_f64().unwrap_or(0.0),
                ctr: row["ctr"].as_f64().unwrap_or(0.0),
                position: row["position"].as_f64().unwrap_or(0.0),
            })
        })
        .collect()
}

fn parse_page_daily_rows(resp: &serde_json::Value) -> Result<Vec<PageDailyMetrics>> {
    let rows = match resp.get("rows").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return Ok(vec![]),
    };

    rows.iter()
        .map(|row| {
            let page = row["keys"][0]
                .as_str()
                .ok_or_else(|| Error::Other("Missing page key".to_string()))?
                .to_string();
            let date = row["keys"][1]
                .as_str()
                .ok_or_else(|| Error::Other("Missing date key".to_string()))?
                .to_string();
            Ok(PageDailyMetrics {
                page,
                date,
                clicks: row["clicks"].as_f64().unwrap_or(0.0),
                impressions: row["impressions"].as_f64().unwrap_or(0.0),
                ctr: row["ctr"].as_f64().unwrap_or(0.0),
                position: row["position"].as_f64().unwrap_or(0.0),
            })
        })
        .collect()
}

fn parse_page_query_rows(resp: &serde_json::Value) -> Result<Vec<PageQueryMetrics>> {
    let rows = match resp.get("rows").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return Ok(vec![]),
    };

    rows.iter()
        .map(|row| {
            let page = row["keys"][0]
                .as_str()
                .ok_or_else(|| Error::Other("Missing page key".to_string()))?
                .to_string();
            let query = row["keys"][1]
                .as_str()
                .ok_or_else(|| Error::Other("Missing query key".to_string()))?
                .to_string();
            Ok(PageQueryMetrics {
                page,
                query,
                clicks: row["clicks"].as_f64().unwrap_or(0.0),
                impressions: row["impressions"].as_f64().unwrap_or(0.0),
                ctr: row["ctr"].as_f64().unwrap_or(0.0),
                position: row["position"].as_f64().unwrap_or(0.0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_page_daily_rows_extracts_page_and_date_keys() {
        let resp = serde_json::json!({
            "rows": [
                {
                    "keys": ["https://example.com/blog/foo", "2026-07-01"],
                    "clicks": 3.0,
                    "impressions": 120.0,
                    "ctr": 0.025,
                    "position": 8.4
                }
            ]
        });
        let rows = parse_page_daily_rows(&resp).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].page, "https://example.com/blog/foo");
        assert_eq!(rows[0].date, "2026-07-01");
        assert_eq!(rows[0].clicks, 3.0);
        assert_eq!(rows[0].impressions, 120.0);
    }

    #[test]
    fn parse_page_daily_rows_empty_when_no_rows() {
        let resp = serde_json::json!({});
        assert!(parse_page_daily_rows(&resp).unwrap().is_empty());
    }

    #[test]
    fn has_more_pages_true_on_full_page() {
        assert!(has_more_pages(25_000, 25_000));
        assert!(has_more_pages(1000, 1000));
    }

    #[test]
    fn has_more_pages_false_on_short_or_empty() {
        assert!(!has_more_pages(0, 25_000));
        assert!(!has_more_pages(999, 1000));
        assert!(!has_more_pages(1, 25_000));
    }

    #[test]
    fn advance_start_row_steps_by_page_size() {
        assert_eq!(advance_start_row(0, 25_000), 25_000);
        assert_eq!(advance_start_row(25_000, 25_000), 50_000);
        assert_eq!(advance_start_row(0, 1000), 1000);
    }

    #[test]
    fn clamp_page_size_respects_gsc_max() {
        assert_eq!(clamp_page_size(0), 1);
        assert_eq!(clamp_page_size(1000), 1000);
        assert_eq!(clamp_page_size(25_000), 25_000);
        assert_eq!(clamp_page_size(100_000), GSC_API_MAX_ROWS);
    }

    #[test]
    fn resolve_pagination_zero_means_unlimited() {
        let (page_size, max_total) = resolve_pagination(0);
        assert_eq!(page_size, GSC_API_MAX_ROWS);
        assert_eq!(max_total, None);
    }

    #[test]
    fn resolve_pagination_positive_is_total_cap() {
        let (page_size, max_total) = resolve_pagination(1000);
        assert_eq!(page_size, 1000);
        assert_eq!(max_total, Some(1000));
    }

    /// Pure multi-page accumulation contract used by the async helper:
    /// full pages continue; a short final page ends the loop.
    #[test]
    fn pagination_loop_accumulates_until_short_page() {
        let page_size: u32 = 3;
        // Simulate three GSC responses: full, full, short.
        let pages: Vec<Vec<u32>> = vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8]];
        let mut all = Vec::new();
        let mut start_row: u32 = 0;
        let mut page_count = 0u32;
        for page in &pages {
            page_count += 1;
            let n = page.len();
            all.extend(page.iter().copied());
            if !has_more_pages(n, page_size) {
                break;
            }
            start_row = advance_start_row(start_row, page_size);
        }
        assert_eq!(page_count, 3);
        assert_eq!(start_row, 6); // advanced twice: 0→3→6, then short page stops
        assert_eq!(all, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn pagination_loop_respects_max_total_cap() {
        let page_size: u32 = 3;
        let max_total = 5usize;
        let pages: Vec<Vec<u32>> = vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8]];
        let mut all = Vec::new();
        for page in &pages {
            all.extend(page.iter().copied());
            if all.len() >= max_total {
                all.truncate(max_total);
                break;
            }
            if !has_more_pages(page.len(), page_size) {
                break;
            }
        }
        assert_eq!(all, vec![1, 2, 3, 4, 5]);
    }
}
