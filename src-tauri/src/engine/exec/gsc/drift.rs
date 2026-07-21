/// Deterministic sitemap ↔ GSC drift detection.
///
/// Compares the site's sitemap against GSC indexing status and produces:
///   - URLs in sitemap but missing from GSC entirely
///   - URLs in GSC but not in sitemap
///   - URLs that are not indexed (with reason breakdown)
///   - A prioritized list of resubmission candidates
///
/// Coverage cap (issue #26): `collect_gsc` inspects at most
/// `GSC_INSPECTION_CAP` sitemap URLs per run. When the collection meta reports
/// `truncated: true`, sitemap URLs beyond the inspected set are NOT treated as
/// "GSC has never inspected it" candidates — they land in the informational
/// `coverage_capped_uninspected` bucket instead. Collection files written
/// before this change have no meta; the pre-existing behavior is preserved for
/// them (re-running `collect_gsc` writes the meta).
///
/// Reads:
///   - manifest.json (site_url + sitemap_url)
///   - sitemap.xml (live fetch)
///   - gsc_collection.json (latest URL inspection results)
///   - link_scan.json (internal link graph)
///   - articles.json (article metadata for scoring)
use std::collections::{HashMap, HashSet};

use serde::Deserialize;

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

    // 2. Fetch sitemap entries with lastmod (high limit — drift detection needs completeness)
    let sitemap_entries = crate::gsc::sitemap::fetch_sitemap_entries(&sitemap_url, 5000).await?;

    // 3. Load GSC inspection data (+ coverage meta when the data comes from
    //    a gsc_collection.json written by collect_gsc)
    let (gsc_items, collection_meta) = load_gsc_items(&paths, project_id)?;

    // 4. Load link scan data (trigger fresh scan if missing or stale >24h)
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_scan_stale = file_age_hours(&link_scan_path)
        .map(|h| h >= 24)
        .unwrap_or(true);
    let link_scan = if !link_scan_path.exists() || link_scan_stale {
        if !link_scan_path.exists() {
            log::info!("[gsc_drift] link_scan.json missing — triggering fresh scan");
        } else {
            log::info!("[gsc_drift] link_scan.json stale — triggering fresh scan");
        }
        if let Ok(articles) = load_articles_for_scan(&paths) {
            if let Ok(content_dir) =
                crate::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
            {
                match crate::content::linking::scan_links(&content_dir, &articles) {
                    Ok(result) => {
                        let json = serde_json::to_string_pretty(&result).unwrap_or_default();
                        let _ = std::fs::write(&link_scan_path, &json);
                        log::info!(
                            "[gsc_drift] fresh scan written: {} articles, {} orphans",
                            result.total_articles,
                            result.orphan_ids.len()
                        );
                    }
                    Err(e) => {
                        log::warn!("[gsc_drift] fresh scan failed: {}", e);
                    }
                }
            }
        }
        load_link_scan(&paths)
    } else {
        load_link_scan(&paths)
    };

    // 5. Load article metadata
    let articles = load_articles(&paths);

    // 5b. Compute freshness of source data files
    let gsc_collection_path = paths.automation_dir.join("gsc_collection.json");
    let gsc_data_age_hours = file_age_hours(&gsc_collection_path);
    let link_scan_age_hours = file_age_hours(&link_scan_path);

    // 6. Build normalized lookup maps
    let mut sitemap_map: HashMap<String, (String, Option<String>)> = HashMap::new(); // normalized → (original_url, lastmod)
    for entry in &sitemap_entries {
        let norm = crate::engine::exec::gsc::normalize_url_for_comparison(&entry.url);
        sitemap_map.insert(norm, (entry.url.clone(), entry.lastmod.clone()));
    }

    let mut gsc_map: HashMap<String, GscItem> = HashMap::new();
    for item in &gsc_items {
        let norm = crate::engine::exec::gsc::normalize_url_for_comparison(&item.url);
        gsc_map.insert(norm, item.clone());
    }

    // Resolve content directory for file existence checks
    let content_dir = crate::content::locator::resolve(&paths.repo_root, None).selected;

    // 7. Compute drift categories
    //
    // Coverage cap (issue #26): when the collection run truncated the sitemap,
    // only the first `cap` sitemap URLs were sent to the Inspection API. URLs
    // beyond that set are not evidence that GSC has never inspected them, so
    // they go into an informational bucket instead of the phantom
    // "never inspected" candidate list.
    let inspected_set = inspected_sitemap_set(&sitemap_entries, collection_meta.as_ref());
    let buckets = categorize_sitemap_urls(
        &sitemap_map,
        &gsc_map,
        inspected_set.as_ref(),
        content_dir.as_deref(),
    );
    let in_sitemap_not_in_gsc = buckets.in_sitemap_not_in_gsc;
    let coverage_capped_uninspected = buckets.coverage_capped_uninspected;
    let not_indexed = buckets.not_indexed;
    let indexed_count = buckets.indexed_count;

    if let Some(meta) = collection_meta.as_ref().filter(|m| m.truncated) {
        log::info!(
            "[gsc_drift] collection was coverage-capped: {} sitemap URLs, {} inspected (cap {}); {} URLs marked not-yet-inspected",
            meta.sitemap_url_count.unwrap_or(0),
            meta.inspected_count.unwrap_or(0),
            meta.cap.unwrap_or(super::GSC_INSPECTION_CAP),
            coverage_capped_uninspected.len(),
        );
    }

    let mut in_gsc_not_in_sitemap: Vec<DriftUrl> = Vec::new();
    for (norm, gsc_item) in &gsc_map {
        if !sitemap_map.contains_key(norm) {
            in_gsc_not_in_sitemap.push(DriftUrl {
                url: gsc_item.url.clone(),
                slug: crate::content::slug::extract_slug_from_url(&gsc_item.url),
                reason_code: gsc_item.reason_code.clone(),
                verdict: gsc_item.verdict.clone(),
                lastmod: None,
                has_content_file: content_file_exists(content_dir.as_deref(), &gsc_item.url),
                issues: diagnose_url(content_dir.as_deref(), &gsc_item.url),
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

    // Attach latest recovery history status to each candidate
    if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let mut stmt = db.prepare(
            "SELECT url, outcome_status FROM gsc_recovery_history WHERE project_id = ?1"
        ).ok();
        if let Some(ref mut s) = stmt {
            if let Ok(rows) = s.query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                let history: HashMap<String, String> = rows.filter_map(|r| r.ok()).collect();
                for c in &mut candidates {
                    if let Some(status) = history.get(&c.url) {
                        c.recovery_status = Some(status.clone());
                    }
                }
            }
        }
    }

    // Sort by priority score descending
    candidates.sort_by(|a, b| b.priority_score.cmp(&a.priority_score));

    let not_indexed_count = not_indexed.len();

    Ok(GscDriftReport {
        site_url,
        sitemap_url,
        checked_at: chrono::Utc::now().to_rfc3339(),
        sitemap_total: sitemap_entries.len(),
        gsc_total: gsc_items.len(),
        indexed_count,
        not_indexed_count,
        in_sitemap_not_in_gsc,
        coverage_capped_uninspected,
        in_gsc_not_in_sitemap,
        not_indexed,
        resubmit_priority: candidates,
        gsc_data_age_hours,
        link_scan_age_hours,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data loaders
// ═══════════════════════════════════════════════════════════════════════════════

fn resolve_site_config(
    project_id: &str,
    project_path: &str,
) -> Result<(String, String), crate::error::Error> {
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
                        let base = crate::models::project::site_base_url(&site_url);
                        format!("{}/sitemap.xml", base)
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
        .unwrap_or_else(|| {
            // `site_url` may be a GSC property ID (sc-domain:…) — convert for fetching.
            format!(
                "{}/sitemap.xml",
                crate::models::project::site_base_url(&site_url)
            )
        });

    Ok((site_url, sitemap_url))
}

/// Inspection-coverage metadata written by `collect_gsc` into
/// `gsc_collection.json` (issue #26).
///
/// Files written before this change have none of these fields; the serde
/// defaults then keep `truncated == false`, preserving the pre-existing drift
/// behavior (every sitemap URL without an inspection record is treated as
/// "never inspected by GSC"). Re-running `collect_gsc` writes the meta.
#[derive(Debug, Default, Clone, Deserialize)]
struct GscCollectionMeta {
    /// Total sitemap URLs discovered at collection time (before capping).
    sitemap_url_count: Option<usize>,
    /// How many URLs were actually sent to the URL Inspection API.
    inspected_count: Option<usize>,
    /// The inspection cap applied at collection time.
    cap: Option<usize>,
    /// True when the sitemap had more URLs than the cap.
    #[serde(default)]
    truncated: bool,
}

fn load_gsc_items(
    paths: &ProjectPaths,
    project_id: &str,
) -> Result<(Vec<GscItem>, Option<GscCollectionMeta>), crate::error::Error> {
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
                    let meta = doc
                        .get("meta")
                        .and_then(|m| serde_json::from_value(m.clone()).ok());
                    return Ok((gsc_items, meta));
                }
            }
        }
    }

    // Fallback: SQLite gsc_url_indexing_status table (no coverage meta available)
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open(&db_path)?;
    let rows = crate::gsc::db::list_by_project(&conn, project_id)?;
    Ok((
        rows.into_iter()
            .map(|r| GscItem {
                url: r.url,
                reason_code: r.last_reason_code,
                verdict: r.last_verdict,
            })
            .collect(),
        None,
    ))
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

/// Load articles.json as full Article structs so they can be passed to
/// `content::linking::scan_links` when we need to trigger a fresh scan.
fn load_articles_for_scan(
    paths: &ProjectPaths,
) -> Result<Vec<crate::models::article::Article>, String> {
    use crate::models::article::Article;
    let articles_path = paths.automation_dir.join("articles.json");
    let raw = std::fs::read_to_string(&articles_path).map_err(|e| e.to_string())?;
    let doc: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let arr = if doc.is_array() {
        doc.as_array().cloned().unwrap_or_default()
    } else {
        doc.get("articles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };

    let articles: Vec<Article> = arr
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    Ok(articles)
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
        doc.get("articles")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty)
    };

    let mut map: HashMap<String, ArticleMeta> = HashMap::new();
    for article in arr {
        let id = article["id"].as_i64().unwrap_or(0);
        let slug = article["url_slug"].as_str().unwrap_or("").to_string();
        if slug.is_empty() {
            continue;
        }
        let normalized_slug = crate::content::slug::normalize_url_slug(&slug);
        let gsc = &article["gsc"];
        let impressions = if gsc.is_object() {
            gsc["impressions"].as_f64()
        } else {
            None
        };

        map.insert(
            normalized_slug,
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

/// Return the age of a file in whole hours, or None if the file does not exist.
fn file_age_hours(path: &std::path::Path) -> Option<i32> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let elapsed = modified.elapsed().ok()?;
    Some((elapsed.as_secs() / 3600) as i32)
}

/// Build the set of normalized sitemap URLs that were sent to the URL
/// Inspection API during collection — but only when the collection meta
/// reports that the sitemap was truncated by the inspection cap (issue #26).
///
/// Returns `None` when no truncation was recorded (including old collection
/// files without meta), in which case every sitemap URL missing from GSC is
/// treated as genuinely never inspected (legacy behavior).
fn inspected_sitemap_set(
    sitemap_entries: &[crate::gsc::sitemap::SitemapEntry],
    meta: Option<&GscCollectionMeta>,
) -> Option<HashSet<String>> {
    let meta = meta.filter(|m| m.truncated)?;
    let cap = meta.cap.unwrap_or(super::GSC_INSPECTION_CAP);
    Some(
        sitemap_entries
            .iter()
            .take(cap)
            .map(|e| crate::engine::exec::gsc::normalize_url_for_comparison(&e.url))
            .collect(),
    )
}

/// Result of categorizing sitemap URLs against GSC inspection records.
struct DriftBuckets {
    /// In sitemap, no GSC inspection record — genuinely never inspected.
    in_sitemap_not_in_gsc: Vec<DriftUrl>,
    /// In sitemap, no GSC record, but beyond the coverage-capped inspected
    /// set. Informational only; never becomes a resubmit candidate.
    coverage_capped_uninspected: Vec<DriftUrl>,
    /// Inspected by GSC and not indexed.
    not_indexed: Vec<DriftUrl>,
    indexed_count: usize,
}

/// Categorize sitemap URLs against the GSC inspection records.
///
/// `inspected_set` is `Some` only when collection was coverage-capped; sitemap
/// URLs outside that set (and without a GSC record) are coverage-capped rather
/// than "never inspected".
fn categorize_sitemap_urls(
    sitemap_map: &HashMap<String, (String, Option<String>)>,
    gsc_map: &HashMap<String, GscItem>,
    inspected_set: Option<&HashSet<String>>,
    content_dir: Option<&std::path::Path>,
) -> DriftBuckets {
    let mut buckets = DriftBuckets {
        in_sitemap_not_in_gsc: Vec::new(),
        coverage_capped_uninspected: Vec::new(),
        not_indexed: Vec::new(),
        indexed_count: 0,
    };

    for (norm, (original_url, lastmod)) in sitemap_map {
        if let Some(gsc_item) = gsc_map.get(norm) {
            if gsc_item.reason_code.as_deref() == Some("indexed_pass") {
                buckets.indexed_count += 1;
            } else {
                buckets.not_indexed.push(DriftUrl {
                    url: original_url.clone(),
                    slug: crate::content::slug::extract_slug_from_url(original_url),
                    reason_code: gsc_item.reason_code.clone(),
                    verdict: gsc_item.verdict.clone(),
                    lastmod: lastmod.clone(),
                    has_content_file: content_file_exists(content_dir, original_url),
                    issues: diagnose_url(content_dir, original_url),
                });
            }
        } else {
            let drift_url = DriftUrl {
                url: original_url.clone(),
                slug: crate::content::slug::extract_slug_from_url(original_url),
                reason_code: None,
                verdict: None,
                lastmod: lastmod.clone(),
                has_content_file: content_file_exists(content_dir, original_url),
                issues: diagnose_url(content_dir, original_url),
            };
            match inspected_set {
                Some(set) if !set.contains(norm) => {
                    buckets.coverage_capped_uninspected.push(drift_url)
                }
                _ => buckets.in_sitemap_not_in_gsc.push(drift_url),
            }
        }
    }

    buckets
}

/// Check whether an MDX file exists for the given URL slug.
fn content_file_exists(content_dir: Option<&std::path::Path>, url: &str) -> bool {
    let dir = match content_dir {
        Some(d) => d,
        None => return false,
    };
    let slug = crate::content::slug::extract_slug_from_url(url);
    let target = slug.trim_end_matches('/');
    let files = crate::content::locator::collect_markdown_files(dir);
    files.iter().any(|p| {
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // Strip numeric prefix if present (e.g. "127_net_worth_tracker" → "net_worth_tracker")
        let bare = crate::content::ops::slug_from_filename(name);
        bare == target
    })
}

/// Diagnose frontmatter / structural issues for a URL's content file.
fn diagnose_url(content_dir: Option<&std::path::Path>, url: &str) -> Vec<String> {
    let dir = match content_dir {
        Some(d) => d,
        None => return Vec::new(),
    };
    let slug = crate::content::slug::extract_slug_from_url(url);
    let target = slug.trim_end_matches('/');
    let files = crate::content::locator::collect_markdown_files(dir);
    let path = files.iter().find(|p| {
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let bare = crate::content::ops::slug_from_filename(name);
        bare == target
    });
    let path = match path {
        Some(p) => p,
        None => return Vec::new(),
    };

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut issues = Vec::new();

    // Check for noindex
    if content.contains("noindex") || content.contains("robots: noindex") {
        issues.push("noindex".to_string());
    }

    // Check for canonical mismatch
    let canonical = crate::content::frontmatter::extract_frontmatter_string(&content, "canonical");
    if let Some(canonical_url) = canonical {
        let canonical_slug = crate::content::slug::extract_slug_from_url(&canonical_url);
        if canonical_slug != target {
            issues.push(format!("canonical mismatch: {}", canonical_slug));
        }
    }

    // Check for missing description
    if crate::content::frontmatter::extract_frontmatter_string(&content, "description").is_none() {
        issues.push("missing meta description".to_string());
    }

    // Check for thin content (< 300 words)
    let word_count = crate::content::ops::count_words(&content);
    if word_count < 300 {
        issues.push(format!("thin content ({} words)", word_count));
    }

    issues
}

fn build_candidate(
    drift_url: &DriftUrl,
    link_scan: &LinkScanData,
    articles: &HashMap<String, ArticleMeta>,
) -> Option<ResubmitCandidate> {
    let reason = drift_url.reason_code.as_deref().unwrap_or("unknown");

    // Skip technical blockers — resubmission won't help
    if matches!(
        reason,
        "robots_blocked" | "noindex" | "fetch_error" | "canonical_mismatch"
    ) {
        return None;
    }

    let meta = articles
        .get(&crate::content::slug::normalize_url_slug(&drift_url.slug))
        .or_else(|| {
            // Try matching by last segment
            let last = drift_url.slug.trim_end_matches('/').rsplit('/').next()?;
            articles.get(&crate::content::slug::normalize_url_slug(last))
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
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|published| {
            let days = (chrono::Utc::now().date_naive() - published).num_days();
            if days > 30 {
                15
            } else {
                0
            }
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
        recovery_status: None,
    })
}

fn build_candidate_for_unknown(
    drift_url: &DriftUrl,
    link_scan: &LinkScanData,
    articles: &HashMap<String, ArticleMeta>,
) -> ResubmitCandidate {
    let meta = articles
        .get(&crate::content::slug::normalize_url_slug(&drift_url.slug))
        .or_else(|| {
            let last = drift_url.slug.trim_end_matches('/').rsplit('/').next()?;
            articles.get(&crate::content::slug::normalize_url_slug(last))
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
            if days > 30 {
                15
            } else {
                0
            }
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
        recovery_status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsc::sitemap::SitemapEntry;

    fn sitemap_entries(n: usize) -> Vec<SitemapEntry> {
        (0..n)
            .map(|i| SitemapEntry {
                url: format!("https://example.com/page-{}", i),
                lastmod: None,
            })
            .collect()
    }

    fn sitemap_map(entries: &[SitemapEntry]) -> HashMap<String, (String, Option<String>)> {
        entries
            .iter()
            .map(|e| {
                (
                    crate::engine::exec::gsc::normalize_url_for_comparison(&e.url),
                    (e.url.clone(), e.lastmod.clone()),
                )
            })
            .collect()
    }

    fn gsc_map(entries: &[SitemapEntry], n: usize, reason_code: &str) -> HashMap<String, GscItem> {
        entries
            .iter()
            .take(n)
            .map(|e| {
                (
                    crate::engine::exec::gsc::normalize_url_for_comparison(&e.url),
                    GscItem {
                        url: e.url.clone(),
                        reason_code: Some(reason_code.to_string()),
                        verdict: None,
                    },
                )
            })
            .collect()
    }

    /// Issue #26: 500 sitemap URLs, 200 inspected (cap), truncated meta →
    /// the 300 uninspected URLs must NOT become phantom "never inspected"
    /// candidates; genuinely not-indexed inspected URLs still surface.
    #[test]
    fn coverage_capped_urls_are_not_phantom_never_inspected() {
        let entries = sitemap_entries(500);
        let smap = sitemap_map(&entries);

        // First 200 URLs were inspected: 199 indexed, 1 crawled-but-not-indexed.
        let mut gmap = gsc_map(&entries, 200, "indexed_pass");
        let not_indexed_norm =
            crate::engine::exec::gsc::normalize_url_for_comparison("https://example.com/page-7");
        gmap.get_mut(&not_indexed_norm).unwrap().reason_code =
            Some("not_indexed_crawled".to_string());

        let meta = GscCollectionMeta {
            sitemap_url_count: Some(500),
            inspected_count: Some(200),
            cap: Some(200),
            truncated: true,
        };

        let inspected_set = inspected_sitemap_set(&entries, Some(&meta));
        let buckets = categorize_sitemap_urls(&smap, &gmap, inspected_set.as_ref(), None);

        assert_eq!(
            buckets.in_sitemap_not_in_gsc.len(),
            0,
            "no phantom 'never inspected' URLs when collection was coverage-capped"
        );
        assert_eq!(
            buckets.coverage_capped_uninspected.len(),
            300,
            "the 300 URLs beyond the cap land in the informational bucket"
        );
        assert_eq!(buckets.indexed_count, 199);
        assert_eq!(buckets.not_indexed.len(), 1);

        // The genuinely not-indexed URL still becomes a candidate with its
        // normal score: base 40 (crawled-not-indexed) + 30 zero-internal-links.
        let candidate = build_candidate(
            &buckets.not_indexed[0],
            &LinkScanData::default(),
            &HashMap::new(),
        )
        .expect("crawled-not-indexed URL should produce a resubmit candidate");
        assert_eq!(candidate.reason_code, "not_indexed_crawled");
        assert_eq!(candidate.priority_score, 70);
    }

    /// Old collection files without coverage meta keep the legacy behavior:
    /// every sitemap URL missing from GSC is a "never inspected" candidate.
    #[test]
    fn missing_meta_preserves_legacy_behavior() {
        let entries = sitemap_entries(500);
        let smap = sitemap_map(&entries);
        let gmap = gsc_map(&entries, 200, "indexed_pass");

        assert!(
            inspected_sitemap_set(&entries, None).is_none(),
            "no meta → no inspected set"
        );

        let buckets = categorize_sitemap_urls(&smap, &gmap, None, None);
        assert_eq!(buckets.in_sitemap_not_in_gsc.len(), 300);
        assert_eq!(buckets.coverage_capped_uninspected.len(), 0);

        // Legacy phantom candidates still get the never-inspected boost.
        let candidate = build_candidate_for_unknown(
            &buckets.in_sitemap_not_in_gsc[0],
            &LinkScanData::default(),
            &HashMap::new(),
        );
        assert_eq!(candidate.reason_code, "not_in_gsc");
        assert_eq!(candidate.priority_score, 80 + 30);
    }

    /// Meta from old collection files (no coverage fields) deserializes with
    /// defaults that keep `truncated == false`.
    #[test]
    fn collection_meta_deserializes_with_backward_compatible_defaults() {
        // Old-style meta block: only the pre-#26 fields.
        let old_meta = serde_json::json!({
            "site_url": "https://example.com",
            "sitemap_url": "https://example.com/sitemap.xml",
            "collected_at": "2026-01-01T00:00:00Z",
            "total_urls": 50,
            "issues_found": 3,
        });
        let meta: GscCollectionMeta = serde_json::from_value(old_meta).unwrap();
        assert!(!meta.truncated);
        assert!(meta.sitemap_url_count.is_none());
        assert!(meta.cap.is_none());

        // New-style meta block round-trips.
        let new_meta = serde_json::json!({
            "site_url": "https://example.com",
            "sitemap_url_count": 500,
            "inspected_count": 200,
            "cap": 200,
            "truncated": true,
        });
        let meta: GscCollectionMeta = serde_json::from_value(new_meta).unwrap();
        assert!(meta.truncated);
        assert_eq!(meta.sitemap_url_count, Some(500));
        assert_eq!(meta.cap, Some(200));
    }
}
