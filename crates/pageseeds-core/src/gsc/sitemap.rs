/// Fetch and parse sitemap XML to extract page URLs.
///
/// Supports both single sitemaps and `<sitemapindex>` documents (one level deep).
use crate::error::{Error, Result};

/// A single entry from a sitemap: URL + optional lastmod.
#[derive(Debug, Clone)]
pub struct SitemapEntry {
    pub url: String,
    pub lastmod: Option<String>,
}

/// Fetch sitemap XML from `sitemap_url` and return all `<loc>` URLs, capped at `limit`.
///
/// Follows one level of `<sitemapindex>` (nested child sitemaps).
pub async fn fetch_sitemap_urls(sitemap_url: &str, limit: usize) -> Result<Vec<String>> {
    let entries = fetch_sitemap_entries(sitemap_url, limit).await?;
    Ok(entries.into_iter().map(|e| e.url).collect())
}

/// Fetch sitemap XML and return full entries including `<lastmod>` dates.
pub async fn fetch_sitemap_entries(sitemap_url: &str, limit: usize) -> Result<Vec<SitemapEntry>> {
    let body = fetch_text(sitemap_url).await?;

    let entries = if is_sitemap_index(&body) {
        // Follow nested sitemaps one level deep
        let child_sitemaps = extract_locs(&body);
        let mut all_entries = Vec::new();
        for child_url in child_sitemaps.iter().take(10) {
            match fetch_text(child_url).await {
                Ok(child_body) => {
                    let mut child_entries = extract_entries(&child_body);
                    all_entries.append(&mut child_entries);
                }
                Err(e) => {
                    log::warn!(
                        "[sitemap] Failed to fetch child sitemap {}: {}",
                        child_url,
                        e
                    );
                }
            }
            if all_entries.len() >= limit {
                break;
            }
        }
        all_entries
    } else {
        extract_entries(&body)
    };

    // Deduplicate by URL while preserving order and first-seen lastmod
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<SitemapEntry> = entries
        .into_iter()
        .filter(|e| seen.insert(e.url.clone()))
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

/// Extract `<url>` entries with both `<loc>` and `<lastmod>` from a sitemap XML string.
fn extract_entries(body: &str) -> Vec<SitemapEntry> {
    let mut entries = Vec::new();
    let mut remaining = body;

    while let Some(url_start) = remaining.find("<url>") {
        let after_url = &remaining[url_start + 5..];
        let url_end = match after_url.find("</url>") {
            Some(end) => end,
            None => break,
        };
        let url_block = &after_url[..url_end];

        // Extract <loc>
        let loc = if let Some(loc_start) = url_block.find("<loc>") {
            let after_loc = &url_block[loc_start + 5..];
            if let Some(loc_end) = after_loc.find("</loc>") {
                after_loc[..loc_end].trim().to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Extract <lastmod>
        let lastmod = if let Some(lm_start) = url_block.find("<lastmod>") {
            let after_lm = &url_block[lm_start + 9..];
            if let Some(lm_end) = after_lm.find("</lastmod>") {
                let lm = after_lm[..lm_end].trim().to_string();
                if lm.is_empty() {
                    None
                } else {
                    Some(lm)
                }
            } else {
                None
            }
        } else {
            None
        };

        if !loc.is_empty() {
            entries.push(SitemapEntry { url: loc, lastmod });
        }

        remaining = &after_url[url_end + 6..];
    }

    // Fallback: if no <url> blocks found, treat as plain loc list
    if entries.is_empty() {
        for loc in extract_locs(body) {
            entries.push(SitemapEntry {
                url: loc,
                lastmod: None,
            });
        }
    }

    entries
}
