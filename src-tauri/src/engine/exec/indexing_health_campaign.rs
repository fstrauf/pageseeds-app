/// Unified indexing health campaign execution module.
///
/// Orchestrates prerequisite checks, drift analysis, cluster context building,
/// agentic distinctiveness review, and campaign plan reduction.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::indexing_health::{
    DistinctivenessVerdict, IndexingCampaignPlan, IndexingCampaignSummary, IndexingTargetContext,
    IndexingTargetPlan, PrerequisiteCheck, PrerequisiteReport, TargetDiagnosis,
};
use crate::models::task::Task;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Check Prerequisites
// ═══════════════════════════════════════════════════════════════════════════════

/// Check freshness of prerequisite artifacts.
/// If any auto-runnable prerequisite is stale, spawns the helper task
/// and returns failure so the parent task pauses until prerequisites
/// are satisfied. Re-run the parent after helpers complete.
pub(crate) fn exec_ihc_check_prerequisites(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let checks = vec![
        check_artifact(&paths, "gsc_collection.json", chrono::Duration::days(7)),
        check_artifact(&paths, "link_scan.json", chrono::Duration::days(7)),
        check_artifact(&paths, "content_audit.json", chrono::Duration::days(14)),
    ];

    // Clusters are a nice-to-have, not a blocker. The campaign runs fine without them.
    let cluster_check = check_artifact(&paths, "cannibalization_clusters.json", chrono::Duration::days(30));

    let all_fresh = checks.iter().all(|c| c.fresh);
    let report = PrerequisiteReport {
        all_fresh,
        checks: {
            let mut c = checks.clone();
            c.push(cluster_check.clone());
            c
        },
    };

    let stale_auto: Vec<&PrerequisiteCheck> = checks
        .iter()
        .filter(|c| !c.fresh && c.action.as_deref().unwrap_or("").starts_with("auto_enqueue"))
        .collect();

    let stale_user: Vec<&PrerequisiteCheck> = checks
        .iter()
        .filter(|c| {
            !c.fresh
                && c.action
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("user_must_run")
        })
        .collect();

    // Spawn cluster refresh as a best-effort helper (don't block campaign on it)
    let mut cluster_helper: Option<(String, String, String)> = None;
    if !cluster_check.fresh {
        if let Ok(conn) = rusqlite::Connection::open(crate::db::default_db_path()) {
            let spec = crate::engine::spawner::TaskSpec {
                project_id: task.project_id.clone(),
                task_type: "cannibalization_audit".to_string(),
                title: Some("cannibalization audit (auto-refresh)".to_string()),
                description: Some("Auto-spawned by indexing_health_campaign because cannibalization_clusters.json was stale.".to_string()),
                run_policy: Some(crate::models::task::TaskRunPolicy::AutoEnqueue),
                priority: crate::models::task::Priority::High,
                agent_policy: crate::models::task::AgentPolicy::None,
                idempotency_key: Some(format!("auto-refresh:cannibalization_clusters:{}", task.project_id)),
                dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::SkipIfActive),
                depends_on: vec![],
                artifacts: vec![],
                ..Default::default()
            };
            match crate::engine::spawner::TaskSpawner::spawn(&conn, spec) {
                Ok(spawned) => {
                    let ty = spawned.task_type.clone();
                    let item = crate::models::queue::EnqueueItem {
                        task_id: spawned.id.clone(),
                        project_id: spawned.project_id,
                        title: spawned.title.clone(),
                        task_type: Some(ty),
                        project_name: None,
                    };
                    if let Err(e) = crate::engine::queue::enqueue_tasks(&conn, vec![item], crate::models::queue::EnqueueMode::Append) {
                        log::warn!("[ihc] failed to enqueue cluster helper: {}", e);
                    }
                    cluster_helper = Some((spawned.id, spawned.task_type, spawned.status.to_string()));
                }
                Err(e) => log::warn!("[ihc] failed to spawn cluster helper: {}", e),
            }
        }
    }

    // Spawn auto-runnable helper tasks in the background and enqueue them
    let mut helpers: Vec<(String, String, String)> = vec![]; // (id, task_type, status)
    if !stale_auto.is_empty() {
        let db_path = crate::db::default_db_path();
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            for check in &stale_auto {
                let task_type = artifact_to_task_type(&check.artifact);
                let idempotency_key = format!(
                    "auto-refresh:{}:{}",
                    task_type, task.project_id
                );
                let spec = crate::engine::spawner::TaskSpec {
                    project_id: task.project_id.clone(),
                    task_type: task_type.to_string(),
                    title: Some(format!("{} (auto-refresh)", task_type.replace('_', " "))),
                    description: Some(format!(
                        "Auto-spawned by indexing_health_campaign because {} was stale.",
                        check.artifact
                    )),
                    run_policy: Some(crate::models::task::TaskRunPolicy::AutoEnqueue),
                    priority: crate::models::task::Priority::High,
                    agent_policy: crate::models::task::AgentPolicy::None,
                    idempotency_key: Some(idempotency_key),
                    dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::SkipIfActive),
                    depends_on: vec![],
                    artifacts: vec![],
                    ..Default::default()
                };
                match crate::engine::spawner::TaskSpawner::spawn(&conn, spec) {
                    Ok(spawned) => {
                        helpers.push((spawned.id.clone(), spawned.task_type.clone(), spawned.status.to_string()));
                        // Also enqueue to the active queue so it actually runs
                        let item = crate::models::queue::EnqueueItem {
                            task_id: spawned.id,
                            project_id: spawned.project_id,
                            title: spawned.title.clone(),
                            task_type: Some(spawned.task_type),
                            project_name: None,
                        };
                        if let Err(e) = crate::engine::queue::enqueue_tasks(&conn, vec![item], crate::models::queue::EnqueueMode::Append) {
                            log::warn!("[ihc] failed to enqueue helper {}: {}", task_type, e);
                        }
                    }
                    Err(e) => log::warn!("[ihc] failed to spawn {}: {}", task_type, e),
                }
            }
        }
    }

    let output = match serde_json::to_string_pretty(&report) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize prerequisite report: {}", e),
                output: None,
            }
        }
    };

    // Write report to disk for downstream steps
    let report_path = paths.automation_dir.join("indexing_prerequisites.json");
    let _ = std::fs::create_dir_all(&paths.automation_dir);
    let _ = std::fs::write(&report_path, &output);

    if !stale_user.is_empty() {
        let names: Vec<String> = stale_user.iter().map(|c| c.artifact.clone()).collect();
        return StepResult {
            success: false,
            message: format!(
                "User action required before campaign can run: {}",
                names.join(", ")
            ),
            output: Some(output),
        };
    }

    if !helpers.is_empty() {
        let helper_lines: Vec<String> = helpers
            .iter()
            .map(|(id, ty, status)| format!("  • {} ({}) — status: {}", id, ty, status))
            .collect();
        return StepResult {
            success: false,
            message: format!(
                "Waiting for {} helper task(s) to complete before campaign can run:\n{}",
                helpers.len(),
                helper_lines.join("\n")
            ),
            output: Some(output),
        };
    }

    let msg = match cluster_helper {
        Some((id, ty, status)) => format!(
            "All required prerequisites are fresh. Cluster data refresh running in background: {} ({}) — status: {}",
            id, ty, status
        ),
        None => "All prerequisite artifacts are fresh.".to_string(),
    };
    StepResult {
        success: true,
        message: msg,
        output: Some(output),
    }
}

