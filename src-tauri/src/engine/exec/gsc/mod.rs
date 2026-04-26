/// GSC sync and collection execution module.
///
/// Covers:
///   - exec_gsc_sync_articles              (sync GSC metrics → articles.json)
///   - exec_collect_gsc                    (URL Inspection API → gsc_collection.json)
///   - exec_gsc_investigate                (agentic investigation of collection results)
///   - create_tasks_from_collection_after_exec  (post-completion task spawner)
///   - create_tasks_from_collection        (parse gsc_collection.json → fix tasks)
///   - normalize_site_for_url_match        (sc-domain: normalisation)
///   - normalize_url_for_comparison        (URL normalization for domain matching)

mod collect;
mod investigate;
mod sync;
mod task_spawner;

pub(crate) use collect::*;
pub(crate) use investigate::*;
pub(crate) use sync::*;
pub(crate) use task_spawner::*;

/// Normalize a URL for domain comparison by stripping scheme and www.
///
/// This ensures URLs like `https://www.example.com/page` and `https://example.com/page`
/// are treated as belonging to the same domain for validation purposes.
///
/// # Examples
/// - `https://www.example.com/page` → `example.com/page`
/// - `http://example.com/` → `example.com/`
/// - `https://EXAMPLE.COM` → `example.com`
pub(crate) fn normalize_url_for_comparison(url: &str) -> String {
    let lower = url.to_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    without_scheme.strip_prefix("www.").unwrap_or(without_scheme).to_string()
}

/// Normalise a GSC property identifier into a URL prefix suitable for `starts_with`.
///
/// Converts various GSC site formats to a canonical `https://domain` prefix,
/// stripping `www.` to ensure consistent matching regardless of subdomain.
///
/// - `sc-domain:example.com` → `https://example.com`
/// - `sc-domain:www.example.com` → `https://example.com`
/// - `https://example.com/`  → `https://example.com`
/// - `https://www.example.com/` → `https://example.com`
/// - `http://example.com` → `https://example.com`
pub(crate) fn normalize_site_for_url_match(site_url: &str) -> String {
    // Strip sc-domain: prefix if present
    let without_prefix = site_url.strip_prefix("sc-domain:").unwrap_or(site_url);
    
    // Use shared normalization for scheme/www stripping, then reconstruct as https://
    let normalized = normalize_url_for_comparison(without_prefix);
    format!("https://{}", normalized.trim_start_matches('/'))
}
