use crate::error::{Error, Result};
use crate::models::gsc::Coverage404Record;

/// Parse a GSC Coverage 404 CSV export. Expected columns (any order):
/// URL, Last crawled, Discovered, Category, Referring page
pub fn parse_coverage_csv(csv_content: &str) -> Result<Vec<Coverage404Record>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_content.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| Error::Other(format!("CSV header error: {}", e)))?
        .clone();

    let find = |name: &str| -> Option<usize> {
        headers
            .iter()
            .position(|h| h.to_lowercase().contains(name))
    };

    let url_idx = find("url").or_else(|| find("page")).unwrap_or(0);
    let crawl_idx = find("crawl").or_else(|| find("last crawled"));
    let category_idx = find("category");

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

        let category = category_idx
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let (reason, priority, suggested_action) = classify_404(&url, &category);
        let path = extract_path(&url);

        records.push(Coverage404Record {
            url,
            last_crawled,
            category,
            reason,
            priority: priority.into(),
            suggested_action,
            path,
        });
    }

    records.sort_by(|a, b| b.priority.cmp(&a.priority));
    Ok(records)
}

fn classify_404(url: &str, category: &str) -> (String, u8, String) {
    let url_lower = url.to_lowercase();
    let cat_lower = category.to_lowercase();

    if url_lower.contains('&') || url_lower.contains("%26") {
        return (
            "Malformed URL with ampersand".to_string(),
            30,
            "Ignore or add canonical".to_string(),
        );
    }
    if url_lower.contains('$') {
        return (
            "Malformed URL with dollar sign".to_string(),
            20,
            "Ignore — likely spam".to_string(),
        );
    }
    if url.contains('?') || url.contains("%3F") {
        return (
            "URL with query parameters".to_string(),
            40,
            "Review — add noindex or redirect to clean URL".to_string(),
        );
    }

    // Check for recognizable path patterns
    let segments: Vec<&str> = url.split('/').collect();
    let last = segments.last().unwrap_or(&"");

    if last.contains("undefined") || last.contains("null") {
        return (
            "URL with undefined/null segment".to_string(),
            25,
            "Fix front-end link generation".to_string(),
        );
    }

    if cat_lower.contains("old") || cat_lower.contains("deprecated") {
        return (
            "Old or deprecated content".to_string(),
            60,
            "Add 301 redirect to current page or remove link".to_string(),
        );
    }

    if cat_lower.contains("misspell") || cat_lower.contains("typo") {
        return (
            "Misspelled URL".to_string(),
            70,
            "Add 301 redirect to correct URL".to_string(),
        );
    }

    (
        "Not found — unknown cause".to_string(),
        50,
        "Investigate referring pages and add redirect if needed".to_string(),
    )
}

fn extract_path(url: &str) -> String {
    // Strip scheme and host manually to get the path component
    if let Some(idx) = url.find("://") {
        let rest = &url[idx + 3..];
        if let Some(slash) = rest.find('/') {
            return rest[slash..].to_string();
        }
        return "/".to_string();
    }
    url.to_string()
}
