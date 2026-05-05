/// Deterministic sitemap ↔ GSC drift detection.
///
/// Compares the site's sitemap against GSC indexing status and produces:
///   - URLs in sitemap but missing from GSC entirely
///   - URLs in GSC but not in sitemap
///   - URLs that are not indexed (with reason breakdown)
///   - A prioritized list of resubmission candidates
///
/// Reads:
///   - manifest.json (site_url + sitemap_url)
///   - sitemap.xml (live fetch)
///   - gsc_collection.json (latest URL inspection results)
///   - link_scan.json (internal link graph)
///   - articles.json (article metadata for scoring)
use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};

/// Internal representation of a GSC item for drift matching.
#[derive(Debug, Clone)]
struct GscItem {
    url: String,
    reason_code: Option<String>,
    verdict: Option<String>,
}

/// Run the full drift analysis for a project.
pub(crate) async fn exec_gsc_drift(
    project_id: &str,
    project_path: &str,
) -> Result<GscDriftReport, crate::error::Error> {
    let paths = ProjectPaths::from_path(project_path);

    // 1. Resolve site config
    let (site_url, sitemap_url) = resolve_site_config(project_id, project_path)?;

    // 2. Fetch sitemap URLs (high limit — drift detection needs completeness)
    let sitemap_urls = crate::gsc::sitemap::fetch_sitemap_urls(&sitemap_url, 5000).await?;

    // 3. Load GSC inspection data
    let gsc_items = load_gsc_items(&paths, project_id)?;

    // 4. Load link scan data
    let link_scan = load_link_scan(&paths);

    // 5. Load article metadata
    let articles = load_articles(&paths);

    // 6. Build normalized lookup maps
    let mut sitemap_map: HashMap<String, String> = HashMap::new(); // normalized → original
    for url in &sitemap_urls {
        let norm = crate::engine::exec::gsc::normalize_url_for_comparison(url);
        sitemap_map.insert(norm, url.clone());
    }

    let mut gsc_map: HashMap<String, GscItem> = HashMap::new();
    for item in &gsc_items {
        let norm = crate::engine::exec::gsc::normalize_url_for_comparison(&item.url);
        gsc_map.insert(norm, item.clone());
    }

    // 7. Compute drift categories
    let mut in_sitemap_not_in_gsc: Vec<DriftUrl> = Vec::new();
    let mut in_gsc_not_in_sitemap: Vec<DriftUrl> = Vec::new();
    let mut not_indexed: Vec<DriftUrl> = Vec::new();
    let mut indexed_count = 0usize;

    for (norm, original_url) in &sitemap_map {
        if let Some(gsc_item) = gsc_map.get(norm) {
            if gsc_item.reason_code.as_deref() == Some("indexed_pass") {
                indexed_count += 1;
            } else {
                not_indexed.push(DriftUrl {
                    url: original_url.clone(),
                    slug: extract_slug(original_url),
                    reason_code: gsc_item.reason_code.clone(),
                    verdict: gsc_item.verdict.clone(),
                });
            }
        } else {
            in_sitemap_not_in_gsc.push(DriftUrl {
                url: original_url.clone(),
                slug: extract_slug(original_url),
                reason_code: None,
                verdict: None,
            });
        }
    }

    for (norm, gsc_item) in &gsc_map {
        if !sitemap_map.contains_key(norm) {
            in_gsc_not_in_sitemap.push(DriftUrl {
                url: gsc_item.url.clone(),
                slug: extract_slug(&gsc_item.url),
                reason_code: gsc_item.reason_code.clone(),
                verdict: gsc_item.verdict.clone(),
            });
        }
    }

    // 8. Build resubmit priority list
    let mut candidates: Vec<ResubmitCandidate> = Vec::new();

    for drift_url in &not_indexed {
        if let Some(candidate) = build_candidate(drift_url, &link_scan, &articles) {
            candidates.push(candidate);
        }
    }

    for drift_url in &in_sitemap_not_in_gsc {
        let mut candidate = build_candidate_for_unknown(drift_url, &link_scan, &articles);
        candidate.priority_score += 25; // Boost: GSC has never seen this URL
        candidate.priority_reason = format!(
            "{} — URL is in sitemap but GSC has never inspected it",
            candidate.priority_reason
        );
        candidates.push(candidate);
    }

    // Sort by priority score descending
    candidates.sort_by(|a, b| b.priority_score.cmp(&a.priority_score));

    let not_indexed_count = not_indexed.len();

    Ok(GscDriftReport {
        site_url,
        sitemap_url,
        checked_at: chrono::Utc::now().to_rfc3339(),
        sitemap_total: sitemap_urls.len(),
        gsc_total: gsc_items.len(),
        indexed_count,
        not_indexed_count,
        in_sitemap_not_in_gsc,
        in_gsc_not_in_sitemap,
        not_indexed,
        resubmit_priority: candidates,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data loaders
// ═══════════════════════════════════════════════════════════════════════════════

fn resolve_site_config(project_id: &str, project_path: &str) -> Result<(String, String), crate::error::Error> {
    let paths = ProjectPaths::from_path(project_path);
    let manifest_path = paths.automation_dir.join("manifest.json");

    if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(site_url) = manifest
                .get("gsc_site")
                .or_else(|| manifest.get("url"))
                .and_then(|v| v.as_str())
                .map(String::from)
            {
                let sitemap_url = manifest
                    .get("sitemap")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        let base = if site_url.starts_with("sc-domain:") {
                            format!("https://{}", &site_url["sc-domain:".len()..])
                        } else if !site_url.starts_with("http://") && !site_url.starts_with("https://") {
                            format!("https://{}", site_url)
                        } else {
                            site_url.clone()
                        };
                        format!("{}/sitemap.xml", base.trim_end_matches('/'))
                    });
                return Ok((site_url, sitemap_url));
            }
        }
    }

    // Fallback: query projects table
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open(&db_path)?;
    let project = crate::engine::task_store::get_project(&conn, project_id)?;

    let site_url = project
        .site_url
        .filter(|s| !s.is_empty())
        .ok_or_else(|| crate::error::Error::Other("No site_url configured".to_string()))?;

    let sitemap_url = project
        .sitemap_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}/sitemap.xml", site_url.trim_end_matches('/')));

    Ok((site_url, sitemap_url))
}

