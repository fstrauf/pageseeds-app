/// Fetch and parse sitemap XML to extract page URLs.
///
/// Supports both single sitemaps and `<sitemapindex>` documents (one level deep).
use crate::error::{Error, Result};

/// Fetch sitemap XML from `sitemap_url` and return all `<loc>` URLs, capped at `limit`.
///
/// Follows one level of `<sitemapindex>` (nested child sitemaps).
pub async fn fetch_sitemap_urls(sitemap_url: &str, limit: usize) -> Result<Vec<String>> {
    let body = fetch_text(sitemap_url).await?;

    let urls = if is_sitemap_index(&body) {
        // Follow nested sitemaps one level deep
        let child_sitemaps = extract_locs(&body);
        let mut all_urls = Vec::new();
        for child_url in child_sitemaps.iter().take(10) {
            match fetch_text(child_url).await {
                Ok(child_body) => {
                    let mut child_urls = extract_locs(&child_body);
                    all_urls.append(&mut child_urls);
                }
                Err(e) => {
                    log::warn!(
                        "[sitemap] Failed to fetch child sitemap {}: {}",
                        child_url,
                        e
                    );
                }
            }
            if all_urls.len() >= limit {
                break;
            }
        }
        all_urls
    } else {
        extract_locs(&body)
    };

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<String> = urls
        .into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect();

    Ok(deduped.into_iter().take(limit).collect())
}

async fn fetch_text(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; PageSeeds/1.0)")
        .build()
        .map_err(|e| Error::Other(e.to_string()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Other(format!("Failed to fetch sitemap '{}': {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Sitemap request to '{}' returned HTTP {}",
            url,
            resp.status()
        )));
    }

    resp.text()
        .await
        .map_err(|e| Error::Other(format!("Failed to read sitemap body: {}", e)))
}

fn is_sitemap_index(body: &str) -> bool {
    body.contains("<sitemapindex")
}

/// Extract all `<loc>` values from a sitemap or sitemapindex XML string.
fn extract_locs(body: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut remaining = body;
    while let Some(start) = remaining.find("<loc>") {
        let after_tag = &remaining[start + 5..];
        if let Some(end) = after_tag.find("</loc>") {
            let url = after_tag[..end].trim().to_string();
            if !url.is_empty() {
                urls.push(url);
            }
            remaining = &after_tag[end + 6..];
        } else {
            break;
        }
    }
    urls
}
