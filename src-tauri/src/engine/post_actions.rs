/// Domain-specific post-step and post-task side effects.
///
/// This module extracts cross-domain behavior from the generic executor so that
/// `executor.rs` remains an orchestrator (sequencing, persistence, status transitions,
/// event emission) rather than a hub for every workflow family's follow-up logic.
use rusqlite::Connection;

use crate::engine::task_store;
use crate::engine::workflows::StepResult;
use crate::engine::workflows::{StepKind, WorkflowStep};
use crate::models::ctr::CtrOutcome;
use crate::models::task::Task;

// ─── Post-step context ───────────────────────────────────────────────────────

pub struct PostStepContext<'a> {
    pub conn: &'a Connection,
    pub task: &'a Task,
    pub step: &'a WorkflowStep,
    pub result: &'a StepResult,
    pub project_path: &'a str,
    pub agent_provider: &'a str,
}

/// Run domain-specific side effects after a single step completes.
/// Returns an optional override for the step's progress message/output.
pub fn after_step(ctx: &PostStepContext<'_>) -> StepOutcomeOverride {
    let mut override_out = StepOutcomeOverride::default();

    // ─── Reddit ──────────────────────────────────────────────────────────────

    if ctx.step.kind == StepKind::RedditSearch && ctx.result.success {
        if let Some(ref out) = ctx.result.output {
            crate::engine::exec::reddit::persist_reddit_opportunities(
                ctx.conn,
                &ctx.task.project_id,
                out,
            );
        }
    }

    if ctx.step.kind == StepKind::RedditEnrich {
        loop {
            let pending: i64 = ctx
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM reddit_opportunities \
                 WHERE project_id=?1 AND (why_relevant IS NULL OR reply_text IS NULL) \
                 AND reply_status != 'skipped'",
                    rusqlite::params![&ctx.task.project_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if pending == 0 {
                break;
            }
            log::info!(
                "[reddit_enrich] {} posts still pending enrichment — running batch",
                pending
            );
            crate::engine::exec::reddit::exec_reddit_enrich(
                ctx.conn,
                ctx.task,
                ctx.project_path,
                ctx.agent_provider,
            );
        }
        override_out.message = Some("Reddit enrichment complete".to_string());
    }

    if ctx.step.kind == StepKind::RedditFetchResults {
        let result =
            crate::engine::exec::reddit::exec_reddit_fetch_results(ctx.conn, &ctx.task.project_id);
        override_out.status = Some(if result.success {
            "ok".to_string()
        } else {
            "failed".to_string()
        });
        override_out.message = Some(result.message.clone());
        override_out.output = result.output.clone();
        override_out.artifact =
            result
                .output
                .clone()
                .map(|out| crate::models::task::TaskArtifact {
                    key: ctx.step.name.clone(),
                    path: None,
                    artifact_type: Some(ctx.step.kind.to_string()),
                    source: Some(ctx.step.kind.to_string()),
                    content: Some(out),
                });
    }

    // ─── Content write orphan ingestion ──────────────────────────────────────

    if ctx.step.name == "content_write_stage" && ctx.result.success {
        let project_path = std::path::Path::new(ctx.project_path);
        match crate::content::article_index::ingest_orphans(
            ctx.conn,
            &ctx.task.project_id,
            project_path,
        ) {
            Ok(ingested) if ingested.ingested > 0 => {
                let (keyword, kd_str, vol) = parse_content_task_keyword_meta(ctx.task);
                for filename in &ingested.files {
                    let _ = ctx.conn.execute(
                        "UPDATE articles
                         SET target_keyword=?1, keyword_difficulty=?2, target_volume=?3,
                             status='draft'
                         WHERE project_id=?4 AND file LIKE ?5",
                        rusqlite::params![
                            keyword.as_deref(),
                            kd_str.as_deref(),
                            vol,
                            &ctx.task.project_id,
                            format!("%{}", filename),
                        ],
                    );
                }
                let _ = crate::content::article_index::export_projection(
                    ctx.conn,
                    &ctx.task.project_id,
                    project_path,
                );
                log::info!(
                    "[content_register] registered {} article(s): {:?}",
                    ingested.ingested,
                    ingested.files
                );
            }
            Ok(_) => {
                log::info!("[content_register] no new orphan files to register after content write")
            }
            Err(e) => log::warn!("[content_register] article registration failed: {}", e),
        }
    }

    // ─── Date enforcement after any content-modifying step ───────────────────

    const CONTENT_WRITE_STEPS: &[&str] = &[
        "content_write_stage",
        "content_review_apply_execute",
        "fix_content_article_apply",
        "fix_ctr_article_apply",
        "sanitize_content_run",
        "content_cleanup_fix",
    ];

    if CONTENT_WRITE_STEPS.contains(&ctx.step.name.as_str()) && ctx.result.success {
        let project_path = std::path::Path::new(ctx.project_path);
        match crate::content::dates::enforce_safe_dates(
            ctx.conn,
            &ctx.task.project_id,
            project_path,
        ) {
            Ok(result) if result.articles_fixed > 0 => {
                log::info!(
                    "[date_enforce] Fixed {} article date(s) after {}",
                    result.articles_fixed,
                    ctx.step.name
                );
            }
            Ok(_) => {}
            Err(e) => log::warn!(
                "[date_enforce] Failed after {}: {}",
                ctx.step.name,
                e
            ),
        }
    }

    override_out
}

/// Optional overrides for step progress after domain post-processing.
#[derive(Default)]
pub struct StepOutcomeOverride {
    pub status: Option<String>,
    pub message: Option<String>,
    pub output: Option<String>,
    /// If set, the caller should append this artifact to the task.
    pub artifact: Option<crate::models::task::TaskArtifact>,
}

// ─── Post-task context ───────────────────────────────────────────────────────

pub struct PostTaskContext<'a> {
    pub conn: &'a Connection,
    pub task: &'a Task,
    pub project_path: &'a str,
    pub progress: &'a [crate::engine::executor::StepProgress],
}