fn load_gsc_items(
    paths: &ProjectPaths,
    project_id: &str,
) -> Result<Vec<GscItem>, crate::error::Error> {
    // Prefer gsc_collection.json (most recent bulk inspection)
    let collection_path = paths.automation_dir.join("gsc_collection.json");
    if let Ok(raw) = std::fs::read_to_string(&collection_path) {
        if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(items) = doc["items"].as_array() {
                let gsc_items: Vec<GscItem> = items
                    .iter()
                    .map(|item| GscItem {
                        url: item["url"].as_str().unwrap_or("").to_string(),
                        reason_code: item["reason_code"].as_str().map(String::from),
                        verdict: item["verdict"].as_str().map(String::from),
                    })
                    .filter(|i| !i.url.is_empty())
                    .collect();
                if !gsc_items.is_empty() {
                    return Ok(gsc_items);
                }
            }
        }
    }

    // Fallback: SQLite gsc_url_indexing_status table
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open(&db_path)?;
    let rows = crate::gsc::db::list_by_project(&conn, project_id)?;
    Ok(rows
        .into_iter()
        .map(|r| GscItem {
            url: r.url,
            reason_code: r.last_reason_code,
            verdict: r.last_verdict,
        })
        .collect())
}

#[derive(Debug, Default)]
struct LinkScanData {
    incoming_by_id: HashMap<i64, usize>,
}

fn load_link_scan(paths: &ProjectPaths) -> LinkScanData {
    let scan_path = paths.automation_dir.join("link_scan.json");
    let raw = match std::fs::read_to_string(&scan_path) {
        Ok(r) => r,
        Err(_) => return LinkScanData::default(),
    };
    let doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(d) => d,
        Err(_) => return LinkScanData::default(),
    };

    let mut incoming_by_id: HashMap<i64, usize> = HashMap::new();
    if let Some(profiles) = doc["profiles"].as_array() {
        for profile in profiles {
            if let Some(id) = profile["id"].as_i64() {
                let count = profile["incoming_ids"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                incoming_by_id.insert(id, count);
            }
        }
    }

    LinkScanData { incoming_by_id }
}

#[derive(Debug, Default, Clone)]
struct ArticleMeta {
    id: i64,
    url_slug: String,
    target_keyword: Option<String>,
    published_date: Option<String>,
    gsc_impressions: Option<f64>,
}

fn load_articles(paths: &ProjectPaths) -> HashMap<String, ArticleMeta> {
    let articles_path = paths.automation_dir.join("articles.json");
    let raw = match std::fs::read_to_string(&articles_path) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };
    let doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };

    let empty = vec![];
    let arr = if doc.is_array() {
        doc.as_array().unwrap_or(&empty)
    } else {
        doc.get("articles").and_then(|v| v.as_array()).unwrap_or(&empty)
    };

    let mut map: HashMap<String, ArticleMeta> = HashMap::new();
    for article in arr {
        let id = article["id"].as_i64().unwrap_or(0);
        let slug = article["url_slug"].as_str().unwrap_or("").to_string();
        if slug.is_empty() {
            continue;
        }
        let gsc = &article["gsc"];
        let impressions = if gsc.is_object() {
            gsc["impressions"].as_f64()
        } else {
            None
        };

        map.insert(
            slug.clone(),
            ArticleMeta {
                id,
                url_slug: slug,
                target_keyword: article["target_keyword"].as_str().map(String::from),
                published_date: article["published_date"].as_str().map(String::from),
                gsc_impressions: impressions,
            },
        );
    }

    map
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn extract_slug(url: &str) -> String {
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if let Some(pos) = without_scheme.find('/') {
        without_scheme[pos + 1..].to_string()
    } else {
        without_scheme.to_string()
    }
}

