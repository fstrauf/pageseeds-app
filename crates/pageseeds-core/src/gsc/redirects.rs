use crate::error::{Error, Result};
use crate::models::gsc::RedirectRecord;

/// Parse a GSC "Page with redirect" CSV export.
/// Expected columns (any order): URL, Last crawled, Referring page, Redirect type
pub fn parse_redirect_csv(csv_content: &str) -> Result<Vec<RedirectRecord>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_content.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| Error::Other(format!("CSV header error: {}", e)))?
        .clone();

    let find = |name: &str| -> Option<usize> {
        headers.iter().position(|h| h.to_lowercase().contains(name))
    };

    let url_idx = find("url").or_else(|| find("page")).unwrap_or(0);
    let crawl_idx = find("crawl").or_else(|| find("last crawled"));
    let redirect_idx = find("redirect").or_else(|| find("type"));
    let final_idx = find("final").or_else(|| find("destination"));

    let mut records = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| Error::Other(format!("CSV row error: {}", e)))?;

        let url = record.get(url_idx).unwrap_or("").trim().to_string();
        if url.is_empty() {
            continue;
        }

        let last_crawled = crawl_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string());

        let redirect_type_raw = redirect_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let final_url = final_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string());

        let (redirect_type, issue, priority, suggested_action) =
            classify_redirect(&url, &redirect_type_raw, final_url.as_deref());

        records.push(RedirectRecord {
            url,
            last_crawled,
            redirect_type,
            issue,
            priority: priority.into(),
            suggested_action,
            final_url,
        });
    }

    records.sort_by(|a, b| b.priority.cmp(&a.priority));
    Ok(records)
}

fn classify_redirect(
    url: &str,
    redirect_type_raw: &str,
    final_url: Option<&str>,
) -> (String, String, u8, String) {
    let url_lower = url.to_lowercase();

    // HTTP → HTTPS
    if url_lower.starts_with("http://") {
        return (
            "Protocol redirect".to_string(),
            "HTTP to HTTPS redirect".to_string(),
            70,
            "Update all internal links to HTTPS".to_string(),
        );
    }

    // www canonicalization
    if let (Some(fin), true) = (
        final_url,
        url_lower.contains("www.") != url.contains("www."),
    ) {
        let _ = fin;
        return (
            "www canonicalization".to_string(),
            "www / non-www inconsistency".to_string(),
            60,
            "Standardize on one canonical form and update links".to_string(),
        );
    }

    // Trailing slash
    if let Some(fin) = final_url {
        let u_trail = url.ends_with('/');
        let f_trail = fin.ends_with('/');
        if u_trail != f_trail {
            return (
                "Trailing slash redirect".to_string(),
                "Trailing slash inconsistency".to_string(),
                50,
                "Standardize trailing slash in CMS/config and update internal links".to_string(),
            );
        }
    }

    // Guess from redirect type field
    let rt_lower = redirect_type_raw.to_lowercase();
    if rt_lower.contains("301") || rt_lower.contains("permanent") {
        return (
            "301 Permanent redirect".to_string(),
            "Page permanently moved".to_string(),
            40,
            "Update internal links to point directly to final URL".to_string(),
        );
    }
    if rt_lower.contains("302") || rt_lower.contains("temporary") {
        return (
            "302 Temporary redirect".to_string(),
            "Temporary redirect — should be permanent".to_string(),
            80,
            "Change to 301 redirect unless intentionally temporary".to_string(),
        );
    }

    (
        "Unknown redirect".to_string(),
        "Redirect destination unclear".to_string(),
        30,
        "Review redirect chain manually".to_string(),
    )
}
