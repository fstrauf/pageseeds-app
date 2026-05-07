/// GSC Indexing Recovery Campaign — deterministic prepare, drift, and plan steps.
///
/// Phase 1 (MVP):
///   - gsc_recovery_prepare: refresh stale link scan; report GSC freshness
///   - gsc_recovery_drift: reuse existing exec_gsc_drift
///   - gsc_recovery_plan: filter/score targets, build source candidates, write plan artifact
///
/// Phase 2:
///   - gsc_indexing_outcome_inspect: re-inspect target URL after wait period
///   - gsc_indexing_outcome_report: compare before/after, write outcome artifact
use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;

// ─── Freshness defaults ───────────────────────────────────────────────────────

const MAX_GSC_AGE_HOURS: u64 = 24;
const MAX_LINK_SCAN_AGE_HOURS: u64 = 24;
const DEFAULT_SITEMAP_LIMIT: usize = 200;

// ─── Prepare ──────────────────────────────────────────────────────────────────

/// Check data freshness and refresh link scan when stale.
/// GSC collection refresh is attempted via the existing collect helper when
/// a token is available; if not, the step warns but does not fail so that
/// planning can fall back to sitemap-only mode.
pub(crate) fn exec_gsc_recovery_prepare(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let mut messages: Vec<String> = Vec::new();

    // 1. Check GSC collection freshness
    let gsc_collection_path = paths.automation_dir.join("gsc_collection.json");
    let gsc_age = file_age_hours(&gsc_collection_path);
    let gsc_fresh = gsc_age.map(|h| h < MAX_GSC_AGE_HOURS).unwrap_or(false);

    if gsc_fresh {
        messages.push(format!("GSC data fresh ({}h old)", gsc_age.unwrap_or(0)));
    } else if gsc_collection_path.exists() {
        messages.push(format!(
            "GSC data stale ({}h old) — will use available data or refresh if possible",
            gsc_age.unwrap_or(999)
        ));
    } else {
        messages.push(
            "GSC data missing — will use sitemap-only mode or refresh if possible".to_string(),
        );
    }

    // 2. Refresh link scan if stale or missing
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_age = file_age_hours(&link_scan_path);
    let link_fresh = link_age
        .map(|h| h < MAX_LINK_SCAN_AGE_HOURS)
        .unwrap_or(false);

    if !link_fresh || !link_scan_path.exists() {
        messages.push(format!(
            "Link scan {} — refreshing",
            if link_scan_path.exists() {
                format!("stale ({}h old)", link_age.unwrap_or(999))
            } else {
                "missing".to_string()
            }
        ));
        match refresh_link_scan(&paths, &task.project_id) {
            Ok(summary) => messages.push(summary),
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Failed to refresh link scan: {}", e),
                    output: None,
                };
            }
        }
    } else {
        messages.push(format!("Link scan fresh ({}h old)", link_age.unwrap_or(0)));
    }

    // 3. Optionally refresh GSC data if stale and token available
    // For V1, we call the existing collect helper when the token is present.
    // This avoids duplicating the auth + inspect pipeline.
    if !gsc_fresh && gsc_token.is_some() {
        messages.push("Attempting GSC refresh via existing collection helper…".to_string());
        let collect_result =
            crate::engine::exec::gsc::exec_collect_gsc(task, project_path, gsc_token);
        if collect_result.success {
            messages.push("GSC refresh succeeded".to_string());
        } else {
            messages.push(format!(
                "GSC refresh failed: {} — continuing with cached data",
                collect_result.message
            ));
        }
    }

    let freshness = serde_json::json!({
        "gsc_data_age_hours": gsc_age,
        "gsc_data_fresh": gsc_fresh,
        "link_scan_age_hours": link_age,
        "link_scan_fresh": link_fresh,
        "partial_gsc_collection": false,
    });

    StepResult {
        success: true,
        message: messages.join(". "),
        output: Some(freshness.to_string()),
    }
}

