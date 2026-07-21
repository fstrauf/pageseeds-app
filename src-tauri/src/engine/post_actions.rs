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
use crate::models::task::{Task, TaskStatus};

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
    let url = crate::engine::exec::ctr_audit::lookup_rendered_audit_url(
        conn,
        &task.project_id,
        article_id,
    )
    .ok_or_else(|| {
        crate::error::Error::Other(format!("No rendered audit URL for article {}", article_id))
    })?;

    // Fetch current GSC metrics as baseline.
    // For live-site projects: read from live_site_pages.
    // For workspace projects: fall back to article_metadata (namespace='gsc').
    // Shared with the after-metrics fetcher in ctr_audit::outcome so both
    // sides of the comparison read from the same source.
    let (baseline_clicks, baseline_impressions, baseline_ctr, baseline_position) =
        crate::engine::exec::ctr_audit::fetch_article_gsc_metrics(
            conn,
            &task.project_id,
            article_id,
            &url,
        );

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

// ─── Content Outcome Review Spawning (issue #23) ─────────────────────────────

/// Days after a content change before its outcome is reviewed. Uniform 30d
/// across write_article / fix_content_article / consolidate_cluster (decision
/// recorded in issue #23).
const CONTENT_OUTCOME_REVIEW_DELAY_DAYS: i64 = 30;

