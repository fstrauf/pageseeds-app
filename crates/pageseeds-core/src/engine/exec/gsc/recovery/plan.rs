use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Plan ─────────────────────────────────────────────────────────────────────

/// Build the structured recovery plan artifact from the drift report.
///
/// 1. Load drift report (from step output or disk)
/// 2. Filter eligible targets (reason codes, incoming link count, technical blockers)
/// 3. Score and rank
/// 4. Build source candidates per target
/// 5. Write gsc_recovery_plan.json
/// 6. Store plan artifact on task for post-action consumption
pub(crate) fn exec_gsc_recovery_plan(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // 1. Load drift report
    let drift_path = paths.automation_dir.join("gsc_recovery_drift.json");
    let report: GscDriftReport = if let Ok(raw) = std::fs::read_to_string(&drift_path) {
        match serde_json::from_str(&raw) {
            Ok(r) => r,
            Err(e) => {
                return StepResult::fail(format!("Failed to parse drift report: {}", e))
            }
        }
    } else {
        return StepResult::fail("Drift report not found — run gsc_recovery_drift first".to_string());
    };

    // 2. Load link scan for incoming link counts
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_scan: Option<serde_json::Value> = std::fs::read_to_string(&link_scan_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let incoming_counts: HashMap<i64, usize> = link_scan
        .as_ref()
        .and_then(|v| v["profiles"].as_array())
        .map(|profiles| {
            profiles
                .iter()
                .filter_map(|p| {
                    let id = p["id"].as_i64()?;
                    let incoming = p["incoming_ids"].as_array()?.len();
                    Some((id, incoming))
                })
                .collect()
        })
        .unwrap_or_default();

    // 3. Load articles for metadata
    let articles = load_articles_map(&paths);

    // 4. Load GSC collection for impressions
    let gsc_collection_path = paths.automation_dir.join("gsc_collection.json");
    let gsc_items: HashMap<String, serde_json::Value> =
        std::fs::read_to_string(&gsc_collection_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["items"].as_array().cloned())
            .map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| {
                        let url = item["url"].as_str()?;
                        let slug = extract_slug(url);
                        Some((slug, item))
                    })
                    .collect()
            })
            .unwrap_or_default();

    // 5. Technical blockers that should not create link tasks
    let technical_blockers: HashSet<&str> = [
        "robots_blocked",
        "noindex",
        "fetch_error",
        "canonical_mismatch",
        "api_error",
    ]
    .iter()
    .cloned()
    .collect();

    // 6. Eligible reason codes for link recovery
    let eligible_reasons: HashSet<&str> = [
        "not_indexed_other",
        "not_indexed_discovered",
        "not_indexed_crawled",
        "not_in_gsc",
    ]
    .iter()
    .cloned()
    .collect();

    // 7. Build targets from resubmit_priority candidates
    let mut targets: Vec<RecoveryTarget> = Vec::new();
    let mut skipped: Vec<SkippedTarget> = Vec::new();
    let mut source_usage_counts: HashMap<i64, usize> = HashMap::new();

    // Open DB for outcome learning lookups
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("[recovery_plan] cannot open DB for outcome learning: {}", e);
            None
        }
    };

    for candidate in &report.resubmit_priority {
        let reason = candidate.reason_code.as_str();

        // Skip technical blockers
        if technical_blockers.contains(reason) {
            skipped.push(SkippedTarget {
                url: candidate.url.clone(),
                reason_code: reason.to_string(),
                skip_reason: "technical blocker; internal links are not the right fix".to_string(),
            });
            continue;
        }

        // Skip if reason is not in eligible list
        if !eligible_reasons.contains(reason) {
            skipped.push(SkippedTarget {
                url: candidate.url.clone(),
                reason_code: reason.to_string(),
                skip_reason: format!("reason '{}' is not eligible for link recovery", reason),
            });
            continue;
        }

        // Outcome learning: check if this URL has been attempted before
        let history_bonus = if let Some(ref conn) = db {
            match crate::gsc::db::get_latest_recovery_outcome(
                conn,
                &task.project_id,
                &candidate.url,
            ) {
                Ok(Some(outcome)) => {
                    if outcome == "resolved" {
                        skipped.push(SkippedTarget {
                            url: candidate.url.clone(),
                            reason_code: reason.to_string(),
                            skip_reason: "previously resolved via recovery; no action needed"
                                .to_string(),
                        });
                        continue;
                    } else if outcome == "linked" || outcome == "pending" {
                        skipped.push(SkippedTarget {
                            url: candidate.url.clone(),
                            reason_code: reason.to_string(),
                            skip_reason: "recovery in progress or awaiting GSC outcome review"
                                .to_string(),
                        });
                        continue;
                    } else if outcome == "failed" {
                        log::info!(
                            "[recovery_plan] boosting priority for {} (previous attempt failed)",
                            candidate.url
                        );
                        20 // Boost score for retry
                    } else {
                        0
                    }
                }
                _ => 0,
            }
        } else {
            0
        };

        // Find article metadata by slug (matching drift.rs lookup strategy)
        let article = articles
            .get(&candidate.slug)
            .cloned()
            .or_else(|| {
                let last = candidate.slug.trim_end_matches('/').rsplit('/').next()?;
                articles.get(&crate::content::slug::normalize_url_slug(last)).cloned()
            })
            .unwrap_or_default();
        let article_id = article.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let slug = article
            .get("url_slug")
            .and_then(|v| v.as_str())
            .unwrap_or(&candidate.slug)
            .to_string();
        let slug = crate::content::slug::normalize_url_slug(&slug);
        let file = article
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let target_keyword = article
            .get("target_keyword")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let published_date = article
            .get("published_date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Check incoming link count
        if article_id == 0 {
            skipped.push(SkippedTarget {
                url: candidate.url.clone(),
                reason_code: reason.to_string(),
                skip_reason: "no matching article found in DB".to_string(),
            });
            continue;
        }

        let incoming_before = incoming_counts.get(&article_id).copied().unwrap_or(0);
        // For not_indexed_crawled URLs, having internal links is NOT a reason to skip.
        // The root cause is likely content quality / cannibalization, not link count.
        // Only skip on link count for "not_in_gsc" URLs where discovery is the issue.
        if incoming_before >= 1 && reason != "not_indexed_crawled" && reason != "not_indexed_discovered" && reason != "not_indexed_other" {
            skipped.push(SkippedTarget {
                url: candidate.url.clone(),
                reason_code: reason.to_string(),
                skip_reason: format!("already has {} incoming internal link(s)", incoming_before),
            });
            continue;
        }

        // Build source candidates
        let source_candidates = build_source_candidates(
            article_id,
            &slug,
            &target_keyword,
            &articles,
            &incoming_counts,
            &gsc_items,
            link_scan.as_ref(),
            &mut source_usage_counts,
        );

        let priority_score = candidate.priority_score as i64 + history_bonus;
        let priority_reason = if history_bonus > 0 {
            format!(
                "{} (+{} retry boost)",
                candidate.priority_reason, history_bonus
            )
        } else {
            candidate.priority_reason.clone()
        };

        targets.push(RecoveryTarget {
            url: candidate.url.clone(),
            slug,
            article_id,
            file,
            reason_code: reason.to_string(),
            priority_score,
            priority_reason,
            incoming_link_count_before: incoming_before,
            target_keyword,
            published_date,
            source_candidates,
        });
    }

    // Also include in_sitemap_not_in_gsc URLs that have zero incoming links
    for drift_url in &report.in_sitemap_not_in_gsc {
        let url = &drift_url.url;

        // Outcome learning: skip resolved/in-progress URLs
        if let Some(ref conn) = db {
            if let Ok(Some(outcome)) =
                crate::gsc::db::get_latest_recovery_outcome(conn, &task.project_id, url)
            {
                if outcome == "resolved" || outcome == "linked" || outcome == "pending" {
                    continue;
                }
            }
        }

        // Find article metadata by slug (matching drift.rs lookup strategy)
        let article = articles
            .get(&crate::content::slug::normalize_url_slug(&drift_url.slug))
            .cloned()
            .or_else(|| {
                let last = drift_url.slug.trim_end_matches('/').rsplit('/').next()?;
                articles.get(&crate::content::slug::normalize_url_slug(last)).cloned()
            })
            .unwrap_or_default();
        let article_id = article.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let slug = article
            .get("url_slug")
            .and_then(|v| v.as_str())
            .unwrap_or(&drift_url.slug)
            .to_string();
        let slug = crate::content::slug::normalize_url_slug(&slug);
        let file = article
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let target_keyword = article
            .get("target_keyword")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let published_date = article
            .get("published_date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if article_id == 0 {
            continue;
        }

        let incoming_before = incoming_counts.get(&article_id).copied().unwrap_or(0);
        if incoming_before >= 1 {
            continue;
        }

        // Skip if already in targets
        if targets.iter().any(|t| t.url == *url) {
            continue;
        }

        let source_candidates = build_source_candidates(
            article_id,
            &slug,
            &target_keyword,
            &articles,
            &incoming_counts,
            &gsc_items,
            link_scan.as_ref(),
            &mut source_usage_counts,
        );

        targets.push(RecoveryTarget {
            url: url.clone(),
            slug,
            article_id,
            file,
            reason_code: "not_in_gsc".to_string(),
            priority_score: 80, // Base score for not_in_gsc
            priority_reason: "in sitemap but never inspected by GSC, zero internal incoming links"
                .to_string(),
            incoming_link_count_before: incoming_before,
            target_keyword,
            published_date,
            source_candidates,
        });
    }

    // Sort by priority score descending
    targets.sort_by(|a, b| b.priority_score.cmp(&a.priority_score));

    // Log skip reasons so users can understand why recovery produced 0 tasks
    if !skipped.is_empty() {
        log::info!("[recovery_plan] {} candidate(s) skipped:", skipped.len());
        for s in &skipped {
            log::info!("  - {}: {}", s.url, s.skip_reason);
        }
    }
    if targets.is_empty() {
        log::warn!("[recovery_plan] 0 eligible targets after filtering — no recovery tasks will be created");
    } else {
        log::info!("[recovery_plan] {} eligible target(s) after filtering", targets.len());
        for t in &targets {
            log::info!(
                "  - {} (score={}, incoming={}, sources={})",
                t.url,
                t.priority_score,
                t.incoming_link_count_before,
                t.source_candidates.len()
            );
        }
    }

    let plan = RecoveryPlan {
        generated_at: chrono::Utc::now().to_rfc3339(),
        project_id: task.project_id.clone(),
        data_freshness: PlanFreshness {
            gsc_collected_at: gsc_collection_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
            gsc_data_age_hours: file_age_hours(&gsc_collection_path),
            link_scan_age_hours: file_age_hours(&link_scan_path),
            sitemap_fetched_at: Some(chrono::Utc::now().to_rfc3339()),
            partial_gsc_collection: false,
        },
        summary: PlanSummary {
            sitemap_total: report.sitemap_total,
            gsc_total: report.gsc_total,
            eligible_targets: targets.len(),
            skipped_targets: skipped.len(),
        },
        targets,
        skipped,
    };

    // Write plan to automation dir
    let plan_path = paths.automation_dir.join("gsc_recovery_plan.json");
    let plan_json = match serde_json::to_string_pretty(&plan) {
        Ok(j) => j,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize plan: {}", e))
        }
    };
    if let Err(e) = std::fs::write(&plan_path, &plan_json) {
        return StepResult::fail(format!("Failed to write plan: {}", e));
    }

    StepResult {
        success: true,
        message: format!(
            "Recovery plan: {} eligible targets, {} skipped. Written to {}",
            plan.summary.eligible_targets,
            plan.summary.skipped_targets,
            plan_path.display()
        ),
        output: Some(plan_json),
        artifact_key: None,
    }
}
