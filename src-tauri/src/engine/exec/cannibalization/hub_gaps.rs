//! Hub gap detection.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Hub gaps
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect clusters that lack a hub/pillar page.
/// Uses DB-tracked page_type='hub' first, then falls back to URL prefix heuristics.
pub(crate) fn detect_hub_gaps(
    records: &[ArticleRecord],
    clusters: &[Cluster],
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
) -> Vec<serde_json::Value> {
    let mut existing_hubs: HashSet<String> = HashSet::new();

    // 1. Primary: DB-tracked hub pages (page_type = 'hub')
    if let Some(conn) = conn {
        match conn.prepare(
            "SELECT url_slug, target_keyword, title FROM articles WHERE project_id = ?1 AND page_type = 'hub'",
        ) {
            Ok(mut stmt) => {
                let rows = stmt.query_map([project_id], |row| {
                    let slug: String = row.get(0)?;
                    let kw: Option<String> = row.get(1)?;
                    let title: String = row.get(2)?;
                    Ok((slug, kw, title))
                });
                if let Ok(rows) = rows {
                    for row in rows.filter_map(|r| r.ok()) {
                        let (slug, kw, title) = row;
                        if let Some(kw) = kw.filter(|s| !s.is_empty()) {
                            existing_hubs.insert(kw.trim().to_lowercase());
                        }
                        // Derive topic from slug
                        let stripped = if slug.starts_with("hub/") {
                            &slug[4..]
                        } else if slug.starts_with("guide/") {
                            &slug[6..]
                        } else if slug.starts_with("hub_") {
                            &slug[4..]
                        } else if slug.starts_with("guide_") {
                            &slug[6..]
                        } else {
                            &slug
                        };
                        let stripped = stripped.trim().replace('_', " ").replace('-', " ").to_lowercase();
                        if !stripped.is_empty() {
                            existing_hubs.insert(stripped);
                        }
                        // Title topic
                        let title_topic = title
                            .trim()
                            .to_lowercase()
                            .trim_end_matches(": complete guide")
                            .trim_end_matches(": the complete guide")
                            .trim_end_matches(" complete guide")
                            .trim_end_matches(": ultimate guide")
                            .trim_end_matches(" ultimate guide")
                            .trim()
                            .to_string();
                        if !title_topic.is_empty() {
                            existing_hubs.insert(title_topic);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("[detect_hub_gaps] Failed to query DB hubs: {}", e);
            }
        }
    }

    // 2. From article records: explicit page_type='hub' or heuristic detection
    for r in records {
        let is_hub_explicit = r.page_type.as_deref() == Some("hub");
        let is_hub_heuristic = !is_hub_explicit
            && (r.url_slug.starts_with("hub/")
                || r.url_slug.starts_with("guide/")
                || r.url_slug.starts_with("hub_")
                || r.url_slug.starts_with("guide_")
                || r.title.to_lowercase().contains("complete guide")
                || r.title.to_lowercase().contains("ultimate guide"));

        if !is_hub_explicit && !is_hub_heuristic {
            continue;
        }

        let kw = r.target_keyword.trim().to_lowercase();
        if !kw.is_empty() {
            existing_hubs.insert(kw);
        }
        let slug = &r.url_slug;
        let stripped = if slug.starts_with("hub/") {
            &slug[4..]
        } else if slug.starts_with("guide/") {
            &slug[6..]
        } else if slug.starts_with("hub_") {
            &slug[4..]
        } else if slug.starts_with("guide_") {
            &slug[6..]
        } else {
            ""
        };
        let stripped = stripped
            .trim()
            .replace('_', " ")
            .replace('-', " ")
            .to_lowercase();
        if !stripped.is_empty() {
            existing_hubs.insert(stripped);
        }
        let title_topic = r
            .title
            .trim()
            .to_lowercase()
            .trim_end_matches(": complete guide")
            .trim_end_matches(": the complete guide")
            .trim_end_matches(" complete guide")
            .trim_end_matches(": ultimate guide")
            .trim_end_matches(" ultimate guide")
            .trim()
            .to_string();
        if !title_topic.is_empty() {
            existing_hubs.insert(title_topic);
        }
    }

    let mut gaps: Vec<serde_json::Value> = Vec::new();
    for cluster in clusters {
        if cluster.hub_exists {
            continue;
        }
        if cluster.page_ids.len() < 3 {
            continue; // Only suggest hubs for clusters with 3+ articles
        }

        let theme_kw = cluster.theme.trim().to_lowercase();
        let has_related_hub = existing_hubs
            .iter()
            .any(|hub_kw| theme_kw.contains(hub_kw) || hub_kw.contains(&theme_kw));

        if has_related_hub {
            continue;
        }

        let spoke_pages: Vec<serde_json::Value> = cluster
            .page_ids
            .iter()
            .filter_map(|&pid| records.iter().find(|r| r.id == pid))
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "url": crate::content::slug::format_blog_link(&r.url_slug),
                    "title": r.title,
                    "impressions": r.gsc["impressions"].as_f64().unwrap_or(0.0),
                })
            })
            .collect();

        gaps.push(serde_json::json!({
            "cluster_id": &cluster.cluster_id,
            "theme": &cluster.theme,
            "suggested_url": format!("/hub/{}", cluster.cluster_id.replace('_', "-")),
            "suggested_title": format!("{}: Complete Guide", capitalize_words(&cluster.theme)),
            "spoke_count": cluster.page_ids.len(),
            "total_impressions": cluster.total_impressions,
            "spoke_pages": spoke_pages,
            "reason": format!("Cluster has {} articles with {} total impressions but no broad parent hub.", cluster.page_ids.len(), cluster.total_impressions as i64),
        }));
    }

    gaps.sort_by(|a, b| {
        let ta = a["total_impressions"].as_f64().unwrap_or(0.0);
        let tb = b["total_impressions"].as_f64().unwrap_or(0.0);
        tb.partial_cmp(&ta).unwrap_or(std::cmp::Ordering::Equal)
    });

    gaps
}

pub(crate) fn capitalize_words(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