/// Spawn follow-up tasks after a successful task completion.
/// Returns the IDs of any newly created tasks.
pub fn after_task_success(ctx: &PostTaskContext<'_>) -> Vec<String> {
    let mut follow_up_ids: Vec<String> = vec![];

    // Content review → individual fix_content_article tasks
    if matches!(
        ctx.task.task_type.as_str(),
        "content_review" | "content_audit"
    ) {
        let fix_task_ids = crate::engine::exec::content::create_content_review_apply_task(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        );
        follow_up_ids.extend(fix_task_ids);
    }

    // Write article (or hub page) → cluster_and_link task
    if matches!(
        ctx.task.task_type.as_str(),
        "write_article" | "create_hub_page" | "refresh_hub_page"
    ) {
        if let Some(task_id) = crate::engine::exec::content::create_cluster_and_link_task(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        ) {
            follow_up_ids.push(task_id);
        }
    }

    // Collect GSC → spawn fix tasks from collection artifact
    if ctx.task.task_type == "collect_gsc" {
        follow_up_ids.extend(
            crate::engine::exec::gsc::create_tasks_from_collection_after_exec(
                ctx.conn,
                ctx.task,
                ctx.project_path,
            ),
        );
    }

    // CTR audit → spawn CTR fix tasks
    if ctx.task.task_type == "ctr_audit" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            follow_up_ids.extend(crate::engine::exec::ctr_audit::create_ctr_fix_tasks(
                ctx.conn,
                &reloaded,
                ctx.project_path,
            ));
            if let Some(task_id) = crate::engine::exec::ctr_audit::create_ctr_site_template_task(
                ctx.conn,
                &reloaded,
                ctx.project_path,
            ) {
                follow_up_ids.push(task_id);
            }
        }
    }

    // Cannibalization audit → spawn cannibalization fix tasks
    if ctx.task.task_type == "cannibalization_audit" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            follow_up_ids.extend(
                crate::engine::exec::cannibalization_audit::create_can_fix_tasks(
                    ctx.conn,
                    &reloaded,
                    ctx.project_path,
                ),
            );
        }
    }

    // Territory research → spawn write_article tasks from content recommendations
    if ctx.task.task_type == "territory_research" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            follow_up_ids.extend(
                crate::engine::exec::territory_research::create_territory_write_tasks(
                    ctx.conn,
                    &reloaded,
                    ctx.project_path,
                ),
            );
        }
    }

    // Indexing diagnostics → collect spawned fix task IDs from step output
    if ctx.task.task_type == "indexing_diagnostics" {
        if let Some(step_result) = ctx
            .progress
            .iter()
            .find(|p| p.step_name == "indexing_diagnostics_run")
        {
            if let Some(ref out) = step_result.output {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(out) {
                    if let Some(ids) = val.get("spawned_task_ids").and_then(|v| v.as_array()) {
                        for id in ids {
                            if let Some(s) = id.as_str() {
                                follow_up_ids.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // CTR fix tasks → record baseline outcome + spawn outcome review task
    if ctx.task.task_type == "fix_ctr_article" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            if reloaded.status == crate::models::task::TaskStatus::Done {
                // Record baseline metrics so the review task has data to compare
                if let Err(e) = record_ctr_outcome_baseline(ctx.conn, &reloaded, ctx.project_path) {
                    log::warn!(
                        "[post_actions] Failed to record CTR outcome baseline for {}: {}",
                        reloaded.id,
                        e
                    );
                }

                let idempotency_key =
                    format!("ctr_outcome_review:{}:{}", reloaded.project_id, reloaded.id);
                let spec = crate::engine::spawner::TaskSpec {
                    project_id: reloaded.project_id.clone(),
                    task_type: "ctr_outcome_review".to_string(),
                    title: Some(format!("CTR outcome review: {}", reloaded.id)),
                    description: Some(format!(
                        "Review CTR outcomes for fix task {}. Will wait 14 days post-deployment before comparing metrics.",
                        reloaded.id
                    )),
                    priority: crate::models::task::Priority::Medium,
                    run_policy: Some(crate::models::task::TaskRunPolicy::UserEnqueue),
        agent_policy: crate::models::task::AgentPolicy::None,
                    depends_on: vec![reloaded.id.clone()],
                    artifacts: vec![],
                    idempotency_key: Some(idempotency_key),
                    ..Default::default()
                };
                if let Ok(task) = crate::engine::spawner::TaskSpawner::spawn(ctx.conn, spec) {
                    follow_up_ids.push(task.id);
                }
            }
        }
    }

    // GSC fix tasks → record resolution
    if matches!(
        ctx.task.task_type.as_str(),
        "fix_indexing" | "fix_technical" | "fix_content" | "fix_gsc_access"
    ) {
        if let Some(ref desc) = ctx.task.description {
            if let Some(url_line) = desc.lines().next() {
                if let Some(url) = url_line.strip_prefix("URL: ") {
                    let fix_summary = ctx
                        .progress
                        .iter()
                        .find(|p| p.step_name == "indexing_fix_apply")
                        .and_then(|p| p.output.as_ref())
                        .map(|o| crate::engine::text::char_prefix(o, 500).to_string())
                        .unwrap_or_else(|| "Fix applied (no summary available)".to_string());

                    let _ = crate::gsc::db::record_fix_resolved(
                        ctx.conn,
                        url,
                        &ctx.task.project_id,
                        &fix_summary,
                    );
                }
            }
        }
    }

    // Content article fix → mark reviewed
    if ctx.task.task_type == "fix_content_article" {
        if let Err(e) = crate::engine::exec::content::mark_fix_content_article_reviewed(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        ) {
            log::warn!(
                "[content_review] failed to persist article review completion: {}",
                e
            );
        }
    }

    follow_up_ids
}

// ─── CTR Outcome Baseline Recording ──────────────────────────────────────────

/// Record a baseline CtrOutcome when a fix_ctr_article task completes.
///
/// Reads the article ID from the task's ctr_context artifact, looks up the
/// article's URL from ctr_rendered_page_audits, fetches current GSC metrics
/// from live_site_pages (or article_metadata for workspace projects), and
/// inserts a baseline record into ctr_outcomes.
/// This gives the subsequent ctr_outcome_review task data to compare against.
fn record_ctr_outcome_baseline(
    conn: &Connection,
    task: &Task,
    _project_path: &str,
) -> crate::error::Result<()> {
    // Extract article_id from ctr_context artifact
    let article_id = task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_context")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v.get("articles")?.as_array()?.first()?.get("id")?.as_i64())
        .ok_or_else(|| {
            crate::error::Error::Other("No article_id in ctr_context artifact".to_string())
        })?;

    // Look up URL from rendered page audits
    let url: String = conn
        .query_row(
            "SELECT url FROM ctr_rendered_page_audits WHERE project_id = ?1 AND article_id = ?2",
            rusqlite::params![&task.project_id, article_id],
            |row| row.get(0),
        )
        .map_err(|_| {
            crate::error::Error::Other(format!("No rendered audit URL for article {}", article_id))
        })?;

    // Fetch current GSC metrics as baseline.
    // For live-site projects: read from live_site_pages.
    // For workspace projects: fall back to article_metadata (namespace='gsc').
    let (baseline_clicks, baseline_impressions, baseline_ctr, baseline_position) =
        fetch_baseline_metrics(conn, &task.project_id, article_id, &url);

    let now = chrono::Utc::now();
    let baseline_start = (now - chrono::Duration::days(28)).to_rfc3339();
    let baseline_end = now.to_rfc3339();

    let outcome = CtrOutcome {
        project_id: task.project_id.clone(),
        article_id,
        fix_task_id: task.id.clone(),
        baseline_start,
        baseline_end,
        after_start: None,
        after_end: None,
        baseline_clicks,
        baseline_impressions,
        baseline_ctr,
        baseline_position,
        after_clicks: None,
        after_impressions: None,
        after_ctr: None,
        after_position: None,
        position_delta: None,
        outcome_status: "pending".to_string(),
        deployed_at: Some(now.to_rfc3339()),
        reviewed_at: None,
    };

    crate::db::set_ctr_outcome(conn, &outcome)?;

    log::info!(
        "[ctr_outcome] Recorded baseline for article {} (task {}): clicks={:.1}, impressions={:.1}, ctr={:.4}, position={:.1}",
        article_id, task.id, baseline_clicks, baseline_impressions, baseline_ctr, baseline_position
    );

    Ok(())
}

/// Try live_site_pages first, then article_metadata (workspace projects),
/// then return zeros.
fn fetch_baseline_metrics(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    url: &str,
) -> (f64, f64, f64, f64) {
    // 1. Live-site path lookup
    let path = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let path = if let Some(pos) = path.find('/') {
        &path[pos..]
    } else {
        "/"
    };

    let live_site_result: Result<
        (Option<f64>, Option<f64>, Option<f64>, Option<f64>),
        rusqlite::Error,
    > = conn.query_row(
        "SELECT gsc_clicks, gsc_impressions, gsc_ctr, gsc_position
             FROM live_site_pages
             WHERE project_id = ?1 AND path = ?2",
        rusqlite::params![project_id, path],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    );

    if let Ok((Some(clicks), Some(impressions), Some(ctr), Some(position))) = live_site_result {
        return (clicks, impressions, ctr, position);
    }

    // 2. Workspace fallback: article_metadata namespace='gsc'
    let meta_result: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT payload FROM article_metadata WHERE project_id = ?1 AND article_id = ?2 AND namespace = 'gsc'",
        rusqlite::params![project_id, article_id],
        |row| row.get(0),
    );

    if let Ok(payload) = meta_result {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&payload) {
            let clicks = val["clicks"].as_f64().unwrap_or(0.0);
            let impressions = val["impressions"].as_f64().unwrap_or(0.0);
            let ctr = val["ctr"].as_f64().unwrap_or(0.0);
            let position = val["avg_position"].as_f64().unwrap_or(0.0);
            if clicks > 0.0 || impressions > 0.0 {
                return (clicks, impressions, ctr, position);
            }
        }
    }

    // 3. Nothing available
    (0.0, 0.0, 0.0, 0.0)
}

// ─── Helpers (copied from executor.rs to avoid cross-module privacy issues) ──

/// Parse keyword metadata embedded in the write_article task description.
fn parse_content_task_keyword_meta(task: &Task) -> (Option<String>, Option<String>, i64) {
    let desc = match task.description.as_deref() {
        Some(d) if !d.is_empty() => d,
        _ => return (None, None, 0),
    };
    let mut keyword = None;
    let mut kd: Option<String> = None;
    let mut volume = 0i64;
    for line in desc.lines() {
        if let Some(rest) = line.strip_prefix("Target keyword:") {
            keyword = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("KD:") {
            if let Ok(n) = rest.trim().parse::<i64>() {
                kd = Some(n.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("Volume:") {
            volume = rest.trim().parse::<i64>().unwrap_or(0);
        }
    }
    (keyword, kd, volume)
}
