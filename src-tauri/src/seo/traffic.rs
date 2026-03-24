use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::seo::solve_ahrefs_captcha;

// ─── Data structures ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficMonthly {
    pub traffic_monthly_avg: f64,
    pub cost_monthly_avg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficTopPage {
    pub url: Option<String>,
    pub traffic: Option<f64>,
    pub keywords: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficTopKeyword {
    pub keyword: Option<String>,
    pub traffic: Option<f64>,
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficTopCountry {
    pub country: Option<String>,
    pub traffic: Option<f64>,
    pub share: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficResult {
    pub domain: String,
    pub traffic: TrafficMonthly,
    /// Raw traffic history data from Ahrefs (list of {date, organic} objects)
    pub traffic_history: Vec<Value>,
    pub top_pages: Vec<TrafficTopPage>,
    pub top_countries: Vec<TrafficTopCountry>,
    pub top_keywords: Vec<TrafficTopKeyword>,
}

// ─── Parsers ──────────────────────────────────────────────────────────────────

fn parse_top_pages(data: &Value) -> Vec<TrafficTopPage> {
    data.as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|p| TrafficTopPage {
            url: p["url"].as_str().map(|s| s.to_string()),
            traffic: p["traffic"].as_f64(),
            keywords: p["keywords"].as_i64(),
        })
        .collect()
}

fn parse_top_keywords(data: &Value) -> Vec<TrafficTopKeyword> {
    data.as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|k| TrafficTopKeyword {
            keyword: k["keyword"].as_str().map(|s| s.to_string()),
            traffic: k["traffic"].as_f64(),
            position: k["position"].as_f64(),
        })
        .collect()
}

fn parse_top_countries(data: &Value) -> Vec<TrafficTopCountry> {
    data.as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|c| TrafficTopCountry {
            country: c["country"].as_str().map(|s| s.to_string()),
            traffic: c["traffic"].as_f64(),
            share: c["share"].as_f64(),
        })
        .collect()
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Check estimated search traffic for a domain or URL via the Ahrefs free traffic checker.
///
/// * `mode` — `"subdomains"` (default) or `"exact"`
/// * `country` — ISO country code or `"None"` for worldwide
pub async fn check_traffic(
    capsolver_key: &str,
    domain_or_url: &str,
    mode: &str,
    country: &str,
) -> Result<TrafficResult> {
    let site_url = format!(
        "https://ahrefs.com/traffic-checker/?input={}&mode={}",
        urlencoding::encode(domain_or_url),
        mode
    );
    let token = solve_ahrefs_captcha(capsolver_key, &site_url).await?;

    let input = serde_json::json!({
        "captcha": token,
        "country": country,
        "protocol": "None",
        "mode": mode,
        "url": domain_or_url
    });

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .build()
        .map_err(Error::Http)?;

    let resp = client
        .get("https://ahrefs.com/v4/stGetFreeTrafficOverview")
        .query(&[("input", input.to_string())])
        .header("accept", "*/*")
        .header(
            "referer",
            format!(
                "https://ahrefs.com/traffic-checker/?input={}&mode={}",
                domain_or_url, mode
            ),
        )
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Ahrefs traffic overview returned status {}",
            resp.status()
        )));
    }

    let data: Value = resp.json().await?;

    // Response: ["Ok", { traffic: { trafficMonthlyAvg, costMontlyAvg }, traffic_history, top_pages, ... }]
    let traffic_data = match &data {
        Value::Array(arr) if arr.len() >= 2 && arr[0].as_str() == Some("Ok") => &arr[1],
        _ => {
            return Err(Error::Other(
                "Unexpected traffic response format".to_string(),
            ))
        }
    };

    let traffic_monthly_avg = traffic_data["traffic"]["trafficMonthlyAvg"]
        .as_f64()
        .unwrap_or(0.0);
    let cost_monthly_avg = traffic_data["traffic"]["costMontlyAvg"]
        .as_f64()
        .unwrap_or(0.0);

    let traffic_history = traffic_data["traffic_history"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    Ok(TrafficResult {
        domain: domain_or_url.to_string(),
        traffic: TrafficMonthly {
            traffic_monthly_avg,
            cost_monthly_avg,
        },
        traffic_history,
        top_pages: parse_top_pages(&traffic_data["top_pages"]),
        top_countries: parse_top_countries(&traffic_data["top_countries"]),
        top_keywords: parse_top_keywords(&traffic_data["top_keywords"]),
    })
}
