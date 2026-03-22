use serde_json::json;
use tokio::time::{sleep, Duration};

use crate::error::Result;
use crate::gsc::client::GscClient;
use crate::models::gsc::InspectionRecord;

/// Inspect a batch of URLs (200 ms delay between requests to respect quota).
pub async fn inspect_batch(
    token: &str,
    site_url: &str,
    urls: Vec<String>,
) -> Result<Vec<InspectionRecord>> {
    let client = GscClient::new(token);
    let mut records = Vec::with_capacity(urls.len());

    for url in &urls {
        let body = json!({
            "inspectionUrl": url,
            "siteUrl": site_url,
        });
        let resp = client.url_inspection_inspect(&body).await?;
        records.push(parse_inspection_record(url, &resp));
        sleep(Duration::from_millis(200)).await;
    }

    Ok(records)
}

fn parse_inspection_record(url: &str, resp: &serde_json::Value) -> InspectionRecord {
    let result = &resp["inspectionResult"];
    let index = &result["indexStatusResult"];
    let rich = &result["richResultsResult"];
    let _ = rich; // kept for future use

    let verdict = index["verdict"].as_str().unwrap_or("UNKNOWN").to_string();
    let coverage_state = index["coverageState"].as_str().unwrap_or("").to_string();

    let (action, priority) = classify_record(&verdict, &coverage_state);

    InspectionRecord {
        url: url.to_string(),
        verdict: Some(verdict.clone()),
        coverage_state: Some(coverage_state.clone()),
        indexing_state: Some(
            index["indexingState"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        ),
        robots_txt_state: Some(
            index["robotsTxtState"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        ),
        page_fetch_state: Some(
            index["pageFetchState"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        ),
        crawl_allowed: Some(index["crawlAllowed"].as_bool().unwrap_or(true)),
        indexing_allowed: Some(index["indexingAllowed"].as_bool().unwrap_or(true)),
        last_crawl_time: index["lastCrawlTime"].as_str().map(String::from),
        google_canonical: index["googleCanonical"].as_str().map(String::from),
        user_canonical: index["userDeclaredCanonical"].as_str().map(String::from),
        sitemaps: index["sitemap"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        reason_code: Some(coverage_state.clone()),
        action: Some(action),
        priority: priority.into(),
    }
}

fn classify_record(verdict: &str, coverage_state: &str) -> (String, u8) {
    match verdict {
        "PASS" => ("No action needed".to_string(), 10),
        "FAIL" => match coverage_state {
            s if s.contains("Crawl") && s.contains("anomaly") => {
                ("Investigate crawl block".to_string(), 90)
            }
            s if s.contains("robots") || s.contains("Blocked") => {
                ("Fix robots.txt exclusion".to_string(), 80)
            }
            s if s.contains("noindex") => {
                ("Remove noindex tag".to_string(), 70)
            }
            s if s.contains("404") || s.contains("Not found") => {
                ("Fix 404 or add redirect".to_string(), 85)
            }
            s if s.contains("Redirect") => {
                ("Resolve redirect chain".to_string(), 60)
            }
            s if s.contains("Duplicate") || s.contains("canonical") => {
                ("Fix canonical tag".to_string(), 50)
            }
            _ => ("Review coverage state".to_string(), 40),
        },
        "NEUTRAL" => ("Monitor — soft indexing signal".to_string(), 20),
        _ => ("Manual review needed".to_string(), 30),
    }
}