/// Spawn a +30d `content_outcome_review` follow-up after a successful
/// write_article / fix_content_article / consolidate_cluster task.
///
/// Carries the article slug and a baseline snapshot of clicks/impressions/
/// position (from the article's stored GSC metadata; empty baseline is fine
/// for brand-new articles) as the `content_outcome_target` artifact consumed
/// by `exec::outcome_review::exec_content_outcome_compare`.
/// Returns the spawned task ID, or None when no slug could be resolved.
fn spawn_content_outcome_review(ctx: &PostTaskContext<'_>) -> Option<String> {
    let slug = outcome_review_slug(ctx)?;
    if slug.is_empty() {
        return None;
    }

    let baseline = outcome_baseline_metrics(ctx.conn, &ctx.task.project_id, &slug);
    let anchor_date = chrono::Utc::now().to_rfc3339();

    let target_artifact = crate::models::task::TaskArtifact {
        key: "content_outcome_target".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("post_actions".to_string()),
        content: Some(
            serde_json::json!({
                "slug": slug,
                "parent_task_type": ctx.task.task_type,
                "parent_task_id": ctx.task.id,
                "anchor_date": anchor_date,
                "baseline": {
                    "clicks": baseline.0,
                    "impressions": baseline.1,
                    "position": baseline.2,
                    "source": baseline.3,
                },
            })
            .to_string(),
        ),
    };

    let idempotency_key = format!(
        "content_outcome_review:{}:{}:{}",
        ctx.task.project_id, ctx.task.id, slug
    );
    let not_before = (chrono::Utc::now()
        + chrono::Duration::days(CONTENT_OUTCOME_REVIEW_DELAY_DAYS))
    .to_rfc3339();
    let spec = crate::engine::spawner::TaskSpec {
        project_id: ctx.task.project_id.clone(),
        task_type: "content_outcome_review".to_string(),
        title: Some(format!("Content outcome review: {}", slug)),
        description: Some(format!(
            "Compare GSC snapshot windows for '{}' {} days after {} (parent: {}).",
            slug, CONTENT_OUTCOME_REVIEW_DELAY_DAYS, ctx.task.task_type, ctx.task.id
        )),
        priority: crate::models::task::Priority::Medium,
        run_policy: Some(crate::models::task::TaskRunPolicy::UserEnqueue),
        agent_policy: crate::models::task::AgentPolicy::None,
        depends_on: vec![ctx.task.id.clone()],
        artifacts: vec![target_artifact],
        idempotency_key: Some(idempotency_key),
        not_before: Some(not_before),
        ..Default::default()
    };

    match crate::engine::spawner::TaskSpawner::spawn(ctx.conn, spec) {
        Ok(task) => {
            log::info!(
                "[post_actions] Spawned content_outcome_review {} for slug '{}' (parent {})",
                task.id,
                slug,
                ctx.task.id
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!(
                "[post_actions] Failed to spawn content_outcome_review for '{}': {}",
                slug,
                e
            );
            None
        }
    }
}

/// Resolve the article slug whose outcome should be reviewed.
///
/// - consolidate_cluster: the keeper slug from the merge plan artifacts.
/// - write_article / fix_content_article: slug of the written/modified file.
fn outcome_review_slug(ctx: &PostTaskContext<'_>) -> Option<String> {
    if ctx.task.task_type == "consolidate_cluster" {
        return merge_keeper_slug(ctx.task);
    }

    let file = find_written_article_file(ctx)?;
    // slug_from_filename already strips numeric prefixes, so normalize_url_slug
    // composes cleanly on top of it.
    let slug = crate::content::slug::normalize_url_slug(&crate::content::ops::slug_from_filename(
        &file,
    ));
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

/// Extract the keeper slug from a consolidate_cluster task's artifacts.
/// Primary source: the `merge_load_plan` step artifact (the recommendation
/// JSON with `keep_url`). Fallback: `cannibalization_strategy` matched by the
/// cluster id in the task title ("Merge cluster: <id>").
fn merge_keeper_slug(task: &Task) -> Option<String> {
    let slug_from_keep_url = |keep_url: &str| -> Option<String> {
        let slug = crate::content::slug::extract_slug_from_url(keep_url);
        if slug.is_empty() {
            None
        } else {
            Some(slug)
        }
    };

    if let Some(plan) = task
        .artifacts
        .iter()
        .find(|a| a.key == "merge_load_plan")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
    {
        if let Some(keep_url) = plan["keep_url"].as_str() {
            if let Some(slug) = slug_from_keep_url(keep_url) {
                return Some(slug);
            }
        }
    }

    let cluster_id = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .unwrap_or("")
        .trim();
    if cluster_id.is_empty() {
        return None;
    }
    let strategy = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())?;
    let rec = strategy["merge_recommendations"].as_array()?.iter().find(|r| {
        r["cluster_id"].as_str().unwrap_or("") == cluster_id
    })?;
    slug_from_keep_url(rec["keep_url"].as_str().unwrap_or(""))
}

/// Snapshot the article's current GSC metrics as the outcome baseline.
///
/// Reads the `gsc` namespace from `article_metadata` (the 90-day aggregate
/// written by the GSC sync). Returns (clicks, impressions, position, source).
/// A zeroed baseline with source "none" is fine for brand-new articles.
fn outcome_baseline_metrics(
    conn: &Connection,
    project_id: &str,
    slug: &str,
) -> (f64, f64, f64, &'static str) {
    let normalized = crate::content::slug::normalize_url_slug(slug);
    let articles = match task_store::list_articles(conn, project_id) {
        Ok(a) => a,
        Err(_) => return (0.0, 0.0, 0.0, "none"),
    };
    let article = articles.iter().find(|a| {
        crate::content::slug::normalize_url_slug(&a.url_slug) == normalized
    });
    let article_id = match article {
        Some(a) => a.id,
        None => return (0.0, 0.0, 0.0, "none"),
    };

    let payload: Option<String> = conn
        .query_row(
            "SELECT payload FROM article_metadata
             WHERE project_id = ?1 AND article_id = ?2 AND namespace = 'gsc'",
            rusqlite::params![project_id, article_id],
            |row| row.get(0),
        )
        .ok();
    payload
        .and_then(|p| serde_json::from_str::<serde_json::Value>(&p).ok())
        .map(|v| {
            (
                v["clicks"].as_f64().unwrap_or(0.0),
                v["impressions"].as_f64().unwrap_or(0.0),
                v["avg_position"].as_f64().unwrap_or(0.0),
                "article_metadata_gsc",
            )
        })
        .unwrap_or((0.0, 0.0, 0.0, "none"))
}

// ─── Content-task keyword / title helpers ───────────────────────────────────

/// Known imperative prefixes used by content-task factories
/// ("Write article: X", "Create hub: X", …).
const CONTENT_TASK_TITLE_PREFIXES: &[&str] = &[
    "Write territory article:",
    "Write article:",
    "Create hub:",
    "Refresh hub:",
];

/// Strip known content-task title prefixes, returning the bare topic.
///
/// Single source of truth for title-prefix stripping — previously inlined in
/// `exec/agentic.rs` (`task_topic_stem`, `hub_spoke_context`) and
/// `exec/content/cluster_link.rs`.
pub(crate) fn strip_content_task_title_prefix(title: &str) -> &str {
    let mut topic = title.trim();
    loop {
        let before = topic;
        for prefix in CONTENT_TASK_TITLE_PREFIXES {
            if let Some(rest) = topic.strip_prefix(prefix) {
                topic = rest.trim_start();
            }
        }
        if topic == before {
            return topic;
        }
    }
}

/// Parse the `"Target keyword:"` line from a content task's description.
///
/// Single source of truth for the keyword line — previously triplicated in
/// `parse_content_task_keyword_meta` and `exec/agentic.rs::task_topic_stem`.
pub(crate) fn content_task_target_keyword(task: &Task) -> Option<String> {
    let desc = task.description.as_deref()?;
    for line in desc.lines() {
        if let Some(rest) = line.strip_prefix("Target keyword:") {
            let keyword = rest.trim();
            if !keyword.is_empty() {
                return Some(keyword.to_string());
            }
        }
    }
    None
}

/// Parse keyword metadata embedded in the write_article task description.
pub(crate) fn parse_content_task_keyword_meta(task: &Task) -> (Option<String>, Option<String>, i64) {
    let desc = match task.description.as_deref() {
        Some(d) if !d.is_empty() => d,
        _ => return (None, None, 0),
    };
    let keyword = content_task_target_keyword(task);
    let mut kd: Option<String> = None;
    let mut volume = 0i64;
    for line in desc.lines() {
        if let Some(rest) = line.strip_prefix("KD:") {
            if let Ok(n) = rest.trim().parse::<i64>() {
                kd = Some(n.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("Volume:") {
            volume = rest.trim().parse::<i64>().unwrap_or(0);
        }
    }
    (keyword, kd, volume)
}

/// Derive the expected URL slug from a filename stem and find the resolved file path
/// for a content-modifying task.
///
/// Returns `Some((expected_slug, absolute_file_path))` if the task modifies a single
/// known article file, or `None` for new-article tasks where no baseline exists.
fn find_expected_slug_and_file(ctx: &PostStepContext<'_>) -> Option<(String, std::path::PathBuf)> {
    let project_path = std::path::Path::new(ctx.project_path);
    let desc = ctx.task.description.as_deref().unwrap_or("");

    // Try to extract a file path from the description via the shared parser
    // (content::ops::file_path_from_description). Patterns:
    //   "File: ./src/blog/posts/02_post.mdx"
    //   "File: ./webapp/content/blog/13_post.mdx"
    // Fallback: try to parse "Article ID: X" and look up the file in DB.
    let file_path = if let Some(path) =
        crate::content::ops::file_path_from_description(desc, project_path)
    {
        path
    } else if let Some(start) = desc.find("Article ID:") {
        let rest = &desc[start + 11..];
        let id_str = rest.trim_start().split(|c: char| !c.is_ascii_digit()).next().unwrap_or("");
        if let Ok(article_id) = id_str.parse::<i64>() {
            if let Ok(articles) = task_store::list_articles(ctx.conn, &ctx.task.project_id) {
                if let Some(article) = articles.iter().find(|a| a.id == article_id) {
                    let path = std::path::Path::new(&article.file);
                    if path.is_relative() {
                        project_path.join(path)
                    } else {
                        path.to_path_buf()
                    }
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        return None;
    };

    if !file_path.exists() {
        return None;
    }

    // Derive expected slug from filename stem (same logic as article_index.rs)
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let expected = crate::content::slug::strip_numeric_prefix(stem)
        .to_lowercase()
        .replace('_', "-");

    if expected.is_empty() {
        return None;
    }

    Some((expected, file_path))
}

// ─── Quality gate + topic health helpers ─────────────────────────────────────

/// Find the article file that was just written by a content task.
///
/// Priority:
/// 1. "File: ..." in task description.
/// 2. Most recently created/updated article for the project.
fn find_written_article_file(ctx: &PostTaskContext<'_>) -> Option<String> {
    let desc = ctx.task.description.as_deref().unwrap_or("");

    // 1. File path from description.
    if let Some(start) = desc.find("File: ") {
        let rest = &desc[start + 6..];
        let end = rest.find(" |").or_else(|| rest.find('\n')).unwrap_or(rest.len());
        let file = rest[..end].trim();
        if !file.is_empty() {
            return Some(file.to_string());
        }
    }

    // 2. Fallback: most recent article for the project.
    let row: Result<String, rusqlite::Error> = ctx.conn.query_row(
        "SELECT file FROM articles
         WHERE project_id = ?1 AND file IS NOT NULL AND file != ''
         ORDER BY COALESCE(updated_at, created_at) DESC
         LIMIT 1",
        rusqlite::params![&ctx.task.project_id],
        |r| r.get(0),
    );
    row.ok()
}

/// Pure classification logic for topic health.
///
/// Extracted so the threshold math can be unit-tested without filesystem or DB state.
fn classify_topic_health(
    avg_quality: i64,
    quality_count: i64,
    total_clicks: f64,
    total_impressions: f64,
) -> (&'static str, Option<f64>) {
    let health_status = if avg_quality >= 70 && (total_clicks > 0.0 || total_impressions >= 1000.0) {
        "promising"
    } else if avg_quality < 50 && total_impressions < 100.0 && total_clicks == 0.0 {
        "depleted"
    } else {
        "unproven"
    };

    let signal_score = if quality_count > 0 {
        Some((avg_quality as f64) + (total_clicks * 10.0) + (total_impressions / 100.0))
    } else {
        None
    };

    (health_status, signal_score)
}

/// Reduce content review / audit signals into per-topic health scores on research_shortlist.
fn run_topic_health_reducer(ctx: &PostTaskContext<'_>) -> crate::error::Result<()> {
    use crate::db::research_shortlist;

    // Load latest audit artifacts for this project.
    let paths = crate::engine::project_paths::ProjectPaths::from_path(ctx.project_path);
    let audit_path = paths.automation_dir.join("content_audit.json");
    let audit_json = std::fs::read_to_string(&audit_path).unwrap_or_default();
    if audit_json.is_empty() {
        return Ok(());
    }
    let audit: serde_json::Value = serde_json::from_str(&audit_json).unwrap_or_default();
    let articles = audit["articles"].as_array().unwrap_or(&Vec::new()).clone();
    if articles.is_empty() {
        return Ok(());
    }

    // Group audited articles by target_keyword/theme and aggregate signals.
    let mut by_theme: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    for article in articles {
        let theme = article["target_keyword"]
            .as_str()
            .or_else(|| article["url_slug"].as_str())
            .unwrap_or("")
            .to_string();
        if theme.is_empty() {
            continue;
        }
        by_theme.entry(theme).or_default().push(article);
    }

    for (theme, items) in by_theme {
        let mut total_quality: i64 = 0;
        let mut quality_count: i64 = 0;
        let mut total_impressions: f64 = 0.0;
        let mut total_clicks: f64 = 0.0;
        let mut min_quality: i64 = i64::MAX;

        for item in &items {
            let quality = item["quality_score"].as_i64().unwrap_or(0);
            if quality > 0 {
                total_quality += quality;
                quality_count += 1;
                min_quality = min_quality.min(quality);
            }
            total_impressions += item["gsc"]["impressions"].as_f64().unwrap_or(0.0);
            total_clicks += item["gsc"]["clicks"].as_f64().unwrap_or(0.0);
        }

        let avg_quality = if quality_count > 0 {
            total_quality / quality_count
        } else {
            0
        };

        let (health_status, signal_score) = classify_topic_health(
            avg_quality,
            quality_count,
            total_clicks,
            total_impressions,
        );

        let normalized_theme = theme.to_lowercase().trim().to_string();
        if normalized_theme.is_empty() {
            continue;
        }

        // Update exact theme match if it exists; otherwise update any shortlist entry
        // whose theme contains the keyword or vice versa.
        let existing = research_shortlist::list_entries(ctx.conn, &ctx.task.project_id, None)?;
        let matched_id = existing.iter().find(|e| e.theme.to_lowercase() == normalized_theme).map(|e| e.id);

        if matched_id.is_some() {
            research_shortlist::update_health(
                ctx.conn,
                &ctx.task.project_id,
                &theme,
                health_status,
                signal_score,
            )?;
        } else {
            // Best-effort fuzzy match: first shortlist theme that contains the keyword.
            for entry in existing {
                if normalized_theme.contains(&entry.theme.to_lowercase())
                    || entry.theme.to_lowercase().contains(&normalized_theme)
                {
                    research_shortlist::update_health(
                        ctx.conn,
                        &ctx.task.project_id,
                        &entry.theme,
                        health_status,
                        signal_score,
                    )?;
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_topic_health_promising_when_quality_and_traffic_signals_are_strong() {
        let (status, score) = classify_topic_health(75, 2, 5.0, 500.0);
        assert_eq!(status, "promising");
        assert!(score.is_some());
    }

    #[test]
    fn classify_topic_health_promising_with_high_impressions_even_without_clicks() {
        let (status, score) = classify_topic_health(80, 1, 0.0, 1200.0);
        assert_eq!(status, "promising");
        assert!(score.is_some());
    }

    #[test]
    fn classify_topic_health_depleted_when_quality_and_impressions_are_low() {
        let (status, _score) = classify_topic_health(40, 2, 0.0, 50.0);
        assert_eq!(status, "depleted");
        // Any clicks should prevent depleted classification.
        let (status_with_clicks, _) = classify_topic_health(40, 2, 1.0, 50.0);
        assert_eq!(status_with_clicks, "unproven");
        // Higher impressions should prevent depleted classification.
        let (status_with_impressions, _) = classify_topic_health(40, 2, 0.0, 150.0);
        assert_eq!(status_with_impressions, "unproven");
    }

    #[test]
    fn classify_topic_health_unproven_for_mixed_or_missing_signals() {
        let (status, score) = classify_topic_health(60, 2, 0.0, 500.0);
        assert_eq!(status, "unproven");
        assert!(score.is_some());

        // No quality data but some traffic → still unproven (not enough evidence either way).
        let (no_quality_status, no_quality_score) = classify_topic_health(0, 0, 0.0, 500.0);
        assert_eq!(no_quality_status, "unproven");
        assert!(no_quality_score.is_none());
    }

    #[test]
    fn classify_topic_health_signal_score_combines_quality_clicks_and_impressions() {
        let (_, score) = classify_topic_health(70, 1, 3.0, 500.0);
        // 70 + (3 * 10) + (500 / 100) = 70 + 30 + 5 = 105
        assert_eq!(score, Some(105.0));
    }

    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    fn make_task() -> Task {
        Task {
            id: "test-task".to_string(),
            task_type: "write_article".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    #[test]
    fn content_task_target_keyword_reads_keyword_line() {
        let mut task = make_task();
        task.description =
            Some("Target keyword: gamma scalping strategy\nKD: 35\nVolume: 3000".to_string());
        assert_eq!(
            content_task_target_keyword(&task).as_deref(),
            Some("gamma scalping strategy")
        );
    }

    #[test]
    fn content_task_target_keyword_skips_empty_and_missing() {
        let mut task = make_task();
        assert!(content_task_target_keyword(&task).is_none());

        task.description = Some("KD: 35\nVolume: 3000".to_string());
        assert!(content_task_target_keyword(&task).is_none());

        task.description = Some("Target keyword:\nKD: 35".to_string());
        assert!(content_task_target_keyword(&task).is_none());
    }

    #[test]
    fn strip_content_task_title_prefix_strips_known_prefixes() {
        assert_eq!(
            strip_content_task_title_prefix("Write article: delta hedging"),
            "delta hedging"
        );
        assert_eq!(
            strip_content_task_title_prefix("Write territory article: theta decay"),
            "theta decay"
        );
        assert_eq!(
            strip_content_task_title_prefix("Create hub: options greeks"),
            "options greeks"
        );
        assert_eq!(
            strip_content_task_title_prefix("Refresh hub: options greeks"),
            "options greeks"
        );
        // No-space variant (hub titles are stripped with bare prefixes upstream).
        assert_eq!(
            strip_content_task_title_prefix("Create hub:options greeks"),
            "options greeks"
        );
        // Unknown prefixes and bare titles are returned trimmed but intact.
        assert_eq!(
            strip_content_task_title_prefix("Cluster and link: delta hedging"),
            "Cluster and link: delta hedging"
        );
        assert_eq!(strip_content_task_title_prefix("plain title"), "plain title");
    }
}
