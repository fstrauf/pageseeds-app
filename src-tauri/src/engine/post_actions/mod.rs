/// Domain-specific post-step and post-task side effects.
///
/// This module extracts cross-domain behavior from the generic executor so that
/// `executor.rs` remains an orchestrator (sequencing, persistence, status transitions,
/// event emission) rather than a hub for every workflow family's follow-up logic.
use rusqlite::Connection;

use crate::engine::task_store;
use crate::engine::workflows::StepResult;
use crate::engine::workflows::{StepKind, WorkflowStep};
use crate::models::task::{Task, TaskStatus};

mod ctr_outcome;
mod keyword_meta;
mod outcome_review;
mod topic_health;
#[cfg(test)]
mod tests;

pub(crate) use keyword_meta::{
    content_task_target_keyword, parse_content_task_keyword_meta, strip_content_task_title_prefix,
};

use ctr_outcome::record_ctr_outcome_baseline;
use keyword_meta::find_expected_slug_and_file;
use outcome_review::spawn_content_outcome_review;
use topic_health::{find_written_article_file, run_topic_health_reducer};
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
            match crate::engine::exec::reddit::persist_reddit_opportunities(
                ctx.conn,
                &ctx.task.project_id,
                out,
            ) {
                Ok(outcome) => {
                    // Success-with-zero-of-N must be impossible (issue #71): when
                    // upserts fail for every parsed post, the picker would come up
                    // empty. Fail the step with the underlying DB error so the
                    // drift surfaces instead of silently wiping the feed. Pure
                    // dedup (already posted/skipped rows counted in `skipped`)
                    // must not fail the step.
                    if outcome.db_failures > 0 && outcome.upserted == 0 {
                        let detail = outcome
                            .errors
                            .unwrap_or_else(|| "unknown DB error".to_string());
                        override_out.success = Some(false);
                        override_out.status = Some("failed".to_string());
                        override_out.message = Some(format!(
                            "Persisted 0 of {} Reddit opportunities: {}",
                            outcome.parsed, detail
                        ));
                    }
                }
                Err(e) => {
                    override_out.success = Some(false);
                    override_out.status = Some("failed".to_string());
                    override_out.message =
                        Some(format!("Failed to persist Reddit opportunities: {}", e));
                }
            }
        }
    }

    if ctx.step.kind == StepKind::RedditEnrich {
        let mut last_pending = -1i64;
        loop {
            let pending: i64 = ctx
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM reddit_opportunities \
                 WHERE project_id=?1 AND (why_relevant IS NULL OR reply_text IS NULL) \
                 AND reply_status = 'pending'",
                    rusqlite::params![&ctx.task.project_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if pending == 0 {
                break;
            }
            if pending == last_pending {
                log::warn!(
                    "[reddit_enrich] no progress in last batch ({} posts still pending) — breaking to avoid infinite loop",
                    pending
                );
                break;
            }
            last_pending = pending;
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
        match ingest_content_write_files(ctx.conn, ctx.task, project_path) {
            Ok(summary) if summary.ingested > 0 => {}
            Ok(_) => log::warn!(
                "[content_register] no new orphan files registered after content write — \
                 content_write_verify will fail the task if no article file was written"
            ),
            Err(e) => log::warn!("[content_register] article registration failed: {}", e),
        }
    }

    // ─── Date enforcement after any content-modifying step ───────────────────

    const CONTENT_WRITE_STEPS: &[&str] = &[
        "content_write_stage",
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
            Err(e) => log::warn!("[date_enforce] Failed after {}: {}", ctx.step.name, e),
        }

        // ─── Slug guard: prevent agent from changing frontmatter slug ──────────
        if let Some((expected, file_path)) = find_expected_slug_and_file(ctx) {
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                if let Some((fm_text, body)) = crate::content::frontmatter::split_mdx(&content) {
                    let actual = crate::content::frontmatter::parse(fm_text)
                        .ok()
                        .and_then(|fm| fm.parsed["slug"].as_str().map(String::from));
                    if let Some(actual) = actual {
                        let actual_clean = actual.trim().trim_matches('"').trim_matches('\'');
                        let expected_clean = expected.trim().trim_matches('"').trim_matches('\'');
                        if !actual_clean.is_empty() && actual_clean != expected_clean {
                            log::warn!(
                                "[slug_guard] Agent changed slug from '{}' to '{}' in {}. Restoring...",
                                expected_clean,
                                actual_clean,
                                file_path.display()
                            );
                            let new_fm = crate::content::frontmatter::replace_scalar(fm_text, "slug", expected_clean);
                            let new_content = crate::content::cleaner::rebuild_mdx(&new_fm, body);
                            if let Err(e) = std::fs::write(&file_path, new_content) {
                                log::warn!("[slug_guard] Failed to restore slug in {}: {}", file_path.display(), e);
                            } else {
                                log::info!("[slug_guard] Restored slug to '{}' in {}", expected_clean, file_path.display());
                                override_out.status = Some("failed".to_string());
                                override_out.message = Some(format!(
                                    "Agent attempted to change slug from '{}' to '{}'. Slug restored. Review agent output to ensure other changes are still valid.",
                                    expected_clean, actual_clean
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    override_out
}

/// Optional overrides for step progress after domain post-processing.
#[derive(Default)]
pub struct StepOutcomeOverride {
    /// If set, overrides the step's success flag (e.g. a post-step persistence
    /// check failed even though the step itself ran fine).
    pub success: Option<bool>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub output: Option<String>,
    /// If set, the caller should append this artifact to the task.
    pub artifact: Option<crate::models::task::TaskArtifact>,
}

/// Ingest new content files after a content-write step, tag them with the
/// task's keyword metadata, mark them as drafts, and export the articles.json
/// projection.
///
/// Shared by `after_step` (post-write registration) and the deterministic
/// `content_write_verify` step (idempotent safety net). Returns the ingest
/// summary so callers can distinguish "registered N new files" from "nothing
/// new on disk".
pub(crate) fn ingest_content_write_files(
    conn: &Connection,
    task: &Task,
    project_path: &std::path::Path,
) -> Result<crate::content::article_index::IngestSummary, String> {
    let ingested =
        crate::content::article_index::ingest_orphans(conn, &task.project_id, project_path)
            .map_err(|e| e.to_string())?;

    if ingested.ingested > 0 {
        let (keyword, kd_str, vol) = parse_content_task_keyword_meta(task);
        for filename in &ingested.files {
            let _ = conn.execute(
                "UPDATE articles
                 SET target_keyword=?1, keyword_difficulty=?2, target_volume=?3,
                     status='draft'
                 WHERE project_id=?4 AND file LIKE ?5",
                rusqlite::params![
                    keyword.as_deref(),
                    kd_str.as_deref(),
                    vol,
                    &task.project_id,
                    format!("%{}", filename),
                ],
            );
        }
        let _ =
            crate::content::article_index::export_projection(conn, &task.project_id, project_path);
        log::info!(
            "[content_register] registered {} article(s): {:?}",
            ingested.ingested,
            ingested.files
        );
    }

    Ok(ingested)
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

    // Content review → individual fix_content_article tasks + topic health reducer
    if matches!(
        ctx.task.task_type.as_str(),
        "content_review" | "content_audit"
    ) {
        let fix_task_ids = crate::engine::exec::content::create_fix_content_article_tasks(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        );
        follow_up_ids.extend(fix_task_ids);

        if let Err(e) = run_topic_health_reducer(ctx) {
            log::warn!("[post_actions] topic health reducer failed: {}", e);
        }
    }

    // Write article (or hub page, or landing page) → quality review → cluster_and_link task
    if matches!(
        ctx.task.task_type.as_str(),
        "write_article" | "create_hub_page" | "refresh_hub_page" | "create_landing_page"
    ) {
        if let Some(article_file) = find_written_article_file(ctx) {
            if let Some(task_id) = crate::engine::exec::content::create_review_article_quality_task(
                ctx.conn,
                ctx.task,
                ctx.project_path,
                &article_file,
            ) {
                follow_up_ids.push(task_id);
            }
        }
        if let Some(task_id) = crate::engine::exec::content::create_cluster_and_link_task(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        ) {
            follow_up_ids.push(task_id);
        }
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
                crate::engine::exec::cannibalization::create_can_fix_tasks(
                    ctx.conn,
                    &reloaded,
                    ctx.project_path,
                ),
            );
        }
    }

    // Indexing health campaign → spawn child fix tasks from campaign plan
    if ctx.task.task_type == "indexing_health_campaign" {
        let spawned = crate::engine::exec::indexing_health::spawn_campaign_children(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        );
        follow_up_ids.extend(spawned);
    }

    // cluster_and_link / interlinking → spawn follow-up if orphans remain
    if matches!(
        ctx.task.task_type.as_str(),
        "cluster_and_link" | "interlinking"
    ) {
        let apply_artifact = ctx
            .task
            .artifacts
            .iter()
            .find(|a| a.key == "cluster_link_apply");
        if let Some(artifact) = apply_artifact {
            if let Some(content) = artifact.content.as_deref() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                    let orphans = val["orphans_remaining"].as_i64().unwrap_or(0);
                    let links_added = val["links_added"].as_i64().unwrap_or(0);
                    // Cap follow-ups at 3 rounds total to avoid infinite loops
                    let current_round = ctx
                        .task
                        .title
                        .as_deref()
                        .and_then(|t| {
                            let re = regex::Regex::new(r"round\s+(\d+)").ok()?;
                            re.captures(t)?.get(1)?.as_str().parse::<u32>().ok()
                        })
                        .unwrap_or(1);
                    if current_round >= 3 {
                        log::info!("[post_actions] cluster_and_link reached round {} — stopping follow-up chain", current_round);
                    } else if orphans > 0 && links_added > 0 {
                        let next_round = current_round + 1;
                        log::info!(
                            "[post_actions] cluster_and_link round {} finished with {} orphans remaining and {} links added — spawning round {}",
                            current_round, orphans, links_added, next_round
                        );
                        if let Ok(task) = task_store::get_task(ctx.conn, &ctx.task.id) {
                            let title = format!(
                                "Cluster and link: round {} ({} orphans remain)",
                                next_round, orphans
                            );
                            if let Ok(Some(follow_up)) =
                                crate::engine::spawner::TaskSpawner::spawn_follow_up(
                                    ctx.conn,
                                    &task,
                                    &ctx.task.task_type,
                                    &title,
                                )
                            {
                                follow_up_ids.push(follow_up.id);
                            }
                        }
                    } else {
                        log::info!("[post_actions] cluster_and_link round {} finished with {} orphans remaining, {} links added — no follow-up needed", current_round, orphans, links_added);
                    }
                }
            }
        }
    }

    // Indexing diagnostics → collect spawned fix task IDs from task artifact
    // (step output is truncated at 4,000 chars, so we store the full result as an artifact)
    if ctx.task.task_type == "indexing_diagnostics" {
        if let Ok(reloaded) = task_store::get_task(ctx.conn, &ctx.task.id) {
            if let Some(artifact) = reloaded
                .artifacts
                .iter()
                .find(|a| a.key == "indexing_diagnostics_result")
            {
                if let Some(ref content) = artifact.content {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
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

    // write_article / fix_content_article / consolidate_cluster → +30d outcome review (issue #23)
    if matches!(
        ctx.task.task_type.as_str(),
        "write_article" | "fix_content_article" | "consolidate_cluster"
    ) {
        if let Some(task_id) = spawn_content_outcome_review(ctx) {
            follow_up_ids.push(task_id);
        }
    }

    // GSC indexing recovery → spawn focused child tasks
    if ctx.task.task_type == "gsc_indexing_recovery" {
        let child_ids = crate::engine::exec::gsc::spawn_recovery_child_tasks(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        );
        follow_up_ids.extend(child_ids);
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

    // fix_indexing_internal_links → update history + spawn delayed outcome review
    if ctx.task.task_type == "fix_indexing_internal_links" {
        // Parse verify step output for incoming count and links added
        let verify_data = ctx
            .progress
            .iter()
            .find(|p| p.kind == "indexing_link_verify")
            .and_then(|p| p.output.as_ref())
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok());

        let incoming_after = verify_data
            .as_ref()
            .and_then(|v| v["incoming_link_count_after"].as_i64())
            .unwrap_or(-1);
        let links_added = verify_data
            .as_ref()
            .and_then(|v| v["links_added"].as_i64())
            .unwrap_or(0);

        // Update recovery history
        let _ = crate::gsc::db::update_recovery_history_on_complete(
            ctx.conn,
            &ctx.task.id,
            incoming_after,
            links_added,
        );

        // Spawn delayed outcome review with not_before = 14 days
        let not_before = (chrono::Utc::now() + chrono::Duration::days(14)).to_rfc3339();
        let idempotency_key = format!(
            "gsc-indexing-outcome-review:{}:{}",
            ctx.task.project_id, ctx.task.id
        );

        let spec = crate::engine::spawner::TaskSpec {
            project_id: ctx.task.project_id.clone(),
            task_type: "gsc_indexing_outcome_review".to_string(),
            title: Some(format!("GSC outcome review for {}", ctx.task.id)),
            description: Some(format!(
                "Re-inspect target URL in GSC 14 days after link fix. Parent: {}",
                ctx.task.id
            )),
            priority: crate::models::task::Priority::Medium,
            run_policy: Some(crate::models::task::TaskRunPolicy::UserEnqueue),
            agent_policy: crate::models::task::AgentPolicy::None,
            depends_on: vec![ctx.task.id.clone()],
            artifacts: ctx.task.artifacts.clone(),
            idempotency_key: Some(idempotency_key),
            not_before: Some(not_before),
            ..Default::default()
        };

        if let Ok(task) = crate::engine::spawner::TaskSpawner::spawn(ctx.conn, spec) {
            follow_up_ids.push(task.id);
        }
    }

    // gsc_indexing_outcome_review → update history with final outcome
    if ctx.task.task_type == "gsc_indexing_outcome_review" {
        let outcome = ctx
            .progress
            .iter()
            .find(|p| p.kind == "gsc_indexing_outcome_report")
            .and_then(|p| p.output.as_ref())
            .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
            .and_then(|v| v["outcome_status"].as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string());

        // Find the parent fix task ID from depends_on
        let parent_task_id = ctx.task.depends_on.first().cloned().unwrap_or_default();
        if !parent_task_id.is_empty() {
            let _ = crate::gsc::db::update_recovery_history_outcome(
                ctx.conn,
                &parent_task_id,
                &outcome,
            );
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

    // Spawn agentic feature spec generation after audit tasks complete
    if matches!(
        ctx.task.task_type.as_str(),
        "content_review" | "content_audit" | "ctr_audit" | "indexing_health_campaign"
    ) {
        // Deduplicate by project + month so only one spec is generated per month
        // regardless of how many audit types run.
        let month = chrono::Utc::now().format("%Y%m").to_string();
        let idempotency_key = format!(
            "feature_spec:{}:{}",
            ctx.task.project_id,
            month
        );
        let spec = crate::engine::spawner::TaskSpec {
            project_id: ctx.task.project_id.clone(),
            task_type: "generate_feature_spec".to_string(),
            title: Some("Feature spec from audit findings".to_string()),
            description: Some(
                "Agentic synthesis of SEO audit findings into a developer feature specification."
                    .to_string(),
            ),
            priority: crate::models::task::Priority::Medium,
            run_policy: Some(crate::models::task::TaskRunPolicy::AutoEnqueue),
            agent_policy: crate::models::task::AgentPolicy::Required,
            depends_on: vec![ctx.task.id.clone()],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::Cooldown { days: 30 }),
            ..Default::default()
        };
        match crate::engine::spawner::TaskSpawner::spawn(ctx.conn, spec) {
            Ok(task) => {
                log::info!(
                    "[post_actions] Spawned feature spec task {} for {}",
                    task.id,
                    ctx.task.id
                );
                follow_up_ids.push(task.id);
            }
            Err(e) => {
                log::warn!("[post_actions] Failed to spawn feature spec task: {}", e);
            }
        }
    }

    // Retry blocked indexing_health_campaign tasks when their prerequisites complete.
    // The IHC step 1 fails when gsc_collection.json / link_scan.json / content_audit.json
    // are stale, auto-spawns helper tasks, and marks itself failed. After a helper
    // finishes, re-enqueue any blocked IHC task so it can run to completion.
    retry_blocked_ihc_tasks(ctx, &mut follow_up_ids);

    follow_up_ids
}

/// Run domain-specific side effects after a task completes with one or more
/// failed steps (`all_ok == false`). Symmetric counterpart to
/// [`after_task_success`]; the executor calls it from the failure branch.
pub fn after_task_failure(ctx: &PostTaskContext<'_>) {
    // A failed fix_content_article (soft-failed verification lands the task in
    // Review, so after_task_success never runs) must release the article's
    // in_review flag — otherwise the article is permanently excluded from
    // select_priority_articles and silently leaves the review pipeline.
    if ctx.task.task_type == "fix_content_article" {
        if let Err(e) = crate::engine::exec::content::release_fix_content_article_in_review(
            ctx.conn,
            ctx.task,
            ctx.project_path,
        ) {
            log::warn!(
                "[content_review] failed to release article in_review after fix failure: {}",
                e
            );
        }
    }
}

/// Cancel a task and run domain-specific cancel side effects.
///
/// Cancellation shares its side effects with [`after_task_failure`] (a
/// cancelled fix_content_article releases the article's in_review flag), so
/// future failure-cleanup logic automatically applies to cancellations too.
pub fn cancel_task(conn: &Connection, task_id: &str) -> crate::error::Result<Task> {
    let task = task_store::update_task_status(conn, task_id, TaskStatus::Cancelled)?;

    if let Ok(project) = task_store::get_project(conn, &task.project_id) {
        after_task_failure(&PostTaskContext {
            conn,
            task: &task,
            project_path: &project.path,
            progress: &[],
        });
    }

    Ok(task)
}

/// When a helper task (collect_gsc, cluster_and_link, content_audit) completes,
/// find any failed indexing_health_campaign tasks in the same project that were
/// blocked on prerequisites and re-enqueue them.
fn retry_blocked_ihc_tasks(ctx: &PostTaskContext<'_>, follow_up_ids: &mut Vec<String>) {
    // Only trigger retry when one of the known IHC prerequisite task types completes.
    if !matches!(
        ctx.task.task_type.as_str(),
        "collect_gsc" | "cluster_and_link" | "content_audit"
    ) {
        return;
    }

    let tasks = match task_store::list_tasks(ctx.conn, &ctx.task.project_id) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[post_actions] IHC retry: failed to list tasks: {}", e);
            return;
        }
    };

    for task in &tasks {
        if task.task_type != "indexing_health_campaign" {
            continue;
        }
        if task.status != crate::models::task::TaskStatus::Failed {
            continue;
        }
        // Only retry if it failed on the prerequisite check step.
        let last_error = task.run.last_error.as_deref().unwrap_or("");
        if !last_error.contains("Waiting for") {
            continue;
        }

        log::info!(
            "[post_actions] Retrying blocked IHC task {} (was waiting for prerequisites)",
            task.id
        );

        // Clear the error and re-enqueue.
        if let Err(e) = task_store::reset_task_error(ctx.conn, &task.id) {
            log::warn!("[post_actions] IHC retry: failed to clear error for {}: {}", task.id, e);
            continue;
        }
        if let Err(e) = task_store::update_task_status(
            ctx.conn,
            &task.id,
            crate::models::task::TaskStatus::Todo,
        ) {
            log::warn!("[post_actions] IHC retry: failed to update status for {}: {}", task.id, e);
            continue;
        }

        let item = crate::models::queue::EnqueueItem {
            task_id: task.id.clone(),
            project_id: task.project_id.clone(),
            title: task.title.clone(),
            task_type: Some(task.task_type.clone()),
            project_name: None,
        };
        if let Err(e) = crate::engine::queue::enqueue_tasks(
            ctx.conn,
            vec![item],
            crate::models::queue::EnqueueMode::Append,
        ) {
            log::warn!("[post_actions] IHC retry: failed to enqueue {}: {}", task.id, e);
            continue;
        }

        follow_up_ids.push(task.id.clone());
    }
}