fn artifact_to_task_type(artifact: &str) -> &str {
    match artifact {
        "gsc_collection.json" => "collect_gsc",
        "link_scan.json" => "cluster_and_link",
        "content_audit.json" => "content_audit",
        _ => artifact.trim_end_matches(".json"),
    }
}

fn check_artifact(
    paths: &ProjectPaths,
    filename: &str,
    max_age: chrono::Duration,
) -> PrerequisiteCheck {
    let path = paths.automation_dir.join(filename);
    let (fresh, age_hours) = if path.exists() {
        match std::fs::metadata(&path) {
            Ok(meta) => match meta.modified() {
                Ok(modified) => match modified.elapsed() {
                    Ok(elapsed) => {
                        let hours = elapsed.as_secs() / 3600;
                        (hours < max_age.num_seconds() as u64, Some(hours as i64))
                    }
                    Err(_) => (false, None),
                },
                Err(_) => (false, None),
            },
            Err(_) => (false, None),
        }
    } else {
        (false, None)
    };

    let action = if fresh {
        None
    } else {
        match filename {
            "cannibalization_strategy.json" => {
                Some("auto_enqueue_cannibalization_audit".to_string())
            }
            _ => Some(format!("auto_enqueue_{}", filename.trim_end_matches(".json"))),
        }
    };

    PrerequisiteCheck {
        artifact: filename.to_string(),
        fresh,
        age_hours,
        action,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Build Target Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Load drift report + cannibalization clusters + content audit,
/// and build per-target context objects for each not-indexed URL.
pub(crate) fn exec_ihc_build_target_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // 1. Load drift report
    let drift_path = paths.automation_dir.join("gsc_recovery_drift.json");
    let drift_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &drift_path,
        "gsc_recovery_drift.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let not_indexed = drift_doc["not_indexed"].as_array().cloned().unwrap_or_default();
    if not_indexed.is_empty() {
        return StepResult {
            success: true,
            message: "No not-indexed URLs found in drift report.".to_string(),
            output: Some("{\"targets\": []}".to_string()),
        };
    }

    // 2. Load cannibalization clusters (DB primary, JSON fallback)
    let clusters_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_latest_audit_artifact(&conn, &task.project_id, "cannibalization_clusters").ok().flatten()
            });
        db_doc.unwrap_or_else(|| {
            let clusters_path = paths.automation_dir.join("cannibalization_clusters.json");
            std::fs::read_to_string(&clusters_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({ "clusters": [] }))
        })
    };
    let clusters = clusters_doc["clusters"].as_array().cloned().unwrap_or_default();

    // Build a map from article URL (normalized) → cluster
    let mut url_to_cluster: HashMap<String, &serde_json::Value> = HashMap::new();
    for cluster in &clusters {
        if let Some(pages) = cluster["pages"].as_array() {
            for page in pages {
                if let Some(url) = page["url"].as_str() {
                    let norm = normalize_url(url);
                    url_to_cluster.insert(norm, cluster);
                }
            }
        }
    }

    // 3. Load content audit (DB primary, JSON fallback)
    let audit_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_audit_report_as_json(&conn, &task.project_id).ok().flatten()
            });
        db_doc.unwrap_or_else(|| {
            let audit_path = paths.automation_dir.join("content_audit.json");
            std::fs::read_to_string(&audit_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({ "articles": [] }))
        })
    };
    let audit_articles = audit_doc["articles"].as_array().cloned().unwrap_or_default();
    let mut audit_by_slug: HashMap<String, &serde_json::Value> = HashMap::new();
    for article in &audit_articles {
        if let Some(slug) = article["url_slug"].as_str() {
            audit_by_slug.insert(slug.to_string(), article);
        }
    }

    // 3b. Load articles.json for article_id / file lookups
    let articles_path = paths.automation_dir.join("articles.json");
    let articles_doc: serde_json::Value = std::fs::read_to_string(&articles_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "articles": [] }));
    let all_articles = articles_doc["articles"].as_array().cloned().unwrap_or_default();
    let mut article_by_slug: HashMap<String, &serde_json::Value> = HashMap::new();
    for article in &all_articles {
        if let Some(slug) = article["url_slug"].as_str() {
            article_by_slug.insert(slug.to_string(), article);
        }
    }

    // 3c. Load link_scan.json for outgoing-link checks when building source candidates
    let link_scan_path = paths.automation_dir.join("link_scan.json");
    let link_scan: serde_json::Value = std::fs::read_to_string(&link_scan_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "profiles": [] }));
    let profiles = link_scan["profiles"].as_array().cloned().unwrap_or_default();
    let outgoing_by_id: HashMap<i64, HashSet<i64>> = profiles
        .iter()
        .filter_map(|p| {
            let id = p["id"].as_i64()?;
            let outgoing: HashSet<i64> = p["outgoing_ids"]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_i64())
                .collect();
            Some((id, outgoing))
        })
        .collect();

    // 4. Build target contexts
    let mut targets: Vec<IndexingTargetContext> = Vec::new();

    for item in &not_indexed {
        let url = item["url"].as_str().unwrap_or("").to_string();
        let slug = item["slug"].as_str().unwrap_or("").to_string();
        let reason_code = item["reason_code"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        if url.is_empty() {
            continue;
        }

        // Find cluster match
        let cluster = url_to_cluster.get(&normalize_url(&url)).cloned();

        let (sibling_count, siblings, shared_headings) = cluster
            .map(|c| {
                let pages = c["pages"].as_array().cloned().unwrap_or_default();
                let sibs: Vec<crate::models::indexing_health::SiblingArticle> = pages
                    .iter()
                    .filter(|p| {
                        p["url"]
                            .as_str()
                            .map(|u| normalize_url(u) != normalize_url(&url))
                            .unwrap_or(true)
                    })
                    .map(|p| crate::models::indexing_health::SiblingArticle {
                        url: p["url"].as_str().unwrap_or("").to_string(),
                        title: p["title"].as_str().unwrap_or("").to_string(),
                        h1: p["h1"].as_str().unwrap_or("").to_string(),
                        word_count: p["word_count"].as_u64().unwrap_or(0) as usize,
                        impressions: p["impressions"].as_f64(),
                    })
                    .collect();
                let headings: Option<Vec<String>> = c["shared_headings"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                (sibs.len(), sibs, headings)
            })
            .unwrap_or((0, vec![], None));

        // Content audit lookup
        let audit = audit_by_slug.get(&slug);
        let word_count = audit
            .and_then(|a| a["word_count"].as_u64())
            .unwrap_or(0) as usize;
        let incoming_links = audit
            .and_then(|a| a["checks"]["internal_links"]["value"].as_u64())
            .unwrap_or(0) as usize;
        let health = audit
            .and_then(|a| a["health"].as_str())
            .unwrap_or("unknown")
            .to_string();

        let title = audit
            .and_then(|a| a["title"].as_str())
            .unwrap_or("")
            .to_string();
        let h1 = audit
            .and_then(|a| a["checks"]["h1_keyword"]["value"].as_str())
            .unwrap_or("")
            .to_string();

        // Article lookup for id/file
        // Normalize drift slug (may have numeric prefix like "265-hub-coffee-beans")
        let normalized_slug = crate::content::slug::normalize_url_slug(&slug);
        let article = article_by_slug.get(&normalized_slug)
            .or_else(|| article_by_slug.get(&slug))
            .or_else(|| {
                // Fallback: extract normalized slug from the full URL
                let url_slug = crate::content::slug::extract_slug_from_url(&url);
                article_by_slug.get(&url_slug)
            });
        let article_id = article
            .and_then(|a| a["id"].as_i64())
            .unwrap_or(0);
        let file = article
            .and_then(|a| a["file"].as_str())
            .unwrap_or("")
            .to_string();
        let target_keyword = article
            .and_then(|a| a["target_keyword"].as_str())
            .unwrap_or("")
            .to_string();

        let diagnosis = TargetDiagnosis {
            has_links: incoming_links >= 1,
            is_long: word_count >= 600,
            has_cluster_siblings: sibling_count > 0,
            suspected_root_cause: if sibling_count > 0 {
                "cannibalization"
            } else if incoming_links == 0 {
                "insufficient_internal_links"
            } else if word_count < 600 {
                "thin_content"
            } else {
                "unknown"
            }
            .to_string(),
        };

        // Build source candidates for add_links targets
        let mut source_candidates: Vec<crate::models::indexing_health::LinkSourceCandidate> =
            Vec::new();
        if incoming_links == 0 && article_id > 0 {
            let target_outgoing = outgoing_by_id.get(&article_id);
            for (src_slug, src_art) in &article_by_slug {
                if src_slug == &slug {
                    continue;
                }
                let src_id = src_art["id"].as_i64().unwrap_or(0);
                if src_id == 0 || src_id == article_id {
                    continue;
                }
                // Skip if already links to target
                let already_links = target_outgoing
                    .map(|out| out.contains(&src_id))
                    .unwrap_or(false);
                if already_links {
                    continue;
                }
                // Simple topical relevance: same cluster or shared keyword
                let src_kw = src_art["target_keyword"].as_str().unwrap_or("");
                let in_cluster = siblings.iter().any(|s| {
                    crate::content::slug::extract_slug_from_url(&s.url) == *src_slug
                });
                let shares_kw = !target_keyword.is_empty()
                    && !src_kw.is_empty()
                    && target_keyword.to_lowercase() == src_kw.to_lowercase();
                if in_cluster || shares_kw {
                    source_candidates.push(crate::models::indexing_health::LinkSourceCandidate {
                        article_id: src_id,
                        slug: src_slug.clone(),
                        title: src_art["title"].as_str().unwrap_or("").to_string(),
                        file: src_art["file"].as_str().unwrap_or("").to_string(),
                        reason: if in_cluster {
                            "cluster sibling".to_string()
                        } else {
                            "shared target keyword".to_string()
                        },
                    });
                }
            }
            // Limit to top 8 candidates
            source_candidates.truncate(8);
        }

        targets.push(IndexingTargetContext {
            target: crate::models::indexing_health::TargetArticleSummary {
                url,
                slug,
                reason_code,
                title,
                h1,
                word_count,
                incoming_links,
                content_audit_health: health,
                article_id,
                file,
            },
            cluster: if sibling_count > 0 {
                Some(crate::models::indexing_health::ClusterContext {
                    cluster_id: cluster
                        .and_then(|c| c["cluster_id"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    theme: cluster
                        .and_then(|c| c["theme"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    sibling_count,
                    siblings,
                    shared_headings,
                    exact_keyword_dupe: false, // populated by reduce step if needed
                })
            } else {
                None
            },
            diagnosis,
            source_candidates,
        });
    }

    // 5. Write contexts to disk
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "project_id": &task.project_id,
        "targets": targets,
    });
    let contexts_json = match serde_json::to_string_pretty(&contexts_doc) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize target contexts: {}", e),
                output: None,
            }
        }
    };
    if let Err(e) = std::fs::write(&contexts_path, &contexts_json) {
        return StepResult {
            success: false,
            message: format!("Failed to write target contexts: {}", e),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Built context for {} not-indexed URL(s), {} with cluster siblings",
            targets.len(),
            targets.iter().filter(|t| t.diagnosis.has_cluster_siblings).count()
        ),
        output: Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "target_count": targets.len(),
                "with_cluster_siblings": targets.iter().filter(|t| t.diagnosis.has_cluster_siblings).count(),
                "contexts_path": contexts_path.display().to_string(),
            }))
            .unwrap_or_default(),
        ),
    }
}

