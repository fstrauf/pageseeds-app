use serde_json::json;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::error::Result;
use crate::gsc::client::GscClient;
use crate::models::gsc::InspectionRecord;

/// Inspect a batch of URLs with limited concurrency (10 parallel requests).
///
/// The URL Inspection API does not support true batching, but requests are
/// independent so we parallelise them. A concurrency limit of 10 keeps us
/// well under GSC rate limits while cutting runtime by ~90% vs sequential.
pub async fn inspect_batch(
    token: &str,
    site_url: &str,
    urls: Vec<String>,
) -> Result<Vec<InspectionRecord>> {
    let client = GscClient::new(token);
    let semaphore = Arc::new(Semaphore::new(10));
    let total = urls.len();

    let mut handles = Vec::with_capacity(total);

    for url in urls {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let client = client.clone();
        let site_url = site_url.to_string();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let body = json!({
                "inspectionUrl": url,
                "siteUrl": site_url,
            });
            let resp = client.url_inspection_inspect(&body).await?;
            Ok::<_, crate::error::Error>(parse_inspection_record(&url, &resp))
        }));
    }

    let mut records = Vec::with_capacity(total);
    for (idx, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(record)) => {
                records.push(record);
                let n = idx + 1;
                if n == 1 || n % 25 == 0 || n == total {
                    log::info!("[collect_gsc] URL inspection progress: {}/{}", n, total);
                }
            }
            Ok(Err(e)) => return Err(e),
            Err(e) => {
                return Err(crate::error::Error::Other(format!(
                    "URL inspection task panicked: {}",
                    e
                )))
            }
        }
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
    let robots_txt_state = index["robotsTxtState"].as_str().unwrap_or("").to_string();
    let page_fetch_state = index["pageFetchState"].as_str().unwrap_or("").to_string();
    let crawl_allowed = index["crawlAllowed"].as_bool().unwrap_or(true);
    let indexing_allowed = index["indexingAllowed"].as_bool().unwrap_or(true);
    let google_canonical = index["googleCanonical"].as_str().map(String::from);
    let user_canonical = index["userDeclaredCanonical"].as_str().map(String::from);

    let (reason_code, action) = classify_record(
        url,
        crawl_allowed,
        &robots_txt_state,
        indexing_allowed,
        &page_fetch_state,
        user_canonical.as_deref(),
        google_canonical.as_deref(),
        &verdict,
        &coverage_state,
    );
    let priority = priority_for_record(reason_code);

    InspectionRecord {
        url: url.to_string(),
        verdict: Some(verdict),
        coverage_state: Some(coverage_state),
        indexing_state: Some(index["indexingState"].as_str().unwrap_or("").to_string()),
        robots_txt_state: Some(robots_txt_state),
        page_fetch_state: Some(page_fetch_state),
        crawl_allowed: Some(crawl_allowed),
        indexing_allowed: Some(indexing_allowed),
        last_crawl_time: index["lastCrawlTime"].as_str().map(String::from),
        google_canonical,
        user_canonical,
        sitemaps: index["sitemap"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        reason_code: Some(reason_code.to_string()),
        action: Some(action.to_string()),
        priority,
    }
}

/// Normalize a URL for reason-code comparison (scheme/www strip, lower, no trailing `/`).
///
/// Kept private next to classification so `gsc` does not depend on `engine`.
/// Same spirit as `engine::exec::gsc::normalize_url_for_comparison` plus trailing-slash trim.
fn normalize_url_for_reason(url: &str) -> String {
    let lower = url.to_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let without_www = without_scheme
        .strip_prefix("www.")
        .unwrap_or(without_scheme);
    without_www.trim_end_matches('/').to_string()
}

/// Classify an inspection record into a stable reason code and action description.
///
/// Inputs are the individual fields extracted from the API response plus the
/// inspected URL (needed to detect alternate-with-canonical pages).
/// Returns `(reason_code, action_description)`.
/// Priority order: first match wins (most critical first).
///
/// Mirrors the Python CLI `classify_record()` in `seo/gsc/indexing.py`, with
/// issue #250 alternate-canonical hygiene.
pub fn classify_record(
    inspected_url: &str,
    crawl_allowed: bool,
    robots_txt_state: &str,
    indexing_allowed: bool,
    page_fetch_state: &str,
    user_canonical: Option<&str>,
    google_canonical: Option<&str>,
    verdict: &str,
    coverage_state: &str,
) -> (&'static str, &'static str) {
    // 1. Robots / crawl block
    if !crawl_allowed || robots_txt_state.to_uppercase().contains("BLOCKED") {
        return (
            "robots_blocked",
            "Fix robots.txt / crawl allow; remove blocked URLs from sitemap until fixed.",
        );
    }

    // 2. Noindex
    if !indexing_allowed {
        return (
            "noindex",
            "Remove/avoid noindex; ensure indexing is allowed for canonical URLs.",
        );
    }

    // 3. Fetch errors (but not unspecified/ok states)
    let pfs_upper = page_fetch_state.to_uppercase();
    if !pfs_upper.is_empty()
        && pfs_upper != "OK"
        && pfs_upper != "SUCCESSFUL"
        && pfs_upper != "PAGE_FETCH_STATE_UNSPECIFIED"
    {
        return (
            "fetch_error",
            "Fix fetchability (4xx/5xx/soft404/redirect); remove broken URLs from sitemap.",
        );
    }

    // 4. Canonical mismatch (user-declared ≠ Google-selected) — wins over alternate
    if let (Some(user), Some(google)) = (user_canonical, google_canonical) {
        let user_norm = normalize_url_for_reason(user);
        let google_norm = normalize_url_for_reason(google);
        if !user_norm.is_empty() && !google_norm.is_empty() && user_norm != google_norm {
            return (
                "canonical_mismatch",
                "Align canonicals/redirects/internal links; ensure sitemap lists canonical URLs only.",
            );
        }
    }

    // 5. Alternate page with proper canonical (inspected URL ≠ Google-selected canonical)
    //    Not a content/interlinking fix target — consolidation/hygiene only (issue #250).
    if let Some(google) = google_canonical {
        let inspected_norm = normalize_url_for_reason(inspected_url);
        let google_norm = normalize_url_for_reason(google);
        if !google_norm.is_empty() && !inspected_norm.is_empty() && inspected_norm != google_norm {
            return (
                "alternate_with_canonical",
                "Consolidation/hygiene only — do not rewrite content on the alternate; verify 301 redirects, internal links, and sitemap list the Google-selected canonical.",
            );
        }
    }

    // 6. Non-PASS verdict — differentiate by coverage state
    if verdict.to_uppercase() != "PASS" {
        let cov_lower = coverage_state.to_lowercase();
        if cov_lower.contains("crawled") && cov_lower.contains("not") {
            return (
                "not_indexed_crawled",
                "Content quality/duplicate issue; improve uniqueness and internal links.",
            );
        }
        if cov_lower.contains("discovered") && cov_lower.contains("not") {
            return (
                "not_indexed_discovered",
                "Crawl budget/queue issue; improve internal links and content quality.",
            );
        }
        return (
            "not_indexed_other",
            "Triage via coverage/indexing states; improve internal links/content uniqueness.",
        );
    }

    // 7. Indexed — no action needed
    ("indexed_pass", "No action needed (indexed).")
}