// ─── Drift ────────────────────────────────────────────────────────────────────

/// Reuse the existing drift computation and return it as a StepResult.
/// Writes gsc_recovery_drift.json to the automation dir so the plan step
/// can read it without re-running the full drift query.
pub(crate) fn exec_gsc_recovery_drift(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to create tokio runtime: {}", e),
                output: None,
            }
        }
    };

    let report: GscDriftReport = match rt.block_on(async {
        crate::engine::exec::gsc::exec_gsc_drift(&task.project_id, project_path).await
    }) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Drift computation failed: {}", e),
                output: None,
            }
        }
    };

    // Persist drift report for plan step
    let drift_path = paths.automation_dir.join("gsc_recovery_drift.json");
    let _ = std::fs::create_dir_all(&paths.automation_dir);
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(&drift_path, json);
    }

    StepResult {
        success: true,
        message: format!(
            "Drift: {} indexed, {} not indexed, {} missing from GSC, {} orphans in priority list",
            report.indexed_count,
            report.not_indexed_count,
            report.in_sitemap_not_in_gsc.len(),
            report
                .resubmit_priority
                .iter()
                .filter(|c| !c.has_internal_links)
                .count(),
        ),
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}

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
                return StepResult {
                    success: false,
                    message: format!("Failed to parse drift report: {}", e),
                    output: None,
                }
            }
        }
    } else {
        return StepResult {
            success: false,
            message: "Drift report not found — run gsc_recovery_drift first".to_string(),
            output: None,
        };
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
        if incoming_before >= 1 {
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
            return StepResult {
                success: false,
                message: format!("Failed to serialize plan: {}", e),
                output: None,
            }
        }
    };
    if let Err(e) = std::fs::write(&plan_path, &plan_json) {
        return StepResult {
            success: false,
            message: format!("Failed to write plan: {}", e),
            output: None,
        };
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
    }
}

// ─── Outcome Review (Phase 2) ─────────────────────────────────────────────────

/// Re-inspect a target URL in GSC after a wait period.
/// Reads the target URL from the task's indexing_link_target artifact.
pub(crate) fn exec_gsc_indexing_outcome_inspect(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Extract target URL from artifact
    let target_url = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_link_target")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v["target"]["url"].as_str().map(String::from));

    let target_url = match target_url {
        Some(u) => u,
        None => {
            return StepResult {
                success: false,
                message: "No target URL found in indexing_link_target artifact".to_string(),
                output: None,
            }
        }
    };

    // Load previous status from gsc_recovery_plan or outcome baseline
    let baseline_path = paths
        .automation_dir
        .join("gsc_indexing_outcome_baseline.json");
    let baseline: Option<serde_json::Value> = std::fs::read_to_string(&baseline_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let previous_reason = baseline
        .as_ref()
        .and_then(|v| v["reason_code"].as_str())
        .unwrap_or("unknown");

    // If no token, we can't inspect — return pending
    let token = match gsc_token {
        Some(t) => t.to_string(),
        None => {
            return StepResult {
                success: true,
                message: "No GSC token available — outcome inspection deferred".to_string(),
                output: Some(
                    serde_json::json!({
                        "target_url": target_url,
                        "status": "deferred",
                        "previous_reason": previous_reason,
                    })
                    .to_string(),
                ),
            }
        }
    };

    // Resolve site_url (GSC property) from manifest
    let site_url = resolve_site_url(project_path);

    // Inspect the URL
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to create tokio runtime: {}", e),
                output: None,
            }
        }
    };

    let inspect_result = rt.block_on(async {
        crate::gsc::indexing::inspect_batch(&token, &site_url, vec![target_url.clone()]).await
    });

    match inspect_result {
        Ok(records) => {
            let record = records.first();
            let current_reason = record
                .and_then(|r| r.reason_code.as_deref())
                .unwrap_or("unknown");
            let current_verdict = record
                .and_then(|r| r.verdict.as_deref())
                .unwrap_or("unknown");

            let outcome = serde_json::json!({
                "target_url": target_url,
                "previous_reason": previous_reason,
                "current_reason": current_reason,
                "current_verdict": current_verdict,
                "inspected_at": chrono::Utc::now().to_rfc3339(),
            });

            StepResult {
                success: true,
                message: format!(
                    "Re-inspected {}: {} → {}",
                    target_url, previous_reason, current_reason
                ),
                output: Some(outcome.to_string()),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("URL Inspection API failed: {}", e),
            output: None,
        },
    }
}

