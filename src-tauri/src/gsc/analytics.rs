use serde_json::json;

use crate::error::{Error, Result};
use crate::gsc::client::GscClient;
use crate::models::gsc::{MoverMetrics, PageMetrics, QueryMetrics};

/// Fetch top pages by clicks for a date range.
pub async fn fetch_page_rows(
    token: &str,
    site_url: &str,
    start_date: &str,
    end_date: &str,
    row_limit: u32,
) -> Result<Vec<PageMetrics>> {
    let client = GscClient::new(token);
    let body = json!({
        "startDate": start_date,
        "endDate": end_date,
        "dimensions": ["page"],
        "rowLimit": row_limit,
        "orderBy": [{"fieldName": "clicks", "sortOrder": "DESCENDING"}]
    });
    let resp = client.search_analytics_query(site_url, &body).await?;
    parse_page_rows(&resp)
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
        "rowLimit": row_limit,
        "orderBy": [{"fieldName": "clicks", "sortOrder": "DESCENDING"}]
    });
    let resp = client.search_analytics_query(site_url, &body).await?;
    parse_query_rows(&resp)
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
    let curr_rows =
        fetch_page_rows(token, site_url, curr_start, curr_end, row_limit).await?;
    let prev_rows =
        fetch_page_rows(token, site_url, prev_start, prev_end, row_limit).await?;

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
                impressions_delta: curr.impressions
                    - prev.map(|p| p.impressions).unwrap_or(0.0),
                position_delta: prev
                    .map(|p| p.position - curr.position)
                    .unwrap_or(0.0),
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