fn normalize_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_lowercase()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Distinctiveness Review (agentic)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic distinctiveness review.
/// For each target with cluster siblings, ask the agent to judge whether the
/// target's title, H1, and focus are sufficiently distinct from its siblings.
pub(crate) fn exec_ihc_distinctiveness_review(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    _context_json: Option<&str>,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load target contexts
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &contexts_path,
        "indexing_target_contexts.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let targets: Vec<IndexingTargetContext> = match contexts_doc["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect::<Vec<IndexingTargetContext>>()
        .into_iter()
        .filter(|t| t.diagnosis.has_cluster_siblings)
        .collect::<Vec<_>>()
    {
        t if t.is_empty() => {
            return StepResult {
                success: true,
                message: "No targets with cluster siblings — distinctiveness review skipped."
                    .to_string(),
                output: Some("[]".to_string()),
            }
        }
        t => t,
    };

    // Load skill
    let repo_root = Path::new(project_path);
    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "indexing-distinctiveness") {
        Ok(s) => s.content,
        Err(msg) => {
            return StepResult { success: false, message: msg, output: None }
        }
    };

    let mut verdicts: Vec<DistinctivenessVerdict> = Vec::new();

    // Process one target at a time to stay within prompt budget
    for target_ctx in &targets {
        let prompt = build_distinctiveness_prompt(&skill, target_ctx);

        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Failed to create runtime for extraction: {}", e),
                    output: None,
                }
            }
        };

        let extract_result = rt.block_on(async {
            crate::rig::extraction::extract_structured::<DistinctivenessVerdict>(
                agent_provider,
                &prompt,
                Some("You are an expert SEO content strategist. Judge article distinctiveness precisely."),
                Some("direct"),
                None,
            )
            .await
        });

        match extract_result {
            Ok(v) => {
                log::info!(
                    "[ihc_distinctiveness] {} → {} ({})",
                    target_ctx.target.url,
                    v.verdict,
                    v.confidence
                );
                verdicts.push(v);
            }
            Err(e) => {
                log::warn!(
                    "[ihc_distinctiveness] failed for {}: {}",
                    target_ctx.target.url,
                    e
                );
                // Push a fallback verdict so the reduce step can still proceed
                verdicts.push(DistinctivenessVerdict {
                    target_url: target_ctx.target.url.clone(),
                    verdict: "DISTINCT".to_string(),
                    confidence: "low".to_string(),
                    recommendation: "NO_ACTION".to_string(),
                    keep_url: None,
                    redirect_url: None,
                    reason: format!("Extraction failed: {}. Defaulting to no action.", e),
                    suggested_title: None,
                    suggested_h1: None,
                });
            }
        }
    }

    // Write verdicts to disk
    let verdicts_path = paths
        .automation_dir
        .join("indexing_distinctiveness_verdicts.json");
    let verdicts_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "verdicts": verdicts,
    });
    let verdicts_json = match serde_json::to_string_pretty(&verdicts_doc) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize verdicts: {}", e),
                output: None,
            }
        }
    };
    let _ = std::fs::write(&verdicts_path, &verdicts_json);

    StepResult {
        success: true,
        message: format!(
            "Distinctiveness review: {} verdict(s), {} OVERLAP",
            verdicts.len(),
            verdicts.iter().filter(|v| v.verdict == "OVERLAP").count()
        ),
        output: Some(verdicts_json),
    }
}

