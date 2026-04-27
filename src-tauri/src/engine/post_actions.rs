/// Domain-specific post-step and post-task side effects.
///
/// This module extracts cross-domain behavior from the generic executor so that
/// `executor.rs` remains an orchestrator (sequencing, persistence, status transitions,
/// event emission) rather than a hub for every workflow family's follow-up logic.

use rusqlite::Connection;

use crate::engine::task_store;
use crate::engine::workflows::{step_params, StepKind, WorkflowStep};
use crate::engine::workflows::StepResult;
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
            crate::engine::exec::reddit::persist_reddit_opportunities(ctx.conn, &ctx.task.project_id, out);
        }
    }

    if ctx.step.kind == StepKind::RedditEnrich {
        loop {
            let pending: i64 = ctx.conn.query_row(
                "SELECT COUNT(*) FROM reddit_opportunities \
                 WHERE project_id=?1 AND (why_relevant IS NULL OR reply_text IS NULL) \
                 AND reply_status != 'skipped'",
                rusqlite::params![&ctx.task.project_id],
                |r| r.get(0),
            ).unwrap_or(0);
            if pending == 0 { break; }
            log::info!("[reddit_enrich] {} posts still pending enrichment — running batch", pending);
            crate::engine::exec::reddit::exec_reddit_enrich(ctx.conn, ctx.task, ctx.project_path, ctx.agent_provider);
        }
        override_out.message = Some("Reddit enrichment complete".to_string());
    }

    if ctx.step.kind == StepKind::RedditFetchResults {
        let result = crate::engine::exec::reddit::exec_reddit_fetch_results(ctx.conn, &ctx.task.project_id);
        override_out.status = Some(if result.success { "ok".to_string() } else { "failed".to_string() });
        override_out.message = Some(result.message.clone());
        override_out.output = result.output.clone();
        override_out.artifact = result.output.clone().map(|out| crate::models::task::TaskArtifact {
            key: ctx.step.name.clone(),
            path: None,
            artifact_type: Some(ctx.step.kind.to_string()),
            source: Some(ctx.step.kind.to_string()),
            content: Some(out),
        });
    }

    // ─── Content write orphan ingestion ──────────────────────────────────────

    if ctx.step.name == "content_write_stage" && ctx.result.success {
        let automation_dir = std::path::Path::new(ctx.project_path)
            .join(".github")
            .join("automation");
        match crate::content::ops::ingest_orphan_files(
            &automation_dir,
            std::path::Path::new(ctx.project_path),
            &ctx.task.project_id,
            ctx.conn,
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
                            format!("%{}" , filename),
                        ],
                    );
                }
                if let Ok(json) = crate::db::export::export_articles(ctx.conn, &ctx.task.project_id) {
                    let articles_path = automation_dir.join("articles.json");
                    let _ = std::fs::write(&articles_path, json);
                }
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
    if matches!(ctx.task.task_type.as_str(), "content_review" | "content_audit") {
        let fix_task_ids = crate::engine::exec::content::create_content_review_apply_task(
            ctx.conn, ctx.task, ctx.project_path,
        );
        follow_up_ids.extend(fix_task_ids);
    }

    // Write article → cluster_and_link task
    if ctx.task.task_type == "write_article" {
        if let Some(task_id) = crate::engine::exec::content::create_cluster_and_link_task(ctx.conn, ctx.task, ctx.project_path) {
            follow_up_ids.push(task_id);
        }
    }

    // Collect GSC → spawn fix tasks from collection artifact
    if ctx.task.task_type == "collect_gsc" {
        follow_up_ids.extend(crate::engine::exec::gsc::create_tasks_from_collection_after_exec(
            ctx.conn, ctx.task, ctx.project_path,
        ));
    }

    // CTR audit → spawn CTR fix tasks
    if ctx.task.task_type == "ctr_audit" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            follow_up_ids.extend(crate::engine::exec::ctr_audit::create_ctr_fix_tasks(
                ctx.conn, &reloaded, ctx.project_path,
            ));
        }
    }

    // Cannibalization audit → spawn cannibalization fix tasks
    if ctx.task.task_type == "cannibalization_audit" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            follow_up_ids.extend(crate::engine::exec::cannibalization_audit::create_can_fix_tasks(
                ctx.conn, &reloaded, ctx.project_path,
            ));
        }
    }

    // Indexing diagnostics → collect spawned fix task IDs from step output
    if ctx.task.task_type == "indexing_diagnostics" {
        if let Some(step_result) = ctx.progress.iter().find(|p| p.step_name == "indexing_diagnostics_run") {
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

    // GSC fix tasks → record resolution
    if matches!(ctx.task.task_type.as_str(), "fix_indexing" | "fix_technical" | "fix_content" | "fix_gsc_access") {
        if let Some(ref desc) = ctx.task.description {
            if let Some(url_line) = desc.lines().next() {
                if let Some(url) = url_line.strip_prefix("URL: ") {
                    let fix_summary = ctx.progress
                        .iter()
                        .find(|p| p.step_name == "indexing_fix_apply")
                        .and_then(|p| p.output.as_ref())
                        .map(|o| crate::engine::text::char_prefix(o, 500).to_string())
                        .unwrap_or_else(|| "Fix applied (no summary available)".to_string());

                    let _ = crate::gsc::db::record_fix_resolved(ctx.conn, url, &ctx.task.project_id, &fix_summary);
                }
            }
        }
    }

    // Content article fix → mark reviewed
    if ctx.task.task_type == "fix_content_article" {
        if let Err(e) = crate::engine::exec::content::mark_fix_content_article_reviewed(ctx.conn, ctx.task, ctx.project_path) {
            log::warn!("[content_review] failed to persist article review completion: {}", e);
        }
    }

    follow_up_ids
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
