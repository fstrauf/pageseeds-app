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
        let mut last_pending = -1i64;
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
        let fix_task_ids = crate::engine::exec::content::create_fix_content_article_tasks(
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

    // Indexing health campaign → spawn child fix tasks from campaign plan
    if ctx.task.task_type == "indexing_health_campaign" {
        let spawned = crate::engine::exec::indexing_health_campaign::spawn_campaign_children(
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
            ..Default::default()
        };

        if let Ok(task) = crate::engine::spawner::TaskSpawner::spawn(ctx.conn, spec) {
            // Set not_before on the spawned task
            let _ = ctx.conn.execute(
                "UPDATE tasks SET not_before = ?1 WHERE id = ?2",
                rusqlite::params![not_before, &task.id],
            );
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

    // Generate feature spec after audit tasks complete
    if matches!(
        ctx.task.task_type.as_str(),
        "content_review" | "content_audit" | "ctr_audit" | "indexing_health_campaign"
    ) {
        if let Err(e) = generate_feature_spec(ctx.project_path, ctx.task) {
            log::warn!("[post_actions] Failed to generate feature spec: {}", e);
        }
    }

    follow_up_ids
}

/// Generate a developer feature spec from audit artifacts.
/// Writes seo_feature_spec.md to the project's automation directory.
fn generate_feature_spec(project_path: &str, task: &Task) -> Result<(), String> {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let automation_dir = &paths.automation_dir;

    let mut issues: Vec<String> = Vec::new();

    // 1. Read content_audit.json for developer-actionable issues
    let audit_path = automation_dir.join("content_audit.json");
    if let Ok(content) = std::fs::read_to_string(&audit_path) {
        if let Ok(audit) = serde_json::from_str::<serde_json::Value>(&content) {
            // Literal template variables in titles
            if let Some(articles) = audit["articles"].as_array() {
                let literal_vars: Vec<&serde_json::Value> = articles
                    .iter()
                    .filter(|a| a["checks"]["literal_template_variable"]["pass"].as_bool() == Some(false))
                    .collect();
                if !literal_vars.is_empty() {
                    issues.push(format!(
                        "## Title Template Variables\n\n{} articles have unrendered template variables in their titles (e.g., `| Brand |`, `{{Brand}}`, `{{{{title}}}}`).\n\n### Affected articles\n{}",
                        literal_vars.len(),
                        literal_vars.iter().map(|a| {
                            format!("- `{}`: {}\n", a["file"].as_str().unwrap_or("unknown"), a["title"].as_str().unwrap_or(""))
                        }).collect::<String>()
                    ));
                }

                // Exact duplicate content
                if let Some(dup_groups) = audit["duplicate_groups"].as_array() {
                    if !dup_groups.is_empty() {
                        issues.push(format!(
                            "## Exact Duplicate Content\n\n{} groups of articles share identical body content. This often indicates SSR fallback pages or template errors serving the same HTML for different URLs.\n\n### Duplicate groups\n{}",
                            dup_groups.len(),
                            dup_groups.iter().map(|g| {
                                let articles = g["articles"].as_array().map(|a| {
                                    a.iter().map(|art| format!("- `{}`\n", art["file"].as_str().unwrap_or("unknown"))).collect::<String>()
                                }).unwrap_or_default();
                                format!("**Group** ({} articles):\n{}", g["article_count"].as_u64().unwrap_or(0), articles)
                            }).collect::<String>()
                        ));
                    }
                }

                // Temporal URLs
                let temporal: Vec<&serde_json::Value> = articles
                    .iter()
                    .filter(|a| a["checks"]["temporal_url"]["pass"].as_bool() == Some(false))
                    .collect();
                if !temporal.is_empty() {
                    issues.push(format!(
                        "## Temporal URLs\n\n{} articles have time-sensitive URL slugs (month names, years, seasonal terms, relative time). These decay in SEO value and should be rewritten to evergreen slugs with 301 redirects.\n\n### Affected articles\n{}",
                        temporal.len(),
                        temporal.iter().map(|a| {
                            format!("- `{}` → `{}`\n", a["url_slug"].as_str().unwrap_or(""), a["file"].as_str().unwrap_or("unknown"))
                        }).collect::<String>()
                    ));
                }
            }
        }
    }

    // 2. Read ctr_audit_context.json for template issues
    let ctr_path = automation_dir.join("ctr_audit_context.json");
    if let Ok(content) = std::fs::read_to_string(&ctr_path) {
        if let Ok(ctr) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(articles) = ctr["articles"].as_array() {
                let template_issues: Vec<&serde_json::Value> = articles
                    .iter()
                    .filter(|a| {
                        a["issues_detected"].as_array().map(|issues| {
                            issues.iter().any(|i| {
                                let s = i.as_str().unwrap_or("");
                                s.contains("template") || s.contains("duplicate") || s.contains("brand")
                            })
                        }).unwrap_or(false)
                    })
                    .collect();
                if !template_issues.is_empty() {
                    issues.push(format!(
                        "## CTR Template Issues\n\n{} articles have title/template issues detected by the CTR audit.\n\n### Common patterns\n- Duplicate brand names in titles (`Title | Brand | Brand`)\n- Missing dynamic title fallback\n- Template variables rendered as literal text\n\n### Fix\nReview your site's layout/template file (e.g., `app/layout.tsx`, `_app.js`, or equivalent) to ensure title templates only append the brand once.\n",
                        template_issues.len()
                    ));
                }
            }
        }
    }

    if issues.is_empty() {
        log::info!("[feature_spec] No developer-actionable issues found — skipping spec generation");
        return Ok(());
    }

    let spec = format!(
        "# SEO Feature Spec\n\nGenerated: {}\nTask: {} ({}))\n\nThis document contains developer-actionable issues identified by the SEO audit. Content-level fixes (rewriting articles, merging cannibalized pages) are handled separately via fix tasks.\n\n---\n\n{}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        task.title.as_deref().unwrap_or("untitled"),
        task.id,
        issues.join("\n\n---\n\n")
    );

    let spec_path = automation_dir.join("seo_feature_spec.md");
    std::fs::create_dir_all(automation_dir).map_err(|e| e.to_string())?;
    std::fs::write(&spec_path, spec).map_err(|e| e.to_string())?;

    log::info!(
        "[feature_spec] Wrote {} developer-actionable issues to {}",
        issues.len(),
        spec_path.display()
    );
    Ok(())
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

/// Derive the expected URL slug from a filename stem and find the resolved file path
/// for a content-modifying task.
///
/// Returns `Some((expected_slug, absolute_file_path))` if the task modifies a single
/// known article file, or `None` for new-article tasks where no baseline exists.
fn find_expected_slug_and_file(ctx: &PostStepContext<'_>) -> Option<(String, std::path::PathBuf)> {
    let project_path = std::path::Path::new(ctx.project_path);
    let desc = ctx.task.description.as_deref().unwrap_or("");

    // Try to extract a file path from the description.
    // Patterns:
    //   "File: ./src/blog/posts/02_post.mdx"
    //   "File: ./webapp/content/blog/13_post.mdx"
    let file_path = if let Some(start) = desc.find("File: ") {
        let rest = &desc[start + 6..];
        let end = rest.find(" |").unwrap_or(rest.len());
        let path_str = rest[..end].trim();
        let path = std::path::Path::new(path_str);
        if path.is_relative() {
            project_path.join(path)
        } else {
            path.to_path_buf()
        }
    } else if let Some(start) = desc.find("File:") {
        // Handle "File:./path" without space
        let rest = &desc[start + 5..];
        let end = rest.find(" |").or_else(|| rest.find('\n')).unwrap_or(rest.len());
        let path_str = rest[..end].trim();
        let path = std::path::Path::new(path_str);
        if path.is_relative() {
            project_path.join(path)
        } else {
            path.to_path_buf()
        }
    } else {
        // Fallback: try to parse "Article ID: X" and look up the file in DB
        if let Some(start) = desc.find("Article ID:") {
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
        }
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