fn build_distinctiveness_prompt(skill: &str, target: &IndexingTargetContext) -> String {
    let siblings_json = match &target.cluster {
        Some(c) => serde_json::to_string_pretty(&c.siblings).unwrap_or_default(),
        None => "[]".to_string(),
    };

    format!(
        "{skill}\n\n---\n\n## Target Article\n\n- URL: {url}\n- Title: {title}\n- H1: {h1}\n- Word count: {wc}\n- Reason not indexed: {reason}\n\n## Cluster Siblings\n\n{siblings}\n\nReturn a single JSON object matching the DistinctivenessVerdict structure.",
        skill = skill,
        url = target.target.url,
        title = target.target.title,
        h1 = target.target.h1,
        wc = target.target.word_count,
        reason = target.target.reason_code,
        siblings = siblings_json,
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Reduce Plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Read all previous step outputs and produce the final campaign plan.
pub(crate) fn exec_ihc_reduce_plan(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load target contexts
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts_doc: serde_json::Value = match std::fs::read_to_string(&contexts_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({ "targets": [] })),
        Err(_) => serde_json::json!({ "targets": [] }),
    };

    let target_contexts: Vec<IndexingTargetContext> = contexts_doc["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    // Load distinctiveness verdicts
    let verdicts_path = paths
        .automation_dir
        .join("indexing_distinctiveness_verdicts.json");
    let verdicts: HashMap<String, DistinctivenessVerdict> = std::fs::read_to_string(&verdicts_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["verdicts"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            let verdict: DistinctivenessVerdict = serde_json::from_value(v).ok()?;
            Some((verdict.target_url.clone(), verdict))
        })
        .collect();

    // Load exact keyword duplicates for flagging
    let dupes_path = paths.automation_dir.join("exact_keyword_duplicates.json");
    let dupes_doc: serde_json::Value = std::fs::read_to_string(&dupes_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "duplicates": [] }));
    let dupe_keywords: Vec<String> = dupes_doc["duplicates"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|d| d["keyword"].as_str().map(String::from))
        .collect();

    let mut plans: Vec<IndexingTargetPlan> = Vec::new();
    let mut summary = IndexingCampaignSummary {
        total_targets: target_contexts.len(),
        fix_content: 0,
        add_links: 0,
        merge: 0,
        rewrite_title_h1: 0,
        no_action: 0,
    };

    for ctx in &target_contexts {
        let verdict = verdicts.get(&ctx.target.url);
        let action = determine_action(ctx, verdict, &dupe_keywords);

        match action.as_str() {
            "fix_content" => summary.fix_content += 1,
            "add_links" => summary.add_links += 1,
            "merge" => summary.merge += 1,
            "rewrite_title_h1" => summary.rewrite_title_h1 += 1,
            _ => summary.no_action += 1,
        }

        plans.push(IndexingTargetPlan {
            url: ctx.target.url.clone(),
            reason_code: ctx.target.reason_code.clone(),
            recommended_action: action,
            context_artifact_key: Some(format!(
                "ihc_target_context_{}",
                slugify_url(&ctx.target.url)
            )),
            distinctiveness_verdict: verdict.cloned(),
            content_audit_summary: None,
            word_count: Some(ctx.target.word_count),
            incoming_links: Some(ctx.target.incoming_links),
            file: Some(ctx.target.file.clone()).filter(|f| !f.is_empty()),
        });
    }

    // Capture summary values before moving summary into plan
    let summary_msg = format!(
        "Campaign plan: {} fix_content, {} add_links, {} merge, {} rewrite_title_h1, {} no_action",
        summary.fix_content, summary.add_links, summary.merge, summary.rewrite_title_h1, summary.no_action
    );

    let plan = IndexingCampaignPlan {
        generated_at: chrono::Utc::now().to_rfc3339(),
        targets: plans,
        summary,
    };

    let plan_json = match serde_json::to_string_pretty(&plan) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize campaign plan: {}", e),
                output: None,
            }
        }
    };

    // Save to database (new primary storage)
    let now_iso = chrono::Utc::now().to_rfc3339();
    if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let _ = crate::db::content_audit::save_audit_artifact(
            &db,
            &task.project_id,
            "indexing_campaign_plan",
            &now_iso,
            &plan_json,
        );
    }

    StepResult {
        success: true,
        message: summary_msg,
        output: Some(plan_json),
    }
}