/// Reasons that must not auto-spawn content/link fix work or inflate "needs work" counts.
///
/// Use at filter/spawn gates so callers share one list instead of divergent string matches.
pub fn is_non_actionable_reason(code: &str) -> bool {
    matches!(code, "indexed_pass" | "alternate_with_canonical")
}

/// Calculate priority for sorting.  Lower = more urgent.
///
/// Mirrors the Python CLI `priority_for_record()` in `seo/gsc/indexing.py`.
pub fn priority_for_record(reason_code: &str) -> i32 {
    match reason_code {
        "robots_blocked" | "noindex" | "fetch_error" => 10,
        "canonical_mismatch" => 20,
        "api_error" => 30,
        "not_indexed_crawled" => 40,
        "not_indexed_discovered" => 50,
        "not_indexed_other" => 70,
        // Low urgency: indexed OK and alternate-with-canonical hygiene (issue #250)
        "indexed_pass" | "alternate_with_canonical" => 999,
        _ => 999,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(
        inspected: &str,
        user: Option<&str>,
        google: Option<&str>,
        verdict: &str,
        coverage: &str,
    ) -> (&'static str, &'static str) {
        classify_record(
            inspected,
            true,
            "ALLOWED",
            true,
            "SUCCESSFUL",
            user,
            google,
            verdict,
            coverage,
        )
    }

    #[test]
    fn alternate_when_inspected_differs_from_google_canonical() {
        // Clean-slug migration: legacy URL inspected; user + Google agree on clean canonical.
        let (code, action) = classify(
            "https://example.com/old-slug",
            Some("https://example.com/clean-slug"),
            Some("https://example.com/clean-slug"),
            "NEUTRAL",
            "Alternative page with proper canonical tag",
        );
        assert_eq!(code, "alternate_with_canonical");
        assert!(action.contains("hygiene") || action.contains("Consolidation"));
        assert_eq!(priority_for_record(code), 999);
        assert!(is_non_actionable_reason(code));
    }

    #[test]
    fn alternate_normalizes_scheme_www_and_trailing_slash() {
        let (code, _) = classify(
            "https://www.example.com/old-slug/",
            None,
            Some("http://example.com/clean-slug"),
            "NEUTRAL",
            "Excluded",
        );
        assert_eq!(code, "alternate_with_canonical");
    }

    #[test]
    fn canonical_mismatch_wins_over_alternate() {
        // user-declared ≠ google-selected, and inspected also ≠ google
        let (code, _) = classify(
            "https://example.com/page-a",
            Some("https://example.com/page-a"),
            Some("https://example.com/page-b"),
            "NEUTRAL",
            "Excluded",
        );
        assert_eq!(code, "canonical_mismatch");
        assert!(!is_non_actionable_reason(code));
        assert_eq!(priority_for_record(code), 20);
    }

    #[test]
    fn inspected_equals_google_non_pass_still_not_indexed() {
        let (code, _) = classify(
            "https://example.com/clean-slug",
            Some("https://example.com/clean-slug"),
            Some("https://example.com/clean-slug"),
            "NEUTRAL",
            "Crawled - currently not indexed",
        );
        assert_eq!(code, "not_indexed_crawled");
        assert!(!is_non_actionable_reason(code));
    }

    #[test]
    fn inspected_equals_google_pass_is_indexed_pass() {
        let (code, _) = classify(
            "https://example.com/clean-slug/",
            Some("https://example.com/clean-slug"),
            Some("https://www.example.com/clean-slug"),
            "PASS",
            "Submitted and indexed",
        );
        assert_eq!(code, "indexed_pass");
        assert!(is_non_actionable_reason(code));
    }

    #[test]
    fn is_non_actionable_only_pass_and_alternate() {
        assert!(is_non_actionable_reason("indexed_pass"));
        assert!(is_non_actionable_reason("alternate_with_canonical"));
        assert!(!is_non_actionable_reason("not_indexed_other"));
        assert!(!is_non_actionable_reason("not_indexed_discovered"));
        assert!(!is_non_actionable_reason("canonical_mismatch"));
        assert!(!is_non_actionable_reason(""));
    }
}