fn build_candidate(
    drift_url: &DriftUrl,
    link_scan: &LinkScanData,
    articles: &HashMap<String, ArticleMeta>,
) -> Option<ResubmitCandidate> {
    let reason = drift_url.reason_code.as_deref().unwrap_or("unknown");

    // Skip technical blockers — resubmission won't help
    if matches!(reason, "robots_blocked" | "noindex" | "fetch_error" | "canonical_mismatch") {
        return None;
    }

    let meta = articles.get(&drift_url.slug).or_else(|| {
        // Try matching by last segment
        let last = drift_url.slug.trim_end_matches('/').rsplit('/').next()?;
        articles.get(last)
    });

    let incoming_link_count = meta
        .and_then(|m| link_scan.incoming_by_id.get(&m.id).copied())
        .unwrap_or(0);

    let base_score = match reason {
        "not_indexed_other" => 100,
        "not_indexed_discovered" => 70,
        "not_indexed_crawled" => 40,
        _ => 20,
    };

    let link_bonus = if incoming_link_count == 0 { 30 } else { 0 };
    let gsc_bonus = if meta.and_then(|m| m.gsc_impressions).unwrap_or(0.0) > 0.0 {
        20
    } else {
        0
    };

    let age_bonus = meta
        .and_then(|m| m.published_date.as_deref())
        .and_then(|d| {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
        })
        .map(|published| {
            let days = (chrono::Utc::now().date_naive() - published).num_days();
            if days > 30 { 15 } else { 0 }
        })
        .unwrap_or(0);

    let score = base_score + link_bonus + gsc_bonus + age_bonus;

    let mut reasons: Vec<&str> = Vec::new();
    reasons.push(match reason {
        "not_indexed_other" => "unknown to Google",
        "not_indexed_discovered" => "discovered but not crawled",
        "not_indexed_crawled" => "crawled but not indexed",
        _ => reason,
    });
    if link_bonus > 0 {
        reasons.push("zero internal incoming links");
    }
    if gsc_bonus > 0 {
        reasons.push("had previous GSC impressions");
    }
    if age_bonus > 0 {
        reasons.push("published >30 days ago");
    }

    Some(ResubmitCandidate {
        url: drift_url.url.clone(),
        slug: drift_url.slug.clone(),
        reason_code: reason.to_string(),
        priority_score: score,
        priority_reason: reasons.join(", "),
        has_internal_links: incoming_link_count > 0,
        incoming_link_count,
        gsc_impressions: meta.and_then(|m| m.gsc_impressions),
        target_keyword: meta.and_then(|m| m.target_keyword.clone()),
        published_date: meta.and_then(|m| m.published_date.clone()),
    })
}

fn build_candidate_for_unknown(
    drift_url: &DriftUrl,
    link_scan: &LinkScanData,
    articles: &HashMap<String, ArticleMeta>,
) -> ResubmitCandidate {
    let meta = articles.get(&drift_url.slug).or_else(|| {
        let last = drift_url.slug.trim_end_matches('/').rsplit('/').next()?;
        articles.get(last)
    });

    let incoming_link_count = meta
        .and_then(|m| link_scan.incoming_by_id.get(&m.id).copied())
        .unwrap_or(0);

    let link_bonus = if incoming_link_count == 0 { 30 } else { 0 };
    let gsc_bonus = if meta.and_then(|m| m.gsc_impressions).unwrap_or(0.0) > 0.0 {
        20
    } else {
        0
    };
    let age_bonus = meta
        .and_then(|m| m.published_date.as_deref())
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|published| {
            let days = (chrono::Utc::now().date_naive() - published).num_days();
            if days > 30 { 15 } else { 0 }
        })
        .unwrap_or(0);

    let score = 80 + link_bonus + gsc_bonus + age_bonus;

    let mut reasons: Vec<&str> = Vec::new();
    reasons.push("in sitemap but never inspected by GSC");
    if link_bonus > 0 {
        reasons.push("zero internal incoming links");
    }
    if gsc_bonus > 0 {
        reasons.push("had previous GSC impressions");
    }
    if age_bonus > 0 {
        reasons.push("published >30 days ago");
    }

    ResubmitCandidate {
        url: drift_url.url.clone(),
        slug: drift_url.slug.clone(),
        reason_code: "not_in_gsc".to_string(),
        priority_score: score,
        priority_reason: reasons.join(", "),
        has_internal_links: incoming_link_count > 0,
        incoming_link_count,
        gsc_impressions: meta.and_then(|m| m.gsc_impressions),
        target_keyword: meta.and_then(|m| m.target_keyword.clone()),
        published_date: meta.and_then(|m| m.published_date.clone()),
    }
}