fn determine_action(
    ctx: &IndexingTargetContext,
    verdict: Option<&DistinctivenessVerdict>,
    _dupe_keywords: &[String],
) -> String {
    // Priority order from spec
    if ctx.target.content_audit_health == "poor" {
        return "fix_content".to_string();
    }

    if ctx.target.incoming_links == 0 {
        return "add_links".to_string();
    }

    if let Some(v) = verdict {
        if v.verdict == "OVERLAP" && v.confidence == "high" {
            return "merge".to_string();
        }
        if v.verdict == "OVERLAP" && (v.confidence == "medium" || v.confidence == "low") {
            return "rewrite_title_h1".to_string();
        }
    }

    if ctx.target.reason_code == "not_indexed_crawled"
        && ctx.diagnosis.is_long
        && ctx.diagnosis.has_links
    {
        return "no_action".to_string();
    }

    "fix_indexing".to_string()
}

fn slugify_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .replace('/', "_")
        .replace('.', "_")
        .replace(':', "_")
        .to_lowercase()
}


// ═══════════════════════════════════════════════════════════════════════════════
// Post-action: Spawn child tasks from campaign plan
// ═══════════════════════════════════════════════════════════════════════════════

use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};
use rusqlite::Connection;

/// Read the campaign plan and spawn appropriate child fix tasks.
pub(crate) fn spawn_campaign_children(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Load campaign plan from DB (primary) or JSON fallback
    let plan: IndexingCampaignPlan = {
        let db_plan = crate::db::content_audit::get_latest_audit_artifact(conn, &parent_task.project_id, "indexing_campaign_plan")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<IndexingCampaignPlan>(v).ok());
        match db_plan {
            Some(p) => p,
            None => {
                let plan_path = paths.automation_dir.join("indexing_campaign_plan.json");
                match std::fs::read_to_string(&plan_path) {
                    Ok(raw) => match serde_json::from_str(&raw) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!("[ihc_post_action] failed to parse campaign plan: {}", e);
                            return vec![];
                        }
                    },
                    Err(e) => {
                        log::warn!("[ihc_post_action] plan file not found: {}", e);
                        return vec![];
                    }
                }
            }
        }
    };

    // Load full target contexts so we can attach cluster artifacts to child tasks
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts: HashMap<String, IndexingTargetContext> = std::fs::read_to_string(&contexts_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["targets"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            let ctx: IndexingTargetContext = serde_json::from_value(v).ok()?;
            Some((ctx.target.url.clone(), ctx))
        })
        .collect();

    // Load content audit so fix_content specs get actual failed checks (DB primary, JSON fallback)
    let audit_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_audit_report_as_json(&conn, &parent_task.project_id).ok().flatten()
            });
        db_doc.unwrap_or_else(|| {
            let audit_path = paths.automation_dir.join("content_audit.json");
            std::fs::read_to_string(&audit_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({"articles": []}))
        })
    };
    let audit_articles = audit_doc["articles"].as_array().cloned().unwrap_or_default();
    let mut audit_by_file: HashMap<String, &serde_json::Value> = HashMap::new();
    let mut audit_by_slug: HashMap<String, &serde_json::Value> = HashMap::new();
    for a in &audit_articles {
        if let Some(f) = a["file"].as_str() {
            if !f.is_empty() { audit_by_file.insert(f.to_string(), a); }
        }
        if let Some(s) = a["url_slug"].as_str() {
            if !s.is_empty() { audit_by_slug.insert(s.to_string(), a); }
        }
    }

    let mut created_ids: Vec<String> = Vec::new();

    // Collect all spawnable targets with priority ordering
    let mut spawnable: Vec<(&IndexingTargetPlan, TaskSpec)> = Vec::new();

    for target in &plan.targets {
        let ctx = contexts.get(&target.url);

        // Skip tasks that require a known article but don't have one
        let requires_article = matches!(
            target.recommended_action.as_str(),
            "fix_content" | "add_links" | "rewrite_title_h1"
        );
        if requires_article {
            match ctx {
                None => {
                    log::warn!(
                        "[ihc_post_action] skipping {} for {} — no target context available",
                        target.recommended_action, target.url
                    );
                    continue;
                }
                Some(ctx) if ctx.target.article_id == 0 => {
                    log::warn!(
                        "[ihc_post_action] skipping {} for {} — no matching article in articles.json (slug lookup failed)",
                        target.recommended_action, target.url
                    );
                    continue;
                }
                _ => {}
            }
        }

        // Look up audit row for this target so fix_content gets real issues
        let audit_row = ctx.and_then(|c| {
            audit_by_file.get(&c.target.file)
                .or_else(|| audit_by_slug.get(&c.target.slug))
                .copied()
        });

        let spec = match target.recommended_action.as_str() {
            "fix_content" => Some(build_fix_content_spec(parent_task, target, ctx, audit_row)),
            "add_links" => Some(build_add_links_spec(parent_task, target, ctx)),
            "rewrite_title_h1" => Some(build_rewrite_spec(parent_task, target, ctx)),
            "merge" => {
                // Merge recommendations require user approval via CannibalizationPicker.
                // Do NOT auto-spawn. Instead, log for visibility.
                log::info!(
                    "[ihc_post_action] merge recommended for {} — awaiting user approval",
                    target.url
                );
                None
            }
            "no_action" | _ => None,
        };

        if let Some(spec) = spec {
            spawnable.push((target, spec));
        }
    }

    // Priority: fix_content > add_links > rewrite_title_h1
    spawnable.sort_by(|(a, _), (b, _)| {
        let priority = |action: &str| match action {
            "fix_content" => 0,
            "add_links" => 1,
            "rewrite_title_h1" => 2,
            _ => 3,
        };
        priority(a.recommended_action.as_str()).cmp(&priority(b.recommended_action.as_str()))
    });

    for (target, spec) in spawnable {
        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[ihc_post_action] spawned {} for {}",
                    task.task_type,
                    target.url
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!(
                    "[ihc_post_action] failed to spawn task for {}: {}",
                    target.url,
                    e
                );
            }
        }
    }

    log::info!(
        "[ihc_post_action] created {} child tasks from campaign plan",
        created_ids.len()
    );
    created_ids
}

