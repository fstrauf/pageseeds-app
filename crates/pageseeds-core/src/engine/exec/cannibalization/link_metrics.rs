//! Internal link graph scanner.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Link graph
// ═══════════════════════════════════════════════════════════════════════════════

/// Enrich article records with incoming/outgoing internal link counts.
pub(crate) fn enrich_link_metrics(records: &mut [ArticleRecord], project_path: &str) {
    let repo_root = Path::new(project_path);
    let content_resolution = crate::content::locator::resolve(repo_root, None);
    let Some(content_dir) = content_resolution.selected else {
        log::warn!("[cannibalization_audit] Could not find content directory for link scan");
        return;
    };

    // Build minimal Article structs for scan_links
    let articles: Vec<crate::models::article::Article> = records
        .iter()
        .map(|r| crate::models::article::Article {
            id: r.id,
            title: r.title.clone(),
            url_slug: r.url_slug.clone(),
            file: r.file.clone(),
            target_keyword: Some(r.target_keyword.clone()),
            keyword_difficulty: None,
            target_volume: 0,
            published_date: Some(r.published_date.clone()),
            word_count: r.word_count as i64,
            status: "published".to_string(),
            review_status: None,
            review_started_at: None,
            last_reviewed_at: None,
            review_count: 0,
            content_gaps_addressed: vec![],
            estimated_traffic_monthly: None,
            page_type: None,
            project_id: String::new(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
            content_hash: None,
            last_edited_at: None,
        })
        .collect();

    match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(result) => {
            for profile in &result.profiles {
                if let Some(record) = records.iter_mut().find(|r| r.id == profile.id) {
                    record.incoming_links = profile.incoming_ids.len();
                    record.outgoing_links = profile.outgoing_ids.len();
                }
            }
        }
        Err(e) => {
            log::warn!("[cannibalization_audit] Link scan failed: {}", e);
        }
    }
}
