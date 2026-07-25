use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Extract slug from a full URL (e.g. `https://example.com/foo/bar` → `foo/bar`).
pub(crate) fn extract_slug(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .skip(1)
        .collect::<Vec<_>>()
        .join("/")
        .trim_end_matches('/')
        .to_string()
}

/// Resolve the site URL (GSC property) from manifest.json.
pub(crate) fn resolve_site_url(project_path: &str) -> String {
    let paths = ProjectPaths::from_path(project_path);
    let manifest_path = paths.automation_dir.join("manifest.json");
    if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(site_url) = manifest
                .get("gsc_site")
                .or_else(|| manifest.get("url"))
                .and_then(|v| v.as_str())
            {
                return site_url.to_string();
            }
        }
    }
    String::new()
}

pub(crate) fn file_age_hours(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() / 3600)
}

pub(crate) fn refresh_link_scan(
    paths: &ProjectPaths,
    project_id: &str,
) -> Result<String, crate::error::Error> {
    let db_path = crate::db::default_db_path();
    let db = rusqlite::Connection::open(&db_path)?;
    let articles = crate::content::article_index::list_articles(&db, project_id)?
        .into_iter()
        .filter(|a| !a.file.is_empty())
        .collect::<Vec<_>>();

    if articles.is_empty() {
        return Ok("No articles to scan".to_string());
    }

    let content_dir = crate::content::locator::resolve(&paths.repo_root, None)
        .selected
        .ok_or_else(|| {
            crate::error::Error::Other("Could not locate content directory".to_string())
        })?;

    let result = crate::content::linking::scan_links(&content_dir, &articles)?;
    let json = serde_json::to_string_pretty(&result)?;
    let scan_path = paths.automation_dir.join("link_scan.json");
    std::fs::write(&scan_path, &json)?;

    Ok(format!(
        "Link scan refreshed: {} articles, {} internal links, {} orphans, {} zero-incoming",
        result.total_articles,
        result.total_internal_links,
        result.orphan_ids.len(),
        result.zero_incoming_ids.len()
    ))
}

pub(crate) fn load_articles_map(paths: &ProjectPaths) -> HashMap<String, serde_json::Value> {
    crate::engine::exec::common::load_project_articles(paths)
        .articles
        .into_iter()
        .filter_map(|a| {
            let slug = a["url_slug"].as_str()?;
            let normalized = crate::content::slug::normalize_url_slug(slug);
            Some((normalized, a))
        })
        .collect()
}

/// Maximum times a single source page can be used across all targets in one campaign.
pub(crate) const MAX_SOURCE_USES_PER_CAMPAIGN: usize = 3;

pub(crate) fn build_source_candidates(
    target_article_id: i64,
    target_slug: &str,
    target_keyword: &str,
    articles: &HashMap<String, serde_json::Value>,
    incoming_counts: &HashMap<i64, usize>,
    gsc_items: &HashMap<String, serde_json::Value>,
    link_scan: Option<&serde_json::Value>,
    source_usage_counts: &mut HashMap<i64, usize>,
) -> Vec<SourceCandidate> {
    let mut candidates: Vec<SourceCandidate> = Vec::new();

    // Build set of source IDs that already link to the target.
    // The link scan profiles use `id` and `outgoing_ids` (Vec<i64>).
    let already_linked_ids: HashSet<i64> = link_scan
        .and_then(|v| v["profiles"].as_array())
        .map(|profiles| {
            profiles
                .iter()
                .filter_map(|p| {
                    let source_id = p["id"].as_i64()?;
                    let outgoing = p["outgoing_ids"].as_array()?;
                    let links_to_target = outgoing
                        .iter()
                        .any(|o| o.as_i64() == Some(target_article_id));
                    if links_to_target {
                        Some(source_id)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let target_text = format!("{} {}", target_keyword, target_slug.replace('-', " "));

    for (url, article) in articles {
        let source_id = article.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        if source_id == target_article_id || source_id == 0 {
            continue;
        }

        // Skip if already links to target
        if already_linked_ids.contains(&source_id) {
            continue;
        }

        // Overuse limit: skip sources that have already been used MAX times
        let current_uses = source_usage_counts.get(&source_id).copied().unwrap_or(0);
        if current_uses >= MAX_SOURCE_USES_PER_CAMPAIGN {
            continue;
        }

        let title = article
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let slug = article
            .get("url_slug")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let file = article
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let source_keyword = article
            .get("target_keyword")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // TF-IDF topical similarity: scale 0.0-1.0 to 0-30 points
        let source_text = format!("{} {} {}", title, source_keyword, slug.replace('-', " "));
        let similarity =
            crate::content::tfidf::similarity_between_texts(&target_text, &source_text);
        let topical_overlap = (similarity * 30.0).round() as i64;

        // GSC impressions bonus
        let gsc_impressions = gsc_items
            .get(url)
            .and_then(|item| item["impressions"].as_i64())
            .unwrap_or(0);
        let gsc_bonus = if gsc_impressions > 1000 {
            20
        } else if gsc_impressions > 100 {
            10
        } else {
            0
        };

        // Indexed bonus
        let indexed_bonus = gsc_items
            .get(url)
            .and_then(|item| item["reason_code"].as_str())
            .map(|r| if r == "indexed_pass" { 20 } else { 0 })
            .unwrap_or(0);

        // Hub-like bonus: source has many outgoing links
        let outgoing_count = link_scan
            .and_then(|v| v["profiles"].as_array())
            .and_then(|profiles| {
                profiles
                    .iter()
                    .find(|p| p["id"].as_i64() == Some(source_id))
                    .and_then(|p| p["outgoing_ids"].as_array().map(|o| o.len()))
            })
            .unwrap_or(0);
        let hub_bonus = if outgoing_count > 5 { 10 } else { 0 };

        let score = topical_overlap + gsc_bonus + indexed_bonus + hub_bonus;

        if score > 0 {
            candidates.push(SourceCandidate {
                article_id: source_id,
                file,
                title,
                slug,
                score,
                gsc_impressions,
                reason: format!(
                    "score={} (topical={:.0} gsc={} indexed={} hub={})",
                    score,
                    similarity * 100.0,
                    gsc_bonus,
                    indexed_bonus,
                    hub_bonus
                ),
            });
        }
    }

    // Sort by score descending, take top 10, then increment usage counts
    candidates.sort_by(|a, b| b.score.cmp(&a.score));
    candidates.truncate(10);

    for c in &candidates {
        *source_usage_counts.entry(c.article_id).or_insert(0) += 1;
    }

    candidates
}