fn build_fix_content_spec(
    parent: &Task,
    target: &IndexingTargetPlan,
    ctx: Option<&IndexingTargetContext>,
    audit_row: Option<&serde_json::Value>,
) -> TaskSpec {
    let url_slug = crate::content::slug::extract_slug_from_url(&target.url);
    let article_id = ctx.map(|c| c.target.article_id).unwrap_or(0);
    let idempotency_key = format!("fix_content_article:{}:{}", parent.project_id, article_id);

    // Build artifacts required by the fix_content_article pipeline
    let mut artifacts = vec![];
    if let Some(ctx) = ctx {
        let article_id = ctx.target.article_id;
        if article_id > 0 {
            // Build suggestions from actual audit failed checks instead of generic stubs
            let mut suggestions = vec![];
            if let Some(audit) = audit_row {
                if let Some(checks) = audit["checks"].as_object() {
                    for (check_name, check_data) in checks {
                        if check_data["pass"].as_bool() == Some(false) {
                            let label = check_data["label"].as_str().unwrap_or(check_name);
                            let value = check_data["value"].as_str().unwrap_or("");
                            let current = if value.is_empty() { "check failed".to_string() } else { value.to_string() };
                            suggestions.push(serde_json::json!({
                                "category": check_name,
                                "current": current,
                                "proposed": format!("Fix: {}", label),
                                "reason": label,
                                "priority": "high"
                            }));
                        }
                    }
                }
                // Also include quality critical issues if present
                if let Some(critical) = audit["quality_critical"].as_array() {
                    for issue in critical {
                        if let Some(text) = issue.as_str() {
                            suggestions.push(serde_json::json!({
                                "category": "quality_critical",
                                "current": "quality issue",
                                "proposed": format!("Fix: {}", text),
                                "reason": text,
                                "priority": "high"
                            }));
                        }
                    }
                }
            }
            // Fallback to at least one generic suggestion if audit had no failed checks
            if suggestions.is_empty() {
                suggestions.push(serde_json::json!({
                    "category": "content_depth",
                    "current": "content flagged as poor",
                    "proposed": "Improve depth, structure, and keyword usage",
                    "reason": "Content audit health = poor but no specific check failures were recorded",
                    "priority": "medium"
                }));
            }

            let rec_key = format!("recommendations_{}", article_id);
            let rec_content = serde_json::json!({
                "article_id": article_id,
                "article_file": &ctx.target.file,
                "article_title": &ctx.target.title,
                "target_keyword": &ctx.target.title,
                "suggestions": suggestions
            });
            artifacts.push(crate::models::task::TaskArtifact {
                key: rec_key,
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("indexing_health_campaign".to_string()),
                content: Some(rec_content.to_string()),
            });
        }
    }

    TaskSpec {
        project_id: parent.project_id.clone(),
        task_type: "fix_content_article".to_string(),
        title: Some(format!("Fix content: {}", url_slug)),
        description: Some(format!(
            "URL: {}\nRecommended action: fix_content (content audit health = poor)\nParent campaign: {}",
            target.url, parent.id
        )),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 30 }),
        depends_on: vec![parent.id.clone()],
        artifacts,
        ..Default::default()
    }
}

fn build_add_links_spec(
    parent: &Task,
    target: &IndexingTargetPlan,
    ctx: Option<&IndexingTargetContext>,
) -> TaskSpec {
    let url_slug = crate::content::slug::extract_slug_from_url(&target.url);
    // Use article_id (not parent.id) so dedup works across repeated campaign runs.
    let article_id = ctx.map(|c| c.target.article_id).unwrap_or(0);
    let idempotency_key = format!("ihc-add-links:{}:{}", parent.project_id, article_id);

    // Build the indexing_link_target artifact that fix_indexing_internal_links expects
    let mut artifacts = vec![];
    if let Some(ctx) = ctx {
        let source_candidates_json: Vec<serde_json::Value> = ctx
            .source_candidates
            .iter()
            .map(|s| {
                serde_json::json!({
                    "article_id": s.article_id,
                    "slug": &s.slug,
                    "title": &s.title,
                    "file": &s.file,
                    "reason": &s.reason,
                })
            })
            .collect();

        let artifact_content = serde_json::json!({
            "campaign_task_id": &parent.id,
            "target": {
                "url": &ctx.target.url,
                "slug": &ctx.target.slug,
                "article_id": ctx.target.article_id,
                "file": &ctx.target.file,
                "reason_code": &ctx.target.reason_code,
                "incoming_link_count_before": ctx.target.incoming_links,
                "target_keyword": &ctx.target.title,
                "source_candidates": source_candidates_json,
            }
        });

        artifacts.push(crate::models::task::TaskArtifact {
            key: "indexing_link_target".to_string(),
            path: None,
            artifact_type: Some("indexing_link_target".to_string()),
            source: Some("indexing_health_campaign".to_string()),
            content: Some(artifact_content.to_string()),
        });
    }

    TaskSpec {
        project_id: parent.project_id.clone(),
        task_type: "fix_indexing_internal_links".to_string(),
        title: Some(format!("Add links: {}", url_slug)),
        description: Some(format!(
            "URL: {}\nRecommended action: add_links (zero incoming internal links)\nParent campaign: {}",
            target.url, parent.id
        )),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 30 }),
        depends_on: vec![parent.id.clone()],
        artifacts,
        ..Default::default()
    }
}

