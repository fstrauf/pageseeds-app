use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::seo::solve_ahrefs_captcha;

// ─── Data structures ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacklinkItem {
    pub anchor: String,
    pub domain_rating: f64,
    pub title: String,
    pub url_from: String,
    pub url_to: String,
    pub edu: bool,
    pub gov: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainOverview {
    pub domain_rating: Option<f64>,
    pub traffic: Option<f64>,
    pub referring_domains: Option<i64>,
    pub backlinks: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacklinksResult {
    pub domain: String,
    pub overview: Option<DomainOverview>,
    pub backlinks: Vec<BacklinkItem>,
}

/// In-memory cached Ahrefs backlinks signature with ISO expiry string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSignature {
    pub signature: String,
    pub valid_until: String, // ISO 8601
    pub overview: Option<DomainOverview>,
}

impl CachedSignature {
    /// Returns true if the signature has not yet expired.
    pub fn is_valid(&self) -> bool {
        let s = self.valid_until.trim_end_matches('Z').to_string() + "+00:00";
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s) {
            chrono::Utc::now() < dt.with_timezone(&chrono::Utc)
        } else {
            false
        }
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn parse_overview(data: &Value) -> Option<DomainOverview> {
    let dr = data["domainRating"]
        .as_f64()
        .or_else(|| data["dr"].as_f64());
    let traffic = data["traffic"]
        .as_f64()
        .or_else(|| data["organicTraffic"].as_f64());
    let rd = data["linkedDomains"]
        .as_i64()
        .or_else(|| data["refdomains"].as_i64())
        .or_else(|| data["referring_domains"].as_i64());
    let bl = data["backlinks"].as_i64();

    Some(DomainOverview {
        domain_rating: dr,
        traffic,
        referring_domains: rd,
        backlinks: bl,
    })
}

/// Acquire a fresh Ahrefs backlinks signature by solving a CapSolver challenge and calling
/// the `stGetFreeBacklinksOverview` endpoint.
async fn acquire_signature(
    capsolver_key: &str,
    domain: &str,
) -> Result<(String, String, Option<DomainOverview>)> {
    let site_url = format!(
        "https://ahrefs.com/backlink-checker/?input={}",
        urlencoding::encode(domain)
    );
    let token = solve_ahrefs_captcha(capsolver_key, &site_url).await?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .build()
        .map_err(Error::Http)?;

    let resp = client
        .post("https://ahrefs.com/v4/stGetFreeBacklinksOverview")
        .json(&serde_json::json!({
            "captcha": token,
            "mode": "subdomains",
            "url": domain
        }))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body_preview = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect::<String>();
        log::warn!(
            "[acquire_signature] Ahrefs returned {} for domain '{}'. Body preview: {}",
            status,
            domain,
            body_preview
        );
        return Err(Error::Other(format!(
            "Ahrefs backlinks overview returned status {}. Body: {}",
            status, body_preview
        )));
    }

    let data: Value = resp.json().await?;

    // Response: ["Ok", { signedInput: { signature, input: { validUntil } }, data: {...} }]
    let inner = match &data {
        Value::Array(arr) if arr.len() >= 2 && arr[0].as_str() == Some("Ok") => &arr[1],
        _ => {
            return Err(Error::Other(
                "Unexpected backlinks overview response format".to_string(),
            ))
        }
    };

    let signature = inner["signedInput"]["signature"]
        .as_str()
        .ok_or_else(|| Error::Other("Missing signature in backlinks response".to_string()))?
        .to_string();

    let valid_until = inner["signedInput"]["input"]["validUntil"]
        .as_str()
        .ok_or_else(|| Error::Other("Missing validUntil in backlinks response".to_string()))?
        .to_string();

    let overview = inner.get("data").and_then(parse_overview);

    Ok((signature, valid_until, overview))
}

/// Fetch the top backlinks list using a previously acquired signature.
async fn fetch_backlinks_list(
    signature: &str,
    valid_until: &str,
    domain: &str,
) -> Result<Vec<BacklinkItem>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .build()
        .map_err(Error::Http)?;

    let resp = client
        .post("https://ahrefs.com/v4/stGetFreeBacklinksList")
        .json(&serde_json::json!({
            "reportType": "TopBacklinks",
            "signedInput": {
                "signature": signature,
                "input": {
                    "validUntil": valid_until,
                    "mode": "subdomains",
                    "url": format!("{}/", domain)
                }
            }
        }))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body_preview = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect::<String>();
        log::warn!(
            "[fetch_backlinks_list] Ahrefs returned {} for domain '{}'. Body preview: {}",
            status,
            domain,
            body_preview
        );
        return Ok(vec![]);
    }

    let data: Value = resp.json().await?;

    let backlinks = match &data {
        Value::Array(arr) if arr.len() >= 2 => {
            arr[1]["topBacklinks"]["backlinks"].as_array().cloned()
        }
        _ => None,
    };

    Ok(backlinks
        .unwrap_or_default()
        .iter()
        .map(|bl| BacklinkItem {
            anchor: bl["anchor"].as_str().unwrap_or("").to_string(),
            domain_rating: bl["domainRating"].as_f64().unwrap_or(0.0),
            title: bl["title"].as_str().unwrap_or("").to_string(),
            url_from: bl["urlFrom"].as_str().unwrap_or("").to_string(),
            url_to: bl["urlTo"].as_str().unwrap_or("").to_string(),
            edu: bl["edu"].as_bool().unwrap_or(false),
            gov: bl["gov"].as_bool().unwrap_or(false),
        })
        .collect())
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Fetch backlinks for a domain, using a cached signature when valid.
/// `sig_cache` is the in-memory map held in `SeoState`.
pub async fn get_backlinks(
    capsolver_key: &str,
    domain: &str,
    sig_cache: &std::sync::Mutex<HashMap<String, CachedSignature>>,
) -> Result<BacklinksResult> {
    // Check cache
    let cached = {
        let cache = sig_cache.lock().map_err(|e| Error::Other(e.to_string()))?;
        cache.get(domain).cloned()
    };

    let (signature, valid_until, overview) = if let Some(c) = cached.filter(|c| c.is_valid()) {
        (c.signature, c.valid_until, c.overview)
    } else {
        let (sig, vu, ov) = acquire_signature(capsolver_key, domain).await?;
        // Store in cache
        {
            let mut cache = sig_cache.lock().map_err(|e| Error::Other(e.to_string()))?;
            cache.insert(
                domain.to_string(),
                CachedSignature {
                    signature: sig.clone(),
                    valid_until: vu.clone(),
                    overview: ov.clone(),
                },
            );
        }
        (sig, vu, ov)
    };

    let backlinks = fetch_backlinks_list(&signature, &valid_until, domain).await?;

    Ok(BacklinksResult {
        domain: domain.to_string(),
        overview,
        backlinks,
    })
}