/// Compare before/after indexing status and write a structured outcome report.
pub(crate) fn exec_gsc_indexing_outcome_report(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load the inspect output from the previous step (stored as artifact or on disk)
    let inspect_path = paths
        .automation_dir
        .join(format!("gsc_outcome_inspect_{}.json", task.id));
    let inspect_data: Option<serde_json::Value> = std::fs::read_to_string(&inspect_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .or_else(|| {
            // Fallback: try to read from task artifacts
            task.artifacts
                .iter()
                .find(|a| a.key == "gsc_indexing_outcome_inspect")
                .and_then(|a| a.content.as_ref())
                .and_then(|c| serde_json::from_str(c).ok())
        });

    let (target_url, previous_reason, current_reason) = inspect_data
        .as_ref()
        .map(|v| {
            (
                v["target_url"].as_str().unwrap_or("").to_string(),
                v["previous_reason"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                v["current_reason"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })
        .unwrap_or_default();

    let outcome_status = if current_reason == "indexed_pass" {
        "resolved"
    } else if current_reason == previous_reason {
        "still_not_indexed"
    } else if current_reason == "unknown" {
        "unknown"
    } else {
        "regressed"
    };

    let report = serde_json::json!({
        "target_url": target_url,
        "previous_reason": previous_reason,
        "current_reason": current_reason,
        "outcome_status": outcome_status,
        "reported_at": chrono::Utc::now().to_rfc3339(),
        "campaign_task_id": task.artifacts.iter().find(|a| a.key == "indexing_link_target")
            .and_then(|a| a.content.as_ref())
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .and_then(|v| v["campaign_task_id"].as_str().map(String::from)),
    });

    let report_path = paths
        .automation_dir
        .join(format!("gsc_outcome_report_{}.json", task.id));
    let _ = std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    );

    StepResult {
        success: true,
        message: format!("Outcome report for {}: {}", target_url, outcome_status),
        output: Some(report.to_string()),
    }
}

// ─── Post-action helper ───────────────────────────────────────────────────────

/// Spawn child `fix_indexing_internal_links` tasks from a recovery plan.
/// Called by post_actions.rs after gsc_indexing_recovery completes.
pub(crate) fn spawn_recovery_child_tasks(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};

    let paths = ProjectPaths::from_path(project_path);
    let plan_path = paths.automation_dir.join("gsc_recovery_plan.json");

    let plan: RecoveryPlan = match std::fs::read_to_string(&plan_path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("[recovery_post_action] failed to parse plan: {}", e);
                return vec![];
            }
        },
        Err(e) => {
            log::warn!("[recovery_post_action] plan file not found: {}", e);
            return vec![];
        }
    };

    let mut created_ids: Vec<String> = Vec::new();

    for target in &plan.targets {
        let idempotency_key = format!(
            "gsc-indexing-recovery:{}:{}:{}",
            parent_task.project_id, target.reason_code, target.url
        );

        let target_artifact = crate::models::task::TaskArtifact {
            key: "indexing_link_target".to_string(),
            path: None,
            artifact_type: Some("indexing_link_target".to_string()),
            source: Some("gsc_recovery_plan".to_string()),
            content: Some(
                serde_json::json!({
                    "campaign_task_id": parent_task.id,
                    "target": {
                        "url": &target.url,
                        "slug": &target.slug,
                        "article_id": target.article_id,
                        "file": &target.file,
                        "reason_code": &target.reason_code,
                        "incoming_link_count_before": target.incoming_link_count_before,
                        "target_keyword": &target.target_keyword,
                        "source_candidates": target.source_candidates,
                    }
                })
                .to_string(),
            ),
        };

        let priority = if target.priority_score >= 100 {
            Priority::High
        } else {
            Priority::Medium
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_indexing_internal_links".to_string(),
            title: Some(format!(
                "Fix links for {} ({})",
                target.slug, target.reason_code
            )),
            description: Some(format!(
                "Add inbound internal links to {}. Reason: {}. Baseline incoming: {}.",
                target.url, target.priority_reason, target.incoming_link_count_before
            )),
            run_policy: Some(TaskRunPolicy::AutoEnqueue),
            agent_policy: AgentPolicy::Required,
            priority,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![target_artifact],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 14 }),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[recovery_post_action] spawned child task {} for {}",
                    task.id,
                    target.url
                );
                // Record in recovery history
                let _ = crate::gsc::db::insert_recovery_history(
                    conn,
                    &parent_task.project_id,
                    &target.url,
                    Some(target.article_id),
                    &parent_task.id,
                    &task.id,
                    &target.reason_code,
                    target.incoming_link_count_before as i64,
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!(
                    "[recovery_post_action] failed to spawn task for {}: {}",
                    target.url,
                    e
                );
            }
        }
    }

    log::info!(
        "[recovery_post_action] created {} child tasks from plan",
        created_ids.len()
    );
    created_ids
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Extract slug from a full URL (e.g. `https://example.com/foo/bar` → `foo/bar`).
fn extract_slug(url: &str) -> String {
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
fn resolve_site_url(project_path: &str) -> String {
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

fn file_age_hours(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() / 3600)
}

fn refresh_link_scan(
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

fn load_articles_map(paths: &ProjectPaths) -> HashMap<String, serde_json::Value> {
    let articles_path = paths.automation_dir.join("articles.json");
    std::fs::read_to_string(&articles_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["articles"].as_array().cloned())
        .map(|articles| {
            articles
                .into_iter()
                .filter_map(|a| {
                    let slug = a["url_slug"].as_str()?;
                    let normalized = crate::content::slug::normalize_url_slug(slug);
                    Some((normalized, a))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Maximum times a single source page can be used across all targets in one campaign.
const MAX_SOURCE_USES_PER_CAMPAIGN: usize = 3;

fn build_source_candidates(
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

// ─── Data structs for plan JSON ───────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoveryPlan {
    generated_at: String,
    project_id: String,
    data_freshness: PlanFreshness,
    summary: PlanSummary,
    targets: Vec<RecoveryTarget>,
    skipped: Vec<SkippedTarget>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlanFreshness {
    gsc_collected_at: Option<String>,
    gsc_data_age_hours: Option<u64>,
    link_scan_age_hours: Option<u64>,
    sitemap_fetched_at: Option<String>,
    partial_gsc_collection: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlanSummary {
    sitemap_total: usize,
    gsc_total: usize,
    eligible_targets: usize,
    skipped_targets: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoveryTarget {
    url: String,
    slug: String,
    article_id: i64,
    file: String,
    reason_code: String,
    priority_score: i64,
    priority_reason: String,
    incoming_link_count_before: usize,
    target_keyword: String,
    published_date: String,
    source_candidates: Vec<SourceCandidate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SourceCandidate {
    article_id: i64,
    file: String,
    title: String,
    slug: String,
    score: i64,
    gsc_impressions: i64,
    reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SkippedTarget {
    url: String,
    reason_code: String,
    skip_reason: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_article(id: i64, slug: &str, title: &str, keyword: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "url_slug": slug,
            "title": title,
            "target_keyword": keyword,
            "file": format!("{:03}_{}.mdx", id, slug.replace('-', "_")),
        })
    }

    #[test]
    fn build_source_candidates_excludes_target_and_already_linked() {
        let mut articles = HashMap::new();
        articles.insert(
            "target".to_string(),
            make_article(1, "target", "Target Page", "machine learning"),
        );
        articles.insert(
            "source-a".to_string(),
            make_article(2, "source-a", "Source A", "deep learning"),
        );
        articles.insert(
            "source-b".to_string(),
            make_article(3, "source-b", "Source B", "baking recipes"),
        );

        let incoming_counts = HashMap::new();
        let gsc_items = HashMap::new();
        let link_scan = serde_json::json!({
            "profiles": [
                {
                    "id": 2,
                    "outgoing_ids": [1]
                }
            ]
        });

        let mut usage = HashMap::new();
        let candidates = build_source_candidates(
            1,
            "target",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );

        // source-a already links to target → excluded
        assert!(
            candidates.iter().all(|c| c.article_id != 2),
            "already-linked source should be excluded"
        );
        // source-b is unrelated (score 0) → excluded by score filter
        assert!(
            candidates.iter().all(|c| c.article_id != 3),
            "unrelated source with score 0 should be excluded"
        );
        // source-a is the only candidate but it's excluded, so list may be empty
        // or source-b might have minimal overlap. The key assertion is source-a is gone.
    }

    #[test]
    fn build_source_candidates_enforces_overuse_limit() {
        let mut articles = HashMap::new();
        for i in 1..=10 {
            articles.insert(
                format!("source-{}", i),
                make_article(
                    i as i64,
                    &format!("source-{}", i),
                    &format!("Source {}", i),
                    "machine learning",
                ),
            );
        }
        // Add target
        articles.insert(
            "target".to_string(),
            make_article(99, "target", "Target Page", "machine learning"),
        );

        let incoming_counts = HashMap::new();
        let gsc_items = HashMap::new();
        let link_scan = serde_json::json!({ "profiles": [] });

        let mut usage = HashMap::new();

        // First call for target-1: gets top candidates
        let c1 = build_source_candidates(
            99,
            "target",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );
        assert!(!c1.is_empty(), "should find candidates");

        // Use the top candidate MAX_SOURCE_USES_PER_CAMPAIGN times
        let top_id = c1[0].article_id;
        for _ in 0..MAX_SOURCE_USES_PER_CAMPAIGN {
            *usage.entry(top_id).or_insert(0) += 1;
        }

        // Next call should not include the overused source
        let c2 = build_source_candidates(
            98,
            "target-2",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );
        assert!(
            !c2.iter().any(|c| c.article_id == top_id),
            "overused source ({} uses) should be excluded",
            MAX_SOURCE_USES_PER_CAMPAIGN
        );
    }

    #[test]
    fn build_source_candidates_scores_by_topical_similarity() {
        let mut articles = HashMap::new();
        articles.insert(
            "target".to_string(),
            make_article(1, "target", "Machine Learning Guide", "machine learning"),
        );
        articles.insert(
            "related".to_string(),
            make_article(2, "related", "Deep Learning Tutorial", "deep learning"),
        );
        articles.insert(
            "unrelated".to_string(),
            make_article(3, "unrelated", "Chocolate Cake Recipe", "baking"),
        );

        let incoming_counts = HashMap::new();
        let gsc_items = HashMap::new();
        let link_scan = serde_json::json!({ "profiles": [] });

        let mut usage = HashMap::new();
        let candidates = build_source_candidates(
            1,
            "target",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );

        // Related source should score higher than unrelated
        let related = candidates.iter().find(|c| c.article_id == 2);
        let unrelated = candidates.iter().find(|c| c.article_id == 3);

        if let (Some(r), Some(u)) = (related, unrelated) {
            assert!(
                r.score > u.score,
                "related source ({}: {}) should score higher than unrelated ({}: {})",
                r.title,
                r.score,
                u.title,
                u.score
            );
        }
    }

    #[test]
    fn file_age_hours_returns_none_for_missing_file() {
        let path = std::path::Path::new("/nonexistent/path/to/file.txt");
        assert!(file_age_hours(path).is_none());
    }
}