fn build_rewrite_spec(
    parent: &Task,
    target: &IndexingTargetPlan,
    ctx: Option<&IndexingTargetContext>,
) -> TaskSpec {
    let url_slug = crate::content::slug::extract_slug_from_url(&target.url);
    // Use article_id (not parent.id) so dedup works across repeated campaign runs.
    let article_id = ctx.map(|c| c.target.article_id).unwrap_or(0);
    let idempotency_key = format!("ihc-rewrite:{}:{}", parent.project_id, article_id);

    // Build a richer description that includes cluster context if available
    let mut description = format!(
        "URL: {}\nRecommended action: rewrite_title_h1\nReason: {}\nParent campaign: {}",
        target.url,
        target
            .distinctiveness_verdict
            .as_ref()
            .map(|v| v.reason.clone())
            .unwrap_or_default(),
        parent.id
    );

    if let Some(v) = &target.distinctiveness_verdict {
        if let Some(title) = &v.suggested_title {
            description.push_str(&format!("\nSuggested title: {}", title));
        }
        if let Some(h1) = &v.suggested_h1 {
            description.push_str(&format!("\nSuggested H1: {}", h1));
        }
    }

    // Build cluster context artifact for the agent
    let mut artifacts = vec![];
    if let Some(ctx) = ctx {
        if let Ok(json) = serde_json::to_string_pretty(ctx) {
            artifacts.push(crate::models::task::TaskArtifact {
                key: "indexing_target_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("indexing_health_campaign".to_string()),
                content: Some(json),
            });
        }
    }

    TaskSpec {
        project_id: parent.project_id.clone(),
        task_type: "fix_indexing".to_string(),
        title: Some(format!("Rewrite title/H1: {}", url_slug)),
        description: Some(description),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 30 }),
        depends_on: vec![parent.id.clone()],
        artifacts,
        ..Default::default()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::indexing_health::{
        DistinctivenessVerdict, IndexingTargetContext, TargetArticleSummary, TargetDiagnosis,
    };
    use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};
    use crate::engine::spawner::DeduplicationPolicy;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn dummy_task() -> Task {
        Task {
            id: "task-123".to_string(),
            task_type: "indexing_health_campaign".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: Priority::High,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::None,
            agent_policy: AgentPolicy::Required,
            title: Some("Test Campaign".to_string()),
            description: Some("Test description".to_string()),
            project_id: "proj-abc".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            not_before: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn dummy_target_ctx(health: &str, links: usize, is_long: bool, reason: &str) -> IndexingTargetContext {
        IndexingTargetContext {
            target: TargetArticleSummary {
                url: "https://example.com/blog/test-article".to_string(),
                slug: "test-article".to_string(),
                reason_code: reason.to_string(),
                title: "Test Article".to_string(),
                h1: "Test H1".to_string(),
                word_count: 800,
                incoming_links: links,
                content_audit_health: health.to_string(),
                article_id: 42,
                file: "content/test-article.mdx".to_string(),
            },
            cluster: None,
            diagnosis: TargetDiagnosis {
                has_links: links > 0,
                is_long,
                has_cluster_siblings: false,
                suspected_root_cause: "test".to_string(),
            },
            source_candidates: vec![],
        }
    }

    fn overlap_verdict(confidence: &str) -> DistinctivenessVerdict {
        DistinctivenessVerdict {
            target_url: "https://example.com/blog/test-article".to_string(),
            verdict: "OVERLAP".to_string(),
            confidence: confidence.to_string(),
            recommendation: "REWRITE".to_string(),
            keep_url: None,
            redirect_url: None,
            reason: "Shares H2s with sibling".to_string(),
            suggested_title: Some("Better Title".to_string()),
            suggested_h1: Some("Better H1".to_string()),
        }
    }

    fn distinct_verdict() -> DistinctivenessVerdict {
        DistinctivenessVerdict {
            target_url: "https://example.com/blog/test-article".to_string(),
            verdict: "DISTINCT".to_string(),
            confidence: "high".to_string(),
            recommendation: "NO_ACTION".to_string(),
            keep_url: None,
            redirect_url: None,
            reason: "Unique angle".to_string(),
            suggested_title: None,
            suggested_h1: None,
        }
    }

    // ─── determine_action tests ─────────────────────────────────────────────────

    #[test]
    fn determine_action_poor_health_returns_fix_content() {
        let ctx = dummy_target_ctx("poor", 5, true, "not_indexed_crawled");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "fix_content");
    }

    #[test]
    fn determine_action_zero_links_returns_add_links() {
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_crawled");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "add_links");
    }

    #[test]
    fn determine_action_high_overlap_returns_merge() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = overlap_verdict("high");
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "merge");
    }

    #[test]
    fn determine_action_medium_overlap_returns_rewrite() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = overlap_verdict("medium");
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "rewrite_title_h1");
    }

    #[test]
    fn determine_action_low_overlap_returns_rewrite() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = overlap_verdict("low");
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "rewrite_title_h1");
    }

    #[test]
    fn determine_action_not_indexed_crawled_long_with_links_no_action() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = distinct_verdict();
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "no_action");
    }

    #[test]
    fn determine_action_not_indexed_other_with_links_fix_indexing() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_other");
        let v = distinct_verdict();
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "fix_indexing");
    }

    #[test]
    fn determine_action_distinct_short_fix_indexing() {
        let ctx = dummy_target_ctx("good", 5, false, "not_indexed_crawled");
        let v = distinct_verdict();
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "fix_indexing");
    }

    #[test]
    fn determine_action_no_verdict_not_indexed_crawled_long_no_action() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "no_action");
    }

    #[test]
    fn determine_action_no_verdict_not_indexed_other_fix_indexing() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_other");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "fix_indexing");
    }

    // ─── slugify_url tests ──────────────────────────────────────────────────────

    #[test]
    fn slugify_url_strips_protocol() {
        assert_eq!(
            slugify_url("https://example.com/blog/my-post"),
            "example_com_blog_my-post"
        );
    }

    #[test]
    fn slugify_url_strips_www() {
        assert_eq!(
            slugify_url("https://www.example.com/page"),
            "example_com_page"
        );
    }

    #[test]
    fn slugify_url_http() {
        assert_eq!(
            slugify_url("http://example.com/path/to/page"),
            "example_com_path_to_page"
        );
    }

    #[test]
    fn slugify_url_lowercases() {
        assert_eq!(
            slugify_url("https://Example.COM/Blog/Page"),
            "example_com_blog_page"
        );
    }

    // ─── check_artifact tests ───────────────────────────────────────────────────

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn paths_from_dir(dir: &std::path::Path) -> ProjectPaths {
        ProjectPaths::from_path(dir.to_str().unwrap())
    }

    #[test]
    fn check_artifact_missing_file_not_fresh() {
        let dir = unique_temp_dir("ihc_test");
        let paths = paths_from_dir(&dir);
        let check = check_artifact(&paths, "missing.json", chrono::Duration::days(7));
        assert!(!check.fresh);
        assert_eq!(check.age_hours, None);
        assert_eq!(check.action, Some("auto_enqueue_missing".to_string()));
    }

    #[test]
    fn check_artifact_fresh_file_is_fresh() {
        let dir = unique_temp_dir("ihc_test");
        let paths = paths_from_dir(&dir);
        std::fs::create_dir_all(&paths.automation_dir).unwrap();
        std::fs::write(paths.automation_dir.join("fresh.json"), "{}").unwrap();

        let check = check_artifact(&paths, "fresh.json", chrono::Duration::days(7));
        assert!(check.fresh);
        assert!(check.age_hours.unwrap() < 1);
        assert_eq!(check.action, None);
    }

    #[test]
    fn check_artifact_cannibalization_auto_enqueues() {
        let dir = unique_temp_dir("ihc_test");
        let paths = paths_from_dir(&dir);
        std::fs::create_dir_all(&paths.automation_dir).unwrap();
        // Fresh file → no action needed
        std::fs::write(paths.automation_dir.join("cannibalization_strategy.json"), "{}").unwrap();
        let check = check_artifact(
            &paths,
            "cannibalization_strategy.json",
            chrono::Duration::days(7),
        );
        assert!(check.fresh);
        assert_eq!(check.action, None);
        // The stale action mapping is verified by the prerequisite_report test below.
    }

    // ─── build_rewrite_spec tests ───────────────────────────────────────────────

    fn dummy_target_plan(action: &str) -> IndexingTargetPlan {
        IndexingTargetPlan {
            url: "https://example.com/blog/test-article".to_string(),
            reason_code: "not_indexed_crawled".to_string(),
            recommended_action: action.to_string(),
            context_artifact_key: None,
            distinctiveness_verdict: Some(overlap_verdict("medium")),
            content_audit_summary: None,
            word_count: Some(800),
            incoming_links: Some(3),
            file: Some("content/test-article.mdx".to_string()),
        }
    }

    #[test]
    fn build_rewrite_spec_sets_correct_task_type() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let spec = build_rewrite_spec(&parent, &target, None);
        assert_eq!(spec.task_type, "fix_indexing");
        assert_eq!(spec.project_id, "proj-abc");
    }

    #[test]
    fn build_rewrite_spec_includes_suggested_title() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let spec = build_rewrite_spec(&parent, &target, None);
        let desc = spec.description.unwrap();
        assert!(desc.contains("Suggested title: Better Title"));
        assert!(desc.contains("Suggested H1: Better H1"));
        assert!(desc.contains("test-article"));
    }

    #[test]
    fn build_rewrite_spec_includes_context_artifact() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let ctx = dummy_target_ctx("good", 3, true, "not_indexed_crawled");
        let spec = build_rewrite_spec(&parent, &target, Some(&ctx));

        assert_eq!(spec.artifacts.len(), 1);
        assert_eq!(spec.artifacts[0].key, "indexing_target_context");
        assert_eq!(spec.artifacts[0].artifact_type, Some("json".to_string()));
        assert!(spec.artifacts[0].content.as_ref().unwrap().contains("test-article"));
    }

    #[test]
    fn build_rewrite_spec_has_idempotency_key() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_rewrite_spec(&parent, &target, Some(&ctx));
        let key = spec.idempotency_key.unwrap();
        assert!(key.starts_with("ihc-rewrite:"));
        assert!(key.contains("proj-abc"));
        // Key uses article_id (42), not parent.id, for cross-run dedup
        assert!(key.contains("42"));
        assert!(!key.contains("task-123"));
    }

    #[test]
    fn build_rewrite_spec_has_cooldown_dedup() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let spec = build_rewrite_spec(&parent, &target, None);
        match spec.dedup_policy {
            Some(DeduplicationPolicy::Cooldown { days }) => assert_eq!(days, 30),
            other => panic!("Expected Cooldown dedup policy, got {:?}", other),
        }
    }

    // ─── build_fix_content_spec tests ───────────────────────────────────────────

    #[test]
    fn build_fix_content_spec_sets_correct_type() {
        let parent = dummy_task();
        let target = dummy_target_plan("fix_content");
        let ctx = dummy_target_ctx("poor", 3, true, "not_indexed_other");
        let spec = build_fix_content_spec(&parent, &target, Some(&ctx), None);
        assert_eq!(spec.task_type, "fix_content_article");
        assert_eq!(spec.project_id, "proj-abc");
    }

    #[test]
    fn build_fix_content_spec_description_includes_url() {
        let parent = dummy_task();
        let target = dummy_target_plan("fix_content");
        let ctx = dummy_target_ctx("poor", 3, true, "not_indexed_other");
        let spec = build_fix_content_spec(&parent, &target, Some(&ctx), None);
        let desc = spec.description.unwrap();
        assert!(desc.contains("test-article"));
        assert!(desc.contains("fix_content"));
    }

    #[test]
    fn build_fix_content_spec_includes_recommendation_artifact() {
        let parent = dummy_task();
        let target = dummy_target_plan("fix_content");
        let ctx = dummy_target_ctx("poor", 3, true, "not_indexed_other");
        let spec = build_fix_content_spec(&parent, &target, Some(&ctx), None);
        assert_eq!(spec.artifacts.len(), 1);
        assert!(spec.artifacts[0].key.starts_with("recommendations_"));
        let content = spec.artifacts[0].content.as_ref().unwrap();
        assert!(content.contains("article_id"));
        assert!(content.contains("suggestions"));
        assert!(content.contains("content_depth"));
    }

    // ─── build_add_links_spec tests ─────────────────────────────────────────────

    #[test]
    fn build_add_links_spec_sets_correct_type() {
        let parent = dummy_task();
        let target = dummy_target_plan("add_links");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_add_links_spec(&parent, &target, Some(&ctx));
        assert_eq!(spec.task_type, "fix_indexing_internal_links");
        assert_eq!(spec.project_id, "proj-abc");
    }

    #[test]
    fn build_add_links_spec_description_includes_url() {
        let parent = dummy_task();
        let target = dummy_target_plan("add_links");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_add_links_spec(&parent, &target, Some(&ctx));
        let desc = spec.description.unwrap();
        assert!(desc.contains("test-article"));
        assert!(desc.contains("add_links"));
    }

    #[test]
    fn build_add_links_spec_includes_target_artifact() {
        let parent = dummy_task();
        let target = dummy_target_plan("add_links");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_add_links_spec(&parent, &target, Some(&ctx));
        assert_eq!(spec.artifacts.len(), 1);
        assert_eq!(spec.artifacts[0].key, "indexing_link_target");
        let content = spec.artifacts[0].content.as_ref().unwrap();
        assert!(content.contains("campaign_task_id"));
        assert!(content.contains("test-article"));
        assert!(content.contains("article_id"));
    }

    // ─── PrerequisiteReport serialization tests ─────────────────────────────────

    #[test]
    fn prerequisite_report_serializes_correctly() {
        let report = PrerequisiteReport {
            all_fresh: false,
            checks: vec![
                PrerequisiteCheck {
                    artifact: "gsc_collection.json".to_string(),
                    fresh: true,
                    age_hours: Some(12),
                    action: None,
                },
                PrerequisiteCheck {
                    artifact: "cannibalization_strategy.json".to_string(),
                    fresh: false,
                    age_hours: Some(500),
                    action: Some("auto_enqueue_cannibalization_audit".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("gsc_collection.json"));
        assert!(json.contains("auto_enqueue_cannibalization_audit"));
        assert!(json.contains("false"));
    }

    // ─── IndexingCampaignPlan serialization tests ───────────────────────────────

    #[test]
    fn campaign_plan_roundtrips_json() {
        let plan = IndexingCampaignPlan {
            generated_at: "2024-01-01".to_string(),
            targets: vec![
                IndexingTargetPlan {
                    url: "https://example.com/a".to_string(),
                    reason_code: "not_indexed_crawled".to_string(),
                    recommended_action: "rewrite_title_h1".to_string(),
                    context_artifact_key: None,
                    distinctiveness_verdict: Some(overlap_verdict("medium")),
                    content_audit_summary: None,
                    word_count: Some(500),
                    incoming_links: Some(2),
                    file: Some("content/a.mdx".to_string()),
                },
            ],
            summary: IndexingCampaignSummary {
                total_targets: 1,
                fix_content: 0,
                add_links: 0,
                merge: 0,
                rewrite_title_h1: 1,
                no_action: 0,
            },
        };
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let parsed: IndexingCampaignPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.targets.len(), 1);
        assert_eq!(parsed.summary.rewrite_title_h1, 1);
    }
}
