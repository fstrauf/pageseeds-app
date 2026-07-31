/// PageSeeds CLI — individual data tools for KimiCode.
///
/// Each subcommand calls a shared standalone function from
/// engine/tools/investigate.rs (the same functions the Rig Tool impls use).
/// Zero business logic duplication.
///
/// Usage (preferred — installed binary, any cwd):
///   pageseeds-cli setup --path . --yes
///   pageseeds-cli <tool> [-i <project-id>] [-p <project-path>] [args...]
/// Install:
///   curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
/// Dev (from pageseeds-app checkout): ./scripts/install-cli.sh  or  FROM_SOURCE=1 ./scripts/install-cli.sh
/// Dev only: cargo run --bin pageseeds-cli -- <tool> ...

use pageseeds_core::config::cli_config::{self, expand_tilde, MISSING_PROJECT_CONTEXT_HINT};
use pageseeds_core::engine::cli_setup;
use pageseeds_core::engine::tools::{InvestigationContext, investigate};
use pageseeds_core::models::cannibalization::ApprovalStatus;
use pageseeds_core::models::task::{Priority, TaskRunPolicy, TaskStatus};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Early help: no args, bare `help`, or -h/--help anywhere (including after tool name).
    // Must run BEFORE requiring -i/-p so e.g. `pageseeds-cli research-pull --help` exits 0.
    if wants_help(&args) {
        print_help();
        return;
    }

    if args[1] == "--version" || args[1] == "-V" {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let tool = &args[1];

    // License subcommands are always free and must not require -i/-p or open the DB.
    if tool == "license" {
        run_license_command(&args);
        return;
    }

    // Free meta tools: no paid gate; handle before project-context resolution.
    if tool == "list-projects" {
        run_list_projects();
        return;
    }
    if tool == "create-project" {
        run_create_project(&args);
        return;
    }
    if tool == "setup" {
        run_setup(&args);
        return;
    }
    if tool == "sync-site-urls" {
        run_sync_site_urls(&args);
        return;
    }

    // Gate paid tools before any DB / project work. Unknown tools skip the gate
    // so they still hit the existing "Unknown tool" path.
    if pageseeds_core::license::requires_paid_license(tool) {
        if let Err(_e) = pageseeds_core::license::require_valid() {
            exit(&format!(
                "Paid command '{tool}' requires a valid PageSeeds license.\n\
Activate: pageseeds-cli license activate <key>\n\
Buy: https://pageseeds.com"
            ));
        }
    }

    let db = pageseeds_core::db::default_db_path();
    let flags_id = flag(&args, "--project-id", "-i");
    let flags_path = flag(&args, "--project-path", "-p")
        .as_deref()
        .map(expand_tilde);

    // Resolve project context: flags → env → local yaml → global defaults → DB fill.
    // Explicit -i/-p always win; after `setup`, desk tools work without flags.
    let cwd = std::env::current_dir().unwrap_or_default();
    let conn_for_resolve = open_db(&db.to_string_lossy()).ok();
    let resolved = cli_config::resolve_project_context(
        flags_id.as_deref(),
        flags_path.as_deref(),
        &cwd,
        conn_for_resolve.as_ref(),
    );
    let (project_id, project_path) = match resolved {
        Ok(r) => (r.project_id, Some(r.project_path)),
        Err(_) => {
            // Keep empty defaults so tools that don't need project (e.g. get-task)
            // still run; tools that require path/id exit with setup hint below.
            (
                flags_id.clone().unwrap_or_default(),
                flags_path.clone(),
            )
        }
    };

    let ctx = InvestigationContext {
        project_id: project_id.clone(),
        project_path: project_path.clone().unwrap_or_default(),
        db_path: db.to_string_lossy().to_string(),
    };

    let require_project_path = || -> String {
        project_path
            .clone()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| exit(MISSING_PROJECT_CONTEXT_HINT))
    };

    let result: Result<serde_json::Value, String> = match tool.as_str() {
        // ── GSC tools (async, kept inline since they need tokio) ──
        "gsc-performance" => gsc_perf(&project_id, &require_project_path(), &args),
        "gsc-queries" => gsc_q(
            &project_id,
            &require_project_path(),
            flag(&args, "--page-url", "-u"),
            &args,
        ),
        "gsc-movers" => gsc_mov(&project_id, &require_project_path(), &args),

        // ── Task / queue orchestration ──
        "list-tasks" => list_tasks(&db.to_string_lossy(), &project_id, &args),
        "cancel-tasks" => cancel_tasks(&db.to_string_lossy(), &project_id, &args),
        "create-task" => create_task(&project_id, &db.to_string_lossy(), &require_project_path(), &args),
        "execute-task" => execute_task(&db.to_string_lossy(), &args),
        "get-task" => get_task(&db.to_string_lossy(), &args),
        "update-task-status" => update_task_status_cmd(&db.to_string_lossy(), &args),
        "select-keywords" => select_keywords(&db.to_string_lossy(), &args),
        "write-context" => write_context(&db.to_string_lossy(), &project_id, &require_project_path(), &args),
        "write-submit" => write_submit(&db.to_string_lossy(), &project_id, &require_project_path(), &args),
        "publish-content" => publish_content(&db.to_string_lossy(), &project_id, &require_project_path(), &args),
        "research-context" => research_context(&db.to_string_lossy(), &project_id),
        "strategy" => strategy_cmd(&require_project_path()),
        "project-config-status" => project_config_status_cmd(&require_project_path()),
        "migrate-project-config" => migrate_project_config_cmd(&require_project_path(), &args),
        "research-pull" => research_pull(&db.to_string_lossy(), &project_id, &args),
        "merge-context" => merge_context(&db.to_string_lossy(), &project_id, &require_project_path(), &args),
        "merge-submit" => merge_submit(&db.to_string_lossy(), &project_id, &require_project_path(), &args),
        "select-content-review" => select_content_review(&db.to_string_lossy(), &args),
        "select-cannibalization" => select_cannibalization(&db.to_string_lossy(), &args),
        "create-articles-from-keywords" => create_articles_from_keywords(&db.to_string_lossy(), &project_id, &args),
        "set-task-status" => set_task_status(&db.to_string_lossy(), &args),
        "create-reddit-replies" => create_reddit_replies(&db.to_string_lossy(), &args),

        // ── Cannibalization strategy workflow ──
        "cannibalization-strategy" => cannibalization_strategy(&db.to_string_lossy(), &project_id),
        "set-review-status" => set_review_status(&db.to_string_lossy(), &args),
        "create-tasks-from-approved" => create_tasks_from_approved(&db.to_string_lossy(), &project_id, &args),

        // ── Dead-weight remediation (WS4) ──
        "score-zero-impression-articles" => score_zero_impression_articles(&db.to_string_lossy(), &project_id, &require_project_path(), &args),

        // ── Site State desk tools (shared domain builders) ──
        "site-overview" => {
            let period_days: Option<i64> = flag(&args, "--period-days", "-d")
                .and_then(|s| s.parse().ok());
            let path = require_project_path();
            open_db(&db.to_string_lossy()).and_then(|conn| {
                pageseeds_core::engine::site_state::build_site_overview(
                    &conn,
                    &project_id,
                    &path,
                    period_days,
                )
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
            })
        }
        // CTR outcome closed-loop (issue #302): compare + report, no task fan-out.
        "ctr-outcomes" => {
            open_db(&db.to_string_lossy()).and_then(|conn| {
                let compare = pageseeds_core::engine::exec::ctr_audit::run_ctr_outcome_compare(
                    &conn,
                    &project_id,
                )
                .map_err(|e| e.to_string())?;
                let report = pageseeds_core::engine::exec::ctr_audit::run_ctr_outcome_report(
                    &conn,
                    &project_id,
                )
                .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({
                    "project_id": project_id,
                    "compare": compare,
                    "report": report,
                }))
            })
        }
        "articles" => {
            let period_days: Option<i64> = flag(&args, "--period-days", "-d")
                .and_then(|s| s.parse().ok());
            let min_impressions: f64 = flag(&args, "--min-impressions", "-m")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let include_redirected = has_flag(&args, "--include-redirected", "-R");
            let limit: Option<usize> = flag(&args, "--limit", "-l")
                .and_then(|s| s.parse().ok());
            let path = require_project_path();
            let status = flag(&args, "--status", "-s");
            open_db(&db.to_string_lossy()).and_then(|conn| {
                pageseeds_core::engine::site_state::list_articles_catalog(
                    &conn,
                    &project_id,
                    &path,
                    pageseeds_core::engine::site_state::ArticlesFilter {
                        status,
                        min_impressions,
                        include_redirected,
                        limit,
                        period_days,
                    },
                )
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
            })
        }
        "article" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            let period_days: Option<i64> = flag(&args, "--period-days", "-d")
                .and_then(|s| s.parse().ok());
            let path = require_project_path();
            open_db(&db.to_string_lossy()).and_then(|conn| {
                pageseeds_core::engine::site_state::get_article_package(
                    &conn,
                    &project_id,
                    &path,
                    &slug,
                    period_days,
                )
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
            })
        }

        // ── Shared functions (single source of truth) ──
        "article-list" => {
            investigate::list_articles_json(&ctx, flag(&args, "--status", "-s").as_deref(), 200)
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "article-frontmatter" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            article_frontmatter(&require_project_path(), &slug)
        }
        "article-body-hash" => {
            investigate::hash_article_bodies(&ctx)
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "article-title-scan" => investigate::scan_article_titles(&ctx).map_err(|e| e.to_string()),
        "validate-article" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            let _ = require_project_path();
            if project_id.is_empty() {
                exit(MISSING_PROJECT_CONTEXT_HINT);
            }
            investigate::validate_article_json(&ctx, &slug)
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        // ── Path B fix package / submit (no nested generate) ──
        "fix-context" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            let kind_raw = flag(&args, "--kind", "-k").unwrap_or_else(|| exit("--kind content|ctr|refresh required"));
            let goals = flag(&args, "--goals", "-g");
            let period_days: Option<i64> = flag(&args, "--period-days", "-d")
                .and_then(|s| s.parse().ok());
            let path = require_project_path();
            if project_id.is_empty() {
                exit("--project-id required");
            }
            let kind = pageseeds_core::engine::fix_package::FixKind::parse(&kind_raw)
                .unwrap_or_else(|e| exit(&e.to_string()));
            open_db(&db.to_string_lossy()).and_then(|conn| {
                pageseeds_core::engine::fix_package::build_fix_package(
                    &conn,
                    &project_id,
                    &path,
                    &slug,
                    kind,
                    goals.as_deref(),
                    period_days,
                )
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
            })
        }
        "fix-submit" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            let kind_raw = flag(&args, "--kind", "-k").unwrap_or_else(|| exit("--kind content|ctr|refresh required"));
            let path = require_project_path();
            if project_id.is_empty() {
                exit("--project-id required");
            }
            let kind = pageseeds_core::engine::fix_package::FixKind::parse(&kind_raw)
                .unwrap_or_else(|e| exit(&e.to_string()));
            let file_override = flag(&args, "--file", "-f");
            let target_keyword = flag(&args, "--keyword", "-K");
            let patch_json = match flag(&args, "--patch", "-P") {
                Some(p) => {
                    let expanded = expand_tilde(&p);
                    match std::fs::read_to_string(&expanded) {
                        Ok(s) => Some(s),
                        Err(e) => exit(&format!("failed to read patch file {expanded}: {e}")),
                    }
                }
                None => None,
            };
            open_db(&db.to_string_lossy()).and_then(|conn| {
                pageseeds_core::engine::fix_package::submit_fix(
                    &conn,
                    &project_id,
                    &path,
                    &slug,
                    kind,
                    pageseeds_core::engine::fix_package::FixSubmitOpts {
                        file_override,
                        patch_json,
                        target_keyword,
                    },
                )
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
            })
        }
        "content-audit-report" => investigate::read_content_audit_report(&require_project_path()).map_err(|e| e.to_string()),
        "run-content-audit" => run_audit(&project_id, &require_project_path()),
        "cannibalization-clusters" => investigate::read_cannibalization_clusters(&require_project_path()).map_err(|e| e.to_string()),
        "indexing-status" => investigate::get_indexing_status(&ctx).map_err(|e| e.to_string()),
        "ctr-health" => ctr_health(&project_id, &require_project_path(), &db.to_string_lossy()),
        "framework-files" => {
            investigate::read_framework_files(&require_project_path(), flag(&args, "--file", "-f").as_deref())
                .map_err(|e| e.to_string())
        }
        "article-link-graph" => investigate::scan_link_graph(&ctx).map_err(|e| e.to_string()),
        "research-shortlist" => {
            investigate::list_research_shortlist(
                &ctx,
                flag(&args, "--status", "-s").as_deref(),
                flag(&args, "--health", "-H").as_deref(),
            ).map_err(|e| e.to_string())
        }
        "article-quality-reviews" => {
            let limit: usize = flag(&args, "--limit", "-l")
                .and_then(|s| s.parse().ok())
                .unwrap_or(50);
            investigate::list_article_quality_reviews(&ctx, limit).map_err(|e| e.to_string())
        }
        "compare-rendered" => compare_rendered(&require_project_path(), &args),
        "write-feature-spec" => write_spec(&require_project_path(), &args),

        // ── Video clips (context = free desk read; render = operator tier) ──
        "video-clip-context" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            let path = require_project_path();
            open_db(&db.to_string_lossy()).and_then(|conn| {
                pageseeds_core::video::video_clip_context(&conn, &project_id, &path, &slug)
                    .map(|r| serde_json::to_value(r).unwrap_or_default())
                    .map_err(|e| e.to_string())
            })
        }
        "video-clip-render" => {
            // Operator tier (docs/CLI_COMMERCIAL.md): no license gate; dev-machine only.
            let clip = flag(&args, "--clip", "-c").unwrap_or_else(|| exit("--clip required"));
            let path = require_project_path();
            pageseeds_core::video::video_clip_render(
                std::path::Path::new(&path),
                std::path::Path::new(&clip),
            )
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .map_err(|e| e.to_string())
        }
        _ => Err(format!("Unknown tool '{}'. Run with --help for list.", tool)),
    };

    match result {
        Ok(json) => println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default()),
        Err(e) => { eprintln!("ERROR: {e}"); std::process::exit(1); }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Task / queue orchestration
// ═══════════════════════════════════════════════════════════════════════════════

fn list_tasks(db_path: &str, project_id: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let conn = open_db(db_path)?;
    let task_type = flag(args, "--task-type", "-t");
    let status = flag(args, "--status", "-s");
    let mut tasks = pageseeds_core::engine::task_store::list_tasks_light(&conn, project_id)
        .map_err(|e| e.to_string())?;
    if let Some(tt) = &task_type {
        tasks.retain(|t| t.task_type == *tt);
    }
    if let Some(s) = &status {
        let want: Vec<TaskStatus> = if s == "todo" {
            vec![TaskStatus::Todo, TaskStatus::Queued]
        } else {
            vec![serde_json::from_value(serde_json::Value::String(s.clone())).unwrap_or(TaskStatus::Todo)]
        };
        tasks.retain(|t| want.contains(&t.status));
    }
    Ok(serde_json::json!({
        "count": tasks.len(),
        "tasks": tasks,
    }))
}

fn cancel_tasks(db_path: &str, project_id: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let conn = open_db(db_path)?;
    let task_type = flag(args, "--task-type", "-t");
    let status = flag(args, "--status", "-s");
    let yes = has_flag(args, "--yes", "-y");

    if task_type.is_none() && status.is_none() {
        return Err("require at least one of --task-type or --status".to_string());
    }

    let mut tasks = pageseeds_core::engine::task_store::list_tasks_light(&conn, project_id)
        .map_err(|e| e.to_string())?;
    if let Some(tt) = &task_type {
        tasks.retain(|t| t.task_type == *tt);
    }
    if let Some(s) = &status {
        let want: Vec<TaskStatus> = if s == "todo" {
            vec![TaskStatus::Todo, TaskStatus::Queued]
        } else {
            vec![serde_json::from_value(serde_json::Value::String(s.clone())).unwrap_or(TaskStatus::Todo)]
        };
        tasks.retain(|t| want.contains(&t.status));
    }

    if tasks.is_empty() {
        return Ok(serde_json::json!({"cancelled": 0, "message": "no matching tasks"}));
    }

    let mut cancelable = Vec::new();
    for t in &tasks {
        match t.status {
            TaskStatus::Done | TaskStatus::Cancelled | TaskStatus::Failed => continue,
            _ => cancelable.push(t.id.clone()),
        }
    }

    if !yes {
        return Ok(serde_json::json!({
            "dry_run": true,
            "would_cancel": cancelable.len(),
            "task_ids": cancelable,
            "message": "pass --yes/-y to cancel",
        }));
    }

    let mut cancelled = Vec::new();
    for id in &cancelable {
        match pageseeds_core::engine::task_store::update_task_status(&conn, id, TaskStatus::Cancelled) {
            Ok(_) => cancelled.push(id.clone()),
            Err(e) => eprintln!("warn: failed to cancel {}: {}", id, e),
        }
    }

    Ok(serde_json::json!({
        "cancelled": cancelled.len(),
        "task_ids": cancelled,
    }))
}

fn execute_task(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let force = has_flag(args, "--force", "");
    let conn = open_db(db_path)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let opts = pageseeds_core::engine::executor::ExecuteOpts {
        dry_run: false,
        ignore_not_before: force,
    };
    let result = rt.block_on(async {
        pageseeds_core::engine::executor::execute_task_with_token(
            &conn, &task_id, None, &opts,
        )
        .await
    })?;
    Ok(serde_json::json!({
        "task_id": task_id,
        "success": result.success,
        "message": result.message,
        "steps": result.steps,
        "follow_up_tasks": result.follow_up_tasks,
    }))
}

fn get_task(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let conn = open_db(db_path)?;
    let task = pageseeds_core::engine::task_store::get_task(&conn, &task_id)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&task).map_err(|e| e.to_string())
}

fn update_task_status_cmd(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let status_str = flag(args, "--status", "-s").unwrap_or_else(|| exit("--status required"));
    let status = match status_str.as_str() {
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        other => {
            return Err(format!(
                "unsupported status '{other}': only 'done' and 'cancelled' are allowed"
            ));
        }
    };
    let conn = open_db(db_path)?;
    let task = pageseeds_core::engine::task_store::update_task_status(&conn, &task_id, status)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&task).map_err(|e| e.to_string())
}

/// Mirror of the `create_article_tasks_from_keywords` Tauri command:
/// build content tasks from selected keywords, persist them, mark the
/// research task done. project_id is derived from the research task row.
/// Uses the canonical creation path in `engine::keyword_selection` so the
/// content brief context is attached exactly like the Tauri command does.
fn select_keywords(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let research_task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let keywords: Vec<String> = flag(args, "--keywords", "-K")
        .map(|s| s.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect())
        .unwrap_or_else(|| exit("--keywords required (comma-separated)"));

    let conn = open_db(db_path)?;
    let research_task = pageseeds_core::engine::task_store::get_task(&conn, &research_task_id)
        .map_err(|e| e.to_string())?;
    let project_id = research_task.project_id.clone();

    let tasks = pageseeds_core::engine::keyword_selection::create_article_tasks_from_keywords(
        &conn,
        &project_id,
        &research_task_id,
        keywords,
    )?;

    Ok(serde_json::json!({
        "parent_task_id": research_task_id,
        "parent_status": TaskStatus::Done,
        "created": tasks.len(),
        "task_ids": tasks.iter().map(|t| &t.id).collect::<Vec<_>>(),
        "titles": tasks.iter().map(|t| &t.title).collect::<Vec<_>>(),
    }))
}

/// Path B: deterministic write package for outer-agent prose (no nested writer).
/// Requires -i project-id, -p project-path, -I research-task-id, -K keyword.
fn write_context(
    db_path: &str,
    project_id: &str,
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }
    // -I optional when -K is project.yaml Primary/problem (intentional strategy write).
    let research_task_id = flag(args, "--task-id", "-I");
    let keyword =
        flag(args, "--keyword", "-K").unwrap_or_else(|| exit("--keyword required"));

    let conn = open_db(db_path)?;
    let package = pageseeds_core::engine::write_package::build_write_package(
        &conn,
        project_id,
        std::path::Path::new(project_path),
        research_task_id.as_deref(),
        &keyword,
    )?;
    serde_json::to_value(package).map_err(|e| e.to_string())
}

/// Path B: validate MDX, ingest, mark write_article done, spawn cluster_and_link.
/// Always prints JSON including ok:false validation failures (exit 0).
/// Domain errors (missing file, bad project) return Err → exit 1.
fn write_submit(
    db_path: &str,
    project_id: &str,
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }
    let file = flag(args, "--file", "-f");
    let slug = flag(args, "--slug", "-S");
    let path_or_slug = file
        .or(slug)
        .unwrap_or_else(|| exit("--file (-f) or --slug (-S) required"));
    let write_task_id = flag(args, "--task-id", "-I");
    let keyword = flag(args, "--keyword", "-K");

    let conn = open_db(db_path)?;
    let result = pageseeds_core::engine::write_package::submit_written_article(
        &conn,
        project_id,
        std::path::Path::new(project_path),
        &path_or_slug,
        pageseeds_core::engine::write_package::SubmitOpts {
            write_task_id,
            keyword,
        },
    )?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Path B second step: catalog draft/ready_to_publish → published via preflight + apply_publish.
/// Slugs: `-S slug` or comma-separated `-S slug1,slug2`. Explicit only — never auto on write-submit.
fn publish_content(
    db_path: &str,
    project_id: &str,
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }
    let raw = flag(args, "--slug", "-S").unwrap_or_else(|| {
        exit("--slug (-S) required (comma-separated for multiple: -S slug1,slug2)")
    });
    let slugs: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if slugs.is_empty() {
        exit("--slug (-S) must not be empty");
    }

    let conn = open_db(db_path)?;
    let result = pageseeds_core::content::publish::publish_by_slugs(
        &conn,
        project_id,
        std::path::Path::new(project_path),
        &slugs,
    )?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Path B merge: deterministic package with full MDX bodies (no nested draft_patch).
/// Modes: -I consolidate-task-id | --keep-id + --redirect-ids | -K keep-url + -R redirect-urls.
fn merge_context(
    db_path: &str,
    project_id: &str,
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }

    use pageseeds_core::engine::merge_package::MergeContextSource;

    let source = if let Some(task_id) = flag(args, "--task-id", "-I") {
        MergeContextSource::ConsolidateTask { task_id }
    } else if let Some(keep_id_s) = flag(args, "--keep-id", "") {
        let keep_id: i64 = keep_id_s
            .parse()
            .map_err(|_| format!("invalid --keep-id: {keep_id_s}"))?;
        let redirect_ids: Vec<i64> = flag(args, "--redirect-ids", "")
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .map(|p| p.parse::<i64>().map_err(|_| format!("invalid redirect id: {p}")))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_else(|| exit("--redirect-ids required with --keep-id"));
        if redirect_ids.is_empty() {
            exit("--redirect-ids must not be empty");
        }
        MergeContextSource::ArticleIds {
            keep_id,
            redirect_ids,
        }
    } else if let Some(keep_url) = flag(args, "--keep-url", "-K") {
        let redirect_urls: Vec<String> = flag(args, "--redirect-urls", "-R")
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| exit("--redirect-urls (-R) required with --keep-url"));
        if redirect_urls.is_empty() {
            exit("--redirect-urls must not be empty");
        }
        MergeContextSource::Urls {
            keep_url,
            redirect_urls,
        }
    } else {
        exit(
            "merge-context requires -I <consolidate-task-id> OR --keep-id + --redirect-ids OR -K/--keep-url + -R/--redirect-urls",
        );
    };

    let conn = open_db(db_path)?;
    let package = pageseeds_core::engine::merge_package::build_merge_package(
        &conn,
        project_id,
        std::path::Path::new(project_path),
        source,
    )?;
    serde_json::to_value(package).map_err(|e| e.to_string())
}

/// Path B merge submit: validate keeper MDX, apply redirects/links/depublish/sync.
/// Validation failures → ok:false JSON (exit 0). Domain errors → Err (exit 1).
fn merge_submit(
    db_path: &str,
    project_id: &str,
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }

    let consolidate_task_id = flag(args, "--task-id", "-I");
    let keep_url = flag(args, "--keep-url", "-K");
    let redirect_urls = flag(args, "--redirect-urls", "-R").map(|s| {
        s.split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
    });
    let confirmed = has_flag(args, "--confirm", "-y");

    let conn = open_db(db_path)?;
    let result = pageseeds_core::engine::merge_package::submit_merge(
        &conn,
        project_id,
        std::path::Path::new(project_path),
        pageseeds_core::engine::merge_package::MergeSubmitOpts {
            consolidate_task_id,
            keep_url,
            redirect_urls,
            confirmed,
        },
    )?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Path B research strategy package: shortlist + health + open research tasks.
/// Deterministic. May refresh research_shortlist via territory analysis when
/// empty/stale (issue #192). Prefer over raw research-shortlist for session seed planning.
fn research_context(db_path: &str, project_id: &str) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }
    let conn = open_db(db_path)?;
    let package = pageseeds_core::engine::research_package::build_research_context(
        &conn,
        project_id,
        pageseeds_core::engine::research_package::RESEARCH_SHORTLIST_MAX_AGE_DAYS,
    )?;
    serde_json::to_value(package).map_err(|e| e.to_string())
}

/// Print project content strategy as JSON (from `project.yaml` via ensure;
/// auto-migrates legacy MD when needed). Empty strategy when no config —
/// never an error. When auto-migrated, includes `project_config_auto_migrated: true`.
fn strategy_cmd(project_path: &str) -> Result<serde_json::Value, String> {
    let paths = pageseeds_core::engine::project_paths::ProjectPaths::from_path(project_path);
    let outcome =
        pageseeds_core::strategy::load_project_strategy_detailed(paths.automation_dir());
    serde_json::to_value(outcome).map_err(|e| e.to_string())
}

/// Read-only: project.yaml vs legacy MD readiness as JSON.
fn project_config_status_cmd(project_path: &str) -> Result<serde_json::Value, String> {
    let paths = pageseeds_core::engine::project_paths::ProjectPaths::from_path(project_path);
    let status =
        pageseeds_core::project_config::project_config_status(paths.automation_dir());
    serde_json::to_value(status).map_err(|e| e.to_string())
}

/// Deterministic MD → project.yaml migrator. `--dry-run` plans only; `--force`
/// backs up then rewrites when YAML already exists.
fn migrate_project_config_cmd(
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    let paths = pageseeds_core::engine::project_paths::ProjectPaths::from_path(project_path);
    let opts = pageseeds_core::project_config::MigrateOpts {
        dry_run: has_flag(args, "--dry-run", ""),
        force: has_flag(args, "--force", ""),
    };
    let report =
        pageseeds_core::project_config::migrate_project_config(paths.automation_dir(), opts)
            .map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Path B research pull: session-owned seeds → custom_keyword_research (no nested theme LLM).
/// Default: create + execute. `--no-execute` only creates the task.
/// Seeds: `-K seed1,seed2,...` (comma-separated).
fn research_pull(
    db_path: &str,
    project_id: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    if project_id.is_empty() {
        exit("--project-id required");
    }
    let seeds: Vec<String> = flag(args, "--keywords", "-K")
        .map(|s| {
            s.split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_else(|| exit("--keywords (-K) required (comma-separated seeds)"));
    let title = flag(args, "--title", "-T");
    // Default execute=true; --no-execute creates only. --execute is explicit opt-in alias.
    let execute = !has_flag(args, "--no-execute", "--no-execute");
    let priority = match flag(args, "--priority", "-P")
        .unwrap_or_else(|| "medium".to_string())
        .as_str()
    {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let conn = open_db(db_path)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let result = rt.block_on(async {
        pageseeds_core::engine::research_package::research_pull(
            &conn,
            project_id,
            pageseeds_core::engine::research_package::ResearchPullOpts {
                seeds,
                title,
                execute,
                priority,
            },
        )
        .await
    })?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Mirror of the `select_content_review_follow_ups` Tauri command:
/// spawn fix_content_article tasks from selected proposal ids on a content_review
/// (or content_audit) parent, then mark the parent done.
fn select_content_review(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let parent_task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let proposal_ids: Vec<String> = flag(args, "--proposals", "-P")
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_else(|| exit("--proposals required (comma-separated proposal ids)"));

    let conn = open_db(db_path)?;
    let tasks = pageseeds_core::engine::content_review_selection::spawn_from_selection(
        &conn,
        &parent_task_id,
        &proposal_ids,
    )
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "parent_task_id": parent_task_id,
        "parent_status": TaskStatus::Done,
        "created": tasks.len(),
        "task_ids": tasks.iter().map(|t| &t.id).collect::<Vec<_>>(),
        "titles": tasks.iter().map(|t| &t.title).collect::<Vec<_>>(),
        "types": tasks.iter().map(|t| &t.task_type).collect::<Vec<_>>(),
    }))
}

/// Mirror of the `create_cannibalization_tasks_from_selection` Tauri command.
/// Selections are parsed from `-S merge:rec-123,hub:rec-456`.
fn select_cannibalization(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let parent_task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let raw = flag(args, "--selections", "-S")
        .unwrap_or_else(|| exit("--selections required (comma-separated type:id pairs)"));

    let mut selections = Vec::new();
    for pair in raw.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let (rec_type, rec_id) = pair.split_once(':').ok_or_else(|| {
            format!("invalid selection '{pair}': expected 'type:id'")
        })?;
        if rec_type.is_empty() || rec_id.is_empty() {
            return Err(format!("invalid selection '{pair}': expected 'type:id'"));
        }
        selections.push(pageseeds_core::models::cannibalization::CannibalizationSelection {
            recommendation_type: rec_type.to_string(),
            recommendation_id: rec_id.to_string(),
        });
    }

    let conn = open_db(db_path)?;
    let tasks = pageseeds_core::cannibalization::spawn_tasks_from_selection(
        &conn,
        &parent_task_id,
        &selections,
    )
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "parent_task_id": parent_task_id,
        "created": tasks.len(),
        "task_ids": tasks.iter().map(|t| &t.id).collect::<Vec<_>>(),
        "titles": tasks.iter().map(|t| &t.title).collect::<Vec<_>>(),
    }))
}

/// Transition a task's status (e.g. mark a manually-completed task done).
fn set_task_status(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let status = flag(args, "--status", "-s").unwrap_or_else(|| exit("--status required"));
    let status_enum: TaskStatus = serde_json::from_value(serde_json::Value::String(status.clone()))
        .map_err(|_| format!("unknown status '{status}' (todo|queued|in_progress|review|done|failed|cancelled)"))?;
    let conn = open_db(db_path)?;
    pageseeds_core::engine::task_store::update_task_status(&conn, &task_id, status_enum)?;
    Ok(serde_json::json!({"task_id": task_id, "status": status}))
}

/// Keyword-pick → content tasks: same creation path as the Tauri command/// (validates picks against the research artifact, marks research done).
fn create_articles_from_keywords(db_path: &str, project_id: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let research_task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let keywords_raw = flag(args, "--keywords", "-k")
        .unwrap_or_else(|| exit("--keywords required (comma-separated)"));
    let keywords: Vec<String> = keywords_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let conn = open_db(db_path)?;
    let tasks = pageseeds_core::engine::keyword_selection::create_article_tasks_from_keywords(
        &conn,
        project_id,
        &research_task_id,
        keywords,
    )?;
    Ok(serde_json::json!({
        "created": tasks.len(),
        "task_ids": tasks.iter().map(|t| &t.id).collect::<Vec<_>>(),
        "titles": tasks.iter().map(|t| &t.title).collect::<Vec<_>>(),
    }))
}

fn create_reddit_replies(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let task_id = flag(args, "--task-id", "-I").unwrap_or_else(|| exit("--task-id required"));
    let post_ids = flag(args, "--post-ids", "-P")
        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect::<Vec<_>>())
        .unwrap_or_else(|| exit("--post-ids required (comma-separated)"));
    let conn = open_db(db_path)?;
    let tasks = pageseeds_core::reddit::spawner::create_reply_tasks_from_opportunities(&conn, &task_id, &post_ids)?;
    Ok(serde_json::json!({
        "created": tasks.len(),
        "task_ids": tasks.iter().map(|t| &t.id).collect::<Vec<_>>(),
        "titles": tasks.iter().map(|t| &t.title).collect::<Vec<_>>(),
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Cannibalization strategy workflow
// ═══════════════════════════════════════════════════════════════════════════════

fn cannibalization_strategy(db_path: &str, project_id: &str) -> Result<serde_json::Value, String> {
    let conn = open_db(db_path)?;
    let strategy = pageseeds_core::cannibalization::get_strategy_with_reviews(&conn, project_id)
        .map_err(|e| e.to_string())?;
    match strategy {
        Some(s) => Ok(serde_json::to_value(s).unwrap_or_default()),
        None => Ok(serde_json::json!({"message": "no strategy found"})),
    }
}

fn set_review_status(db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let strategy_id = flag(args, "--strategy-id", "-S").unwrap_or_else(|| exit("--strategy-id required"));
    let project_id = flag(args, "--project-id", "-i").unwrap_or_else(|| exit("--project-id required"));
    let rec_type = flag(args, "--recommendation-type", "-T").unwrap_or_else(|| exit("--recommendation-type required"));
    let rec_id = flag(args, "--recommendation-id", "-I").unwrap_or_else(|| exit("--recommendation-id required"));
    let status = flag(args, "--status", "-s").unwrap_or_else(|| exit("--status required"));
    let notes = flag(args, "--notes", "-n");

    let status_enum = match status.as_str() {
        "approved" => ApprovalStatus::Approved,
        "rejected" => ApprovalStatus::Rejected,
        "needs_review" => ApprovalStatus::NeedsReview,
        _ => ApprovalStatus::Pending,
    };

    let conn = open_db(db_path)?;
    let review = pageseeds_core::db::set_strategy_review(
        &conn,
        &strategy_id,
        &project_id,
        &rec_type,
        &rec_id,
        status_enum,
        None,
        notes.as_deref(),
    ).map_err(|e| e.to_string())?;

    Ok(serde_json::to_value(review).unwrap_or_default())
}

fn create_tasks_from_approved(db_path: &str, project_id: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let strategy_id = flag(args, "--strategy-id", "-S")
        .unwrap_or_else(|| exit("--strategy-id required (use 'latest' to resolve from project)"));
    let strategy_id = if strategy_id == "latest" {
        pageseeds_core::cannibalization::resolve_strategy_id(
            &open_db(db_path)?,
            project_id,
        ).map_err(|e| e.to_string())?
    } else {
        strategy_id
    };

    let conn = open_db(db_path)?;
    let created = pageseeds_core::cannibalization::spawn_tasks_from_approved(&conn, &strategy_id, project_id)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "strategy_id": strategy_id,
        "created_tasks": created.len(),
        "task_ids": created,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dead-weight remediation (WS4)
// ═══════════════════════════════════════════════════════════════════════════════

fn score_zero_impression_articles(db_path: &str, project_id: &str, project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    use pageseeds_core::seo::dead_weight::{
        list_from_cache, score_and_persist_with_provider, ScoreOptions, DEFAULT_MAX_IMPRESSIONS,
        DEFAULT_MAX_LIVE_SCORES, DEFAULT_SCORE_TTL_DAYS,
    };

    let conn = open_db(db_path)?;
    let from_cache = has_flag(args, "--from-cache", "") || has_flag(args, "--list", "");

    if from_cache {
        let result = list_from_cache(&conn, project_id).map_err(|e| e.to_string())?;
        return serde_json::to_value(result).map_err(|e| e.to_string());
    }

    let force = has_flag(args, "--force", "");
    let max_impressions: f64 = flag(args, "--max-impressions", "-m")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_IMPRESSIONS);
    let max_live: usize = flag(args, "--max", "")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_LIVE_SCORES);
    let ttl_days: u64 = flag(args, "--ttl-days", "")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SCORE_TTL_DAYS);
    let country = flag(args, "--country", "").unwrap_or_else(|| "us".to_string());

    let project = pageseeds_core::engine::task_store::get_project(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let provider_name = project.seo_provider.as_deref().unwrap_or("dataforseo");
    let env = pageseeds_core::config::env_resolver::EnvResolver::new(project_path);
    let provider =
        pageseeds_core::seo::resolve_provider(provider_name, &env).map_err(|e| e.to_string())?;

    let opts = ScoreOptions {
        max_impressions,
        force,
        ttl_days,
        max_live,
        country,
    };

    // Sync score loop; SERP is driven via Handle::block_on inside assess_fn.
    // Do not wrap the whole call in rt.block_on (nested block_on panics).
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let result = score_and_persist_with_provider(
        &conn,
        project_id,
        provider.as_ref(),
        &opts,
        rt.handle(),
    )
    .map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Remaining inline functions (not yet extracted as shared — small or async)
// ═══════════════════════════════════════════════════════════════════════════════

fn article_frontmatter(project_path: &str, slug: &str) -> Result<serde_json::Value, String> {
    // Shared resolver: direct path, then NN_slug.mdx / normalized stem / frontmatter.
    let fp = pageseeds_core::content::ops::resolve_slug_or_path(
        std::path::Path::new(project_path),
        slug,
    )?;
    let meta = pageseeds_core::content::ops::read_file_metadata(&fp).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "slug": meta.url_slug, "file": meta.file_name,
        "title": meta.title, "published_date": meta.published_date,
        "status": meta.status, "word_count": meta.word_count,
    }))
}

fn run_audit(project_id: &str, project_path: &str) -> Result<serde_json::Value, String> {
    use pageseeds_core::models::task::*;
    let task = pageseeds_core::models::task::Task {
        id: "cli-audit".into(), task_type: "content_audit".into(),
        project_id: project_id.to_string(), title: Some("CLI content audit".into()),
        description: None, status: TaskStatus::InProgress, phase: "audit".into(),
        priority: Priority::Medium,
        created_at: chrono::Utc::now().to_rfc3339(), updated_at: chrono::Utc::now().to_rfc3339(),
        not_before: None, run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None, follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::None, depends_on: vec![], artifacts: vec![],
        run: Default::default(),
    };
    let result = pageseeds_core::engine::exec::content_audit::exec_content_audit(&task, project_path);
    if !result.success { return Err(result.message); }
    serde_json::from_str(result.output.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())
}

fn ctr_health(project_id: &str, project_path: &str, db_path: &str) -> Result<serde_json::Value, String> {
    let conn = open_db(db_path)?;
    let articles = pageseeds_core::engine::task_store::list_articles(&conn, project_id).map_err(|e| e.to_string())?;
    let summary = pageseeds_core::content::ops::build_ctr_health_summary(
        std::path::Path::new(project_path), &articles, 0, 0, &conn, project_id,
    );
    Ok(serde_json::to_value(summary).unwrap_or_default())
}

/// Build create-task success JSON, optionally including off-mode `warning`.
fn create_task_success_json(
    project_path: &str,
    task_id: &str,
    task_type: &str,
    title: impl serde::Serialize,
    status: impl serde::Serialize,
) -> serde_json::Value {
    let mut v = serde_json::json!({
        "task_id": task_id,
        "task_type": task_type,
        "title": title,
        "status": status,
    });
    if let Some(warning) = pageseeds_core::models::seo_program::off_mode_create_warning(
        std::path::Path::new(project_path),
        task_type,
    ) {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("warning".into(), serde_json::Value::String(warning));
        }
    }
    v
}

fn create_task(
    project_id: &str,
    db_path: &str,
    project_path: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    let tt = flag(args, "--task-type", "-t").unwrap_or_default();
    let title = flag(args, "--title", "-T").unwrap_or_default();
    let reason = flag(args, "--reason", "-r").unwrap_or_default();
    let priority = flag(args, "--priority", "-P").unwrap_or_else(|| "medium".to_string());
    let auto_enqueue = has_flag(args, "--auto-enqueue", "-a");
    let slug = flag(args, "--slug", "-S");

    let priority_enum = match priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let conn = open_db(db_path)?;

    // fix_content_article always goes through the shared slug helper so the
    // recommendations_{article_id} artifact (SERP categories) is attached.
    if tt == "fix_content_article" {
        let slug_val = slug.ok_or_else(|| {
            "--slug required for fix_content_article (url slug of the article to fix)".to_string()
        })?;
        let task = pageseeds_core::engine::content_fix::spawn_fix_content_article_for_slug(
            &conn,
            project_id,
            &slug_val,
            &reason,
            pageseeds_core::engine::content_fix::SpawnFixForSlugOpts {
                title: if title.is_empty() { None } else { Some(title) },
                priority: priority_enum,
                auto_enqueue,
                source: "pageseeds-cli".to_string(),
            },
        )
        .map_err(|e| e.to_string())?;
        return Ok(create_task_success_json(
            project_path,
            &task.id,
            &tt,
            task.title,
            task.status,
        ));
    }

    // fix_ctr_article always attaches a single-article ctr_context (GSC + file
    // excerpt), matching audit-spawned children. Bare TaskSpawner creates omit
    // the artifact and cause analyze to fail.
    if tt == "fix_ctr_article" {
        let slug_val = slug.ok_or_else(|| {
            "--slug required for fix_ctr_article (url slug of the article to fix)".to_string()
        })?;
        let task = pageseeds_core::engine::ctr_fix::spawn_fix_ctr_article_for_slug(
            &conn,
            project_id,
            project_path,
            &slug_val,
            pageseeds_core::engine::ctr_fix::SpawnFixCtrForSlugOpts {
                title: if title.is_empty() { None } else { Some(title) },
                priority: priority_enum,
                auto_enqueue,
                source: "pageseeds-cli".to_string(),
                reason: if reason.is_empty() {
                    None
                } else {
                    Some(reason)
                },
            },
        )
        .map_err(|e| e.to_string())?;
        return Ok(create_task_success_json(
            project_path,
            &task.id,
            &tt,
            task.title,
            task.status,
        ));
    }

    // fix_indexing_internal_links always attaches indexing_link_target (IHC child
    // shape). Bare TaskSpawner creates omit the artifact and fail at context.
    if tt == "fix_indexing_internal_links" {
        let slug_val = slug.ok_or_else(|| {
            "--slug required for fix_indexing_internal_links (url slug of the article to add inbound links for)".to_string()
        })?;
        let task = pageseeds_core::engine::indexing_link_fix::spawn_fix_indexing_internal_links_for_slug(
            &conn,
            project_id,
            project_path,
            &slug_val,
            pageseeds_core::engine::indexing_link_fix::SpawnFixIndexingLinksForSlugOpts {
                title: if title.is_empty() { None } else { Some(title) },
                priority: priority_enum,
                auto_enqueue,
                source: "pageseeds-cli".to_string(),
                reason: if reason.is_empty() {
                    None
                } else {
                    Some(reason)
                },
            },
        )
        .map_err(|e| e.to_string())?;
        return Ok(create_task_success_json(
            project_path,
            &task.id,
            &tt,
            task.title,
            task.status,
        ));
    }

    let task = pageseeds_core::engine::spawner::TaskSpawner::spawn(&conn, pageseeds_core::engine::spawner::TaskSpec {
        project_id: project_id.to_string(), task_type: tt.clone(),
        title: Some(title.clone()), description: Some(reason),
        priority: priority_enum,
        run_policy: if auto_enqueue { Some(TaskRunPolicy::AutoEnqueue) } else { None },
        ..Default::default()
    }).map_err(|e| e.to_string())?;
    Ok(create_task_success_json(
        project_path,
        &task.id,
        &tt,
        title,
        task.status,
    ))
}

fn write_spec(project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let title = flag(args, "--issue-title", "-T").unwrap_or_default();
    let sev = flag(args, "--severity", "-s").unwrap_or_else(|| "warning".into());
    let impact = flag(args, "--impact", "-m").unwrap_or_default();
    let file = flag(args, "--file-to-edit", "-f").unwrap_or_default();
    let current = flag(args, "--current-code", "-c").unwrap_or_default();
    let fixed = flag(args, "--fixed-code", "-F").unwrap_or_default();
    let notes = flag(args, "--notes", "-n");
    let paths = pageseeds_core::engine::project_paths::ProjectPaths::from_path(project_path);
    let spec = paths.automation_dir.join("seo_feature_spec.md");
    let header = if spec.exists() { String::new() } else {
        format!("# SEO Feature Specification\n\nGenerated by PageSeeds on {}\n\n", chrono::Utc::now().format("%Y-%m-%d"))
    };
    let existing = if spec.exists() { std::fs::read_to_string(&spec).unwrap_or_default() } else { String::new() };
    let ns = notes.map(|n| format!("\n**Notes:** {n}\n")).unwrap_or_default();
    let section = format!("\n---\n\n## {title}\n\n**Severity:** {sev} | **Impact:** {impact}\n**File:** `{file}`\n\n**Current:**\n```\n{current}\n```\n\n**Fixed:**\n```\n{fixed}\n```{ns}");
    std::fs::write(&spec, format!("{header}{existing}{section}")).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"path": spec.to_string_lossy().to_string(), "issue": title}))
}

/// Compare source frontmatter titles with what Google actually sees (live HTML).
fn compare_rendered(project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let max: usize = flag(args, "--max", "-m").and_then(|s| s.parse().ok()).unwrap_or(25);
    pageseeds_core::engine::exec::ctr_audit::rendered::compare_rendered_titles(project_path, max)
}

// ── GSC (async — kept inline since they need tokio runtime) ──────────────────

fn gsc_limit(args: &[String], default: u32) -> u32 {
    flag(args, "--limit", "-l")
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
        .min(200)
}

fn gsc_perf(project_id: &str, project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site, token) = rt.block_on(gsc_token(project_id, project_path))?;
    let end = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let start = (chrono::Utc::now() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
    let limit = gsc_limit(args, 50);
    let m = rt.block_on(pageseeds_core::gsc::analytics::fetch_page_rows(
        &token, &site, &start, &end, limit,
    ))
    .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(m).unwrap_or_default())
}

fn gsc_q(
    project_id: &str,
    project_path: &str,
    page: Option<String>,
    args: &[String],
) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site, token) = rt.block_on(gsc_token(project_id, project_path))?;
    let end = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let start = (chrono::Utc::now() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
    let limit = gsc_limit(args, 50);
    if let Some(url) = page {
        let m = rt.block_on(pageseeds_core::gsc::analytics::fetch_queries_for_page(
            &token, &site, &url, &start, &end, limit,
        ))
        .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    } else {
        let m = rt.block_on(pageseeds_core::gsc::analytics::fetch_page_query_rows(
            &token, &site, &start, &end, limit,
        ))
        .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    }
}

fn gsc_mov(project_id: &str, project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site, token) = rt.block_on(gsc_token(project_id, project_path))?;
    let now = chrono::Utc::now();
    let ce = now.format("%Y-%m-%d").to_string();
    let cs = (now - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
    let pe = (now - chrono::Duration::days(31)).format("%Y-%m-%d").to_string();
    let ps = (now - chrono::Duration::days(61)).format("%Y-%m-%d").to_string();
    let limit = gsc_limit(args, 30);
    let m = rt.block_on(pageseeds_core::gsc::analytics::compute_movers(
        &token, &site, &cs, &ce, &ps, &pe, limit,
    ))
    .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(m).unwrap_or_default())
}

async fn gsc_token(project_id: &str, project_path: &str) -> Result<(String, String), String> {
    let resolver = pageseeds_core::config::env_resolver::EnvResolver::new(project_path);
    let sa = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH").map(|(v, _)| v)
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        .ok_or_else(|| "GSC not connected".to_string())?;
    let token = pageseeds_core::gsc::auth::get_service_account_token(&sa).await.map_err(|e| e.to_string())?;
    let conn = open_db(&pageseeds_core::db::default_db_path().to_string_lossy())?;
    let project = pageseeds_core::engine::task_store::get_project(&conn, project_id).map_err(|e| e.to_string())?;
    let site = project.site_url.unwrap_or_default();
    if site.is_empty() { return Err("No site_url configured".into()); }
    Ok((site, token.access_token))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Open the production DB through the canonical migrated open (`db::init`).
/// Raw `Connection::open` skips migrations, which lets the schema drift behind
/// the app and silently breaks writes (see issue #71).
fn open_db(db_path: &str) -> Result<rusqlite::Connection, String> {
    pageseeds_core::db::init(std::path::Path::new(db_path)).map_err(|e| e.to_string())
}

fn flag(args: &[String], long: &str, short: &str) -> Option<String> {
    for i in 0..args.len() {
        let matches_long = !long.is_empty() && args[i] == long;
        let matches_short = !short.is_empty() && args[i] == short;
        if matches_long || matches_short {
            if i + 1 < args.len() {
                return Some(args[i + 1].clone());
            }
        }
    }
    None
}

fn has_flag(args: &[String], long: &str, short: &str) -> bool {
    args.iter().any(|a| {
        (!long.is_empty() && a == long) || (!short.is_empty() && a == short)
    })
}

/// Hard-error path: `ERROR: …` on stderr, empty stdout, exit 1.
/// License deny must use this same shape with a buy URL in the message.
fn exit(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    std::process::exit(1);
}

/// Free meta: one-off consolidation of site URLs into `projects.site_url`.
///
/// Gathers candidates from the projects table, manifest.json, and
/// seo_workspace.json; picks a winner (prefer `sc-domain:…`); writes the DB.
/// Idempotent. Optional `-i <project-id>` limits to one project.
fn run_sync_site_urls(args: &[String]) {
    let project_id = flag(args, "--project-id", "-i");
    let db = pageseeds_core::db::default_db_path();
    let conn = match open_db(&db.to_string_lossy()) {
        Ok(c) => c,
        Err(e) => exit(&format!("failed to open project database: {e}")),
    };

    let payload = if let Some(id) = project_id {
        match pageseeds_core::engine::site_url_sync::sync_site_url_for_id(&conn, &id) {
            Ok(row) => serde_json::json!({
                "mode": "single",
                "projects_scanned": 1,
                "projects_updated": if row.changed { 1 } else { 0 },
                "projects_already_ok": if !row.changed && row.site_url.is_some() { 1 } else { 0 },
                "projects_missing": if row.site_url.is_none() { 1 } else { 0 },
                "results": [row],
            }),
            Err(e) => exit(&format!("sync-site-urls failed: {e}")),
        }
    } else {
        match pageseeds_core::engine::site_url_sync::sync_all_site_urls(&conn) {
            Ok(report) => serde_json::to_value(report).unwrap_or_default(),
            Err(e) => exit(&format!("sync-site-urls failed: {e}")),
        }
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );
}

/// Free meta: list registered projects as JSON (same DB as desktop).
fn run_list_projects() {
    let db = pageseeds_core::db::default_db_path();
    let conn = match open_db(&db.to_string_lossy()) {
        Ok(c) => c,
        Err(e) => exit(&format!("failed to open project database: {e}")),
    };
    match cli_setup::list_projects(&conn) {
        Ok(outcome) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&outcome).unwrap_or_default()
            );
        }
        Err(e) => exit(&format!("failed to list projects: {e}")),
    }
}

/// Free meta: create/link a workspace project via the shared helper.
fn run_create_project(args: &[String]) {
    let path = flag(args, "--path", "").or_else(|| flag(args, "--project-path", "-p"));
    let name = flag(args, "--name", "-n");
    let site_url = flag(args, "--site-url", "");
    let cwd = std::env::current_dir().ok();

    let db = pageseeds_core::db::default_db_path();
    let conn = match open_db(&db.to_string_lossy()) {
        Ok(c) => c,
        Err(e) => exit(&format!("failed to open project database: {e}")),
    };
    match cli_setup::create_project(
        &conn,
        cli_setup::CreateProjectOpts {
            path,
            name,
            site_url,
            cwd,
        },
    ) {
        Ok(outcome) => {
            let payload = serde_json::json!({
                "created": outcome.created,
                "project": outcome.project,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
        }
        Err(e) => exit(&e.to_string()),
    }
}

/// Free meta: one-shot onboarding — link/create, write defaults, optional first-win desk read.
fn run_setup(args: &[String]) {
    let json_mode = has_flag(args, "--json", "");
    let status_only = has_flag(args, "--status", "");
    let skip_first_win = has_flag(args, "--skip-first-win", "");
    let _yes = has_flag(args, "--yes", "-y"); // reserved for future interactive prompts

    let path = flag(args, "--path", "").or_else(|| flag(args, "--project-path", "-p"));
    let name = flag(args, "--name", "-n");
    let site_url = flag(args, "--site-url", "");
    let license_key = flag(args, "--license", "").or_else(|| {
        std::env::var("PAGESEEDS_LICENSE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    let cwd = std::env::current_dir().ok();

    if status_only {
        run_setup_status(path, json_mode, cwd);
        return;
    }

    let db = pageseeds_core::db::default_db_path();
    let conn = match open_db(&db.to_string_lossy()) {
        Ok(c) => c,
        Err(e) => exit(&format!("failed to open project database: {e}")),
    };
    let outcome = match cli_setup::setup(
        &conn,
        cli_setup::SetupOpts {
            path,
            name,
            site_url,
            license_key,
            skip_first_win,
            cwd,
        },
    ) {
        Ok(o) => o,
        Err(e) => exit(&e.to_string()),
    };

    let payload = serde_json::to_value(&outcome).unwrap_or_default();
    if !json_mode {
        for line in outcome.human_progress_lines() {
            eprintln!("{line}");
        }
    }
    // Machine-readable summary on stdout (agents pipe setup; --json skips human stderr only).
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    );
}

/// Report-only readiness check (no mutations). Exit 1 when not desk-ready.
fn run_setup_status(path: Option<String>, json_mode: bool, cwd: Option<std::path::PathBuf>) {
    let db = pageseeds_core::db::default_db_path();
    let conn = open_db(&db.to_string_lossy()).ok();
    let outcome = match cli_setup::setup_status(
        conn.as_ref(),
        cli_setup::SetupStatusOpts { path, cwd },
    ) {
        Ok(o) => o,
        Err(e) => exit(&e.to_string()),
    };

    // Always print JSON for --status so agents can parse; human notes on stderr.
    if !json_mode {
        if outcome.desk_ready {
            eprintln!("setup status: desk-ready");
        } else {
            eprintln!("setup status: not desk-ready — run pageseeds-cli setup --path . --yes");
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&outcome).unwrap_or_default()
    );
    if !outcome.desk_ready {
        std::process::exit(1);
    }
}

/// `pageseeds-cli license activate|status|deactivate` — free, no -i/-p, no DB.
fn run_license_command(args: &[String]) {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
    match sub {
        "activate" => {
            let key = args.get(3).map(|s| s.as_str()).unwrap_or("");
            if key.is_empty() {
                exit("usage: pageseeds-cli license activate <key>");
            }
            match pageseeds_core::license::activate(key) {
                Ok(()) => {
                    let st = pageseeds_core::license::status();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&st).unwrap_or_else(|_| r#"{"status":"valid"}"#.into())
                    );
                }
                Err(e) => exit(&format!("license activate failed: {e}")),
            }
        }
        "status" => {
            let st = pageseeds_core::license::status();
            println!(
                "{}",
                serde_json::to_string_pretty(&st).unwrap_or_else(|_| r#"{"status":"invalid"}"#.into())
            );
        }
        "deactivate" => match pageseeds_core::license::deactivate() {
            Ok(()) => {
                println!(r#"{{"status":"missing","message":"license removed from local store"}}"#);
            }
            Err(e) => exit(&format!("license deactivate failed: {e}")),
        },
        "" | "help" => {
            println!(
                r#"pageseeds-cli license — offline JWT license management

Usage:
  pageseeds-cli license activate <key>
  pageseeds-cli license status
  pageseeds-cli license deactivate

Notes:
  - Free commands (desk, GSC reads, inspect) never require a license
  - Paid commands require a valid non-expired JWT (plan=cli, RS256)
  - Store: $PAGESEEDS_LICENSE_PATH or ~/.config/pageseeds/license.jwt
  - No phone-home; deactivate only deletes the local file
  - Buy: https://pageseeds.com
"#
            );
        }
        other => exit(&format!(
            "unknown license subcommand '{other}'. Use: activate | status | deactivate"
        )),
    }
}

/// True when the invocation should print help and exit 0 (no tool dispatch).
fn wants_help(args: &[String]) -> bool {
    if args.len() < 2 {
        return true;
    }
    if args[1] == "help" {
        return true;
    }
    // -h / --help anywhere after argv[0], including `tool --help`
    args[1..].iter().any(|a| a == "-h" || a == "--help")
}

/// One help row for a dispatched tool. Keep in sync with the `match` arms in `main`.
struct ToolHelp {
    name: &'static str,
    purpose: &'static str,
    example: &'static str,
    section: &'static str,
}

/// Complete inventory of match-arm tools. New tools must be added here and in `main`.
const TOOLS: &[ToolHelp] = &[
    // Meta / onboarding (free)
    ToolHelp {
        name: "list-projects",
        purpose: "List registered projects as JSON (no -i/-p)",
        example: "list-projects",
        section: "Meta",
    },
    ToolHelp {
        name: "create-project",
        purpose: "Register a workspace project (shared helper / same DB as desktop)",
        example: "create-project --path . --name \"My Site\"",
        section: "Meta",
    },
    ToolHelp {
        name: "setup",
        purpose: "Link/create project, write defaults, first desk win (idempotent)",
        example: "setup --path . --yes [--skip-first-win] [--status] [--license KEY]",
        section: "Meta",
    },
    ToolHelp {
        name: "sync-site-urls",
        purpose: "One-off: gather site_url from manifest/workspace/DB → write projects.site_url",
        example: "sync-site-urls [-i <project-id>]",
        section: "Meta",
    },
    // GSC
    ToolHelp {
        name: "gsc-performance",
        purpose: "GSC page performance rows (last 90d)",
        example: "gsc-performance -i <id> -p <path> [-l N]",
        section: "GSC",
    },
    ToolHelp {
        name: "gsc-queries",
        purpose: "GSC queries (site-wide or -u page URL)",
        example: "gsc-queries -i <id> -p <path> [-u <page-url>] [-l N]",
        section: "GSC",
    },
    ToolHelp {
        name: "gsc-movers",
        purpose: "GSC page movers (recent vs prior 30d)",
        example: "gsc-movers -i <id> -p <path> [-l N]",
        section: "GSC",
    },
    // Task / queue
    ToolHelp {
        name: "list-tasks",
        purpose: "List tasks (optional type/status filter)",
        example: "list-tasks -i <id> [-t type] [-s status]",
        section: "Task / queue",
    },
    ToolHelp {
        name: "cancel-tasks",
        purpose: "Cancel matching tasks (requires --yes)",
        example: "cancel-tasks -i <id> -t type [-s status] --yes",
        section: "Task / queue",
    },
    ToolHelp {
        name: "create-task",
        purpose: "Create a task (fix_content/ctr/indexing_links need -S slug)",
        example: "create-task -i <id> -p <path> -t fix_content_article -S <slug>",
        section: "Task / queue",
    },
    ToolHelp {
        name: "execute-task",
        purpose: "Run one task by id; --force overrides not_before gate",
        example: "execute-task -I <task-id> [--force]",
        section: "Task / queue",
    },
    ToolHelp {
        name: "get-task",
        purpose: "Full task JSON including artifacts",
        example: "get-task -I <task-id>",
        section: "Task / queue",
    },
    ToolHelp {
        name: "update-task-status",
        purpose: "Close artifact-review tasks (done|cancelled)",
        example: "update-task-status -I <task-id> -s done",
        section: "Task / queue",
    },
    ToolHelp {
        name: "select-keywords",
        purpose: "Create content tasks from research picks",
        example: "select-keywords -I <research-task-id> -K kw1,kw2",
        section: "Task / queue",
    },
    ToolHelp {
        name: "write-context",
        purpose: "Path B write package (brief/path/skill; no LLM). -I optional when -K is project.yaml Primary/problem",
        example: "write-context -i <id> -p <path> -K \"per-leg P&L options\"  |  write-context -I <research-id> -K <kw>",
        section: "Path B write",
    },
    ToolHelp {
        name: "write-submit",
        purpose: "Path B validate+ingest MDX (ok:false still exit 0)",
        example: "write-submit -i <id> -p <path> -f <mdx> [-I write-task-id] [-K keyword]",
        section: "Path B write",
    },
    ToolHelp {
        name: "publish-content",
        purpose: "Catalog draft/ready → published (preflight+apply; explicit second step)",
        example: "publish-content -i <id> -p <path> -S <slug>[,slug2,...]",
        section: "Path B write",
    },
    ToolHelp {
        name: "research-context",
        purpose: "Path B research strategy package; refreshes shortlist when empty/stale",
        example: "research-context -i <id>",
        section: "Path B research",
    },
    ToolHelp {
        name: "strategy",
        purpose: "Project content strategy as JSON (project.yaml via ensure; auto-migrates MD)",
        example: "strategy -i <id> -p <path>",
        section: "Path B research",
    },
    ToolHelp {
        name: "project-config-status",
        purpose: "Read-only: project.yaml vs legacy MD readiness as JSON",
        example: "project-config-status -p <path>",
        section: "Path B research",
    },
    ToolHelp {
        name: "migrate-project-config",
        purpose: "Deterministic MD→project.yaml migrator (no LLM)",
        example: "migrate-project-config -p <path> [--dry-run] [--force]",
        section: "Path B research",
    },
    ToolHelp {
        name: "research-pull",
        purpose: "Path B seeds → custom_keyword_research (create+execute)",
        example: "research-pull -i <id> -K seed1,seed2 [--no-execute]",
        section: "Path B research",
    },
    ToolHelp {
        name: "merge-context",
        purpose: "Path B merge package with full MDX bodies (no LLM)",
        example: "merge-context -i <id> -p <path> -I <consolidate-task-id>",
        section: "Path B merge",
    },
    ToolHelp {
        name: "merge-submit",
        purpose: "Path B apply merge (ok:false validation still exit 0)",
        example: "merge-submit -i <id> -p <path> -I <consolidate-task-id> [-y]",
        section: "Path B merge",
    },
    ToolHelp {
        name: "select-content-review",
        purpose: "Spawn fix_content_article from content_review picks",
        example: "select-content-review -I <review-task-id> -P id1,id2",
        section: "Task / queue",
    },
    ToolHelp {
        name: "select-cannibalization",
        purpose: "Spawn cannibalization fixes from parent picks",
        example: "select-cannibalization -I <parent-task-id> -S type:id,...",
        section: "Task / queue",
    },
    ToolHelp {
        name: "create-articles-from-keywords",
        purpose: "Create article tasks from keyword list",
        example: "create-articles-from-keywords -i <id> -I <research-task-id> -k \"kw1, kw2\"",
        section: "Task / queue",
    },
    ToolHelp {
        name: "set-task-status",
        purpose: "Set any task status string",
        example: "set-task-status -I <task-id> -s done",
        section: "Task / queue",
    },
    ToolHelp {
        name: "create-reddit-replies",
        purpose: "Create draft_reddit_reply tasks for post ids",
        example: "create-reddit-replies -I <parent-task-id> -P post1,post2",
        section: "Task / queue",
    },
    // Cannibalization
    ToolHelp {
        name: "cannibalization-strategy",
        purpose: "Load strategy + review rows for a project",
        example: "cannibalization-strategy -i <id>",
        section: "Cannibalization",
    },
    ToolHelp {
        name: "set-review-status",
        purpose: "Approve/reject a cannibalization recommendation",
        example: "set-review-status -i <id> -S <strategy-id> -T <type> -I <rec-id> -s approved",
        section: "Cannibalization",
    },
    ToolHelp {
        name: "create-tasks-from-approved",
        purpose: "Spawn tasks from approved recommendations",
        example: "create-tasks-from-approved -i <id> -S latest",
        section: "Cannibalization",
    },
    // Dead-weight
    ToolHelp {
        name: "score-zero-impression-articles",
        purpose: "Score low/zero-impression articles; persist winnability; --from-cache/--list lists without SERP",
        example: "score-zero-impression-articles -i <id> -p <path> [--from-cache|--list] [--force] [-m <max-impr>] [--max <N>] [--ttl-days <N>]",
        section: "Dead-weight",
    },
    // Site State desk
    ToolHelp {
        name: "site-overview",
        purpose: "Site health desk (totals, top pages, movers, outcomes)",
        example: "site-overview -i <id> -p <path> [-d period-days]",
        section: "Site State desk",
    },
    ToolHelp {
        name: "ctr-outcomes",
        purpose: "CTR closed-loop: verify deployments, classify ready outcomes, report rollup",
        example: "ctr-outcomes -i <id>",
        section: "Site State desk",
    },
    ToolHelp {
        name: "articles",
        purpose: "Article catalog with GSC filters",
        example: "articles -i <id> -p <path> [-s status] [-m min-imp] [-l N] [-R]",
        section: "Site State desk",
    },
    ToolHelp {
        name: "article",
        purpose: "Full package for one article slug",
        example: "article -i <id> -p <path> -S <slug> [-d period-days]",
        section: "Site State desk",
    },
    // Article inspect
    ToolHelp {
        name: "article-list",
        purpose: "Lightweight article list from DB",
        example: "article-list -i <id> [-s status]",
        section: "Article inspect",
    },
    ToolHelp {
        name: "article-frontmatter",
        purpose: "Parsed frontmatter for a slug",
        example: "article-frontmatter -p <path> -S <slug>",
        section: "Article inspect",
    },
    ToolHelp {
        name: "article-body-hash",
        purpose: "Body hashes for change detection",
        example: "article-body-hash -i <id> -p <path>",
        section: "Article inspect",
    },
    ToolHelp {
        name: "article-title-scan",
        purpose: "Scan titles across content dir",
        example: "article-title-scan -i <id> -p <path>",
        section: "Article inspect",
    },
    ToolHelp {
        name: "validate-article",
        purpose: "Validate one article (floors / structure)",
        example: "validate-article -i <id> -p <path> -S <slug>",
        section: "Article inspect",
    },
    // Path B fix
    ToolHelp {
        name: "fix-context",
        purpose: "Path B fix package (file + queries + skill; no generate)",
        example: "fix-context -i <id> -p <path> -S <slug> -k content|ctr|refresh [-g goals]",
        section: "Path B fix",
    },
    ToolHelp {
        name: "fix-submit",
        purpose: "Path B apply patch and/or validate on-disk MDX (-K retargets keyword)",
        example: "fix-submit -i <id> -p <path> -S <slug> -k content|ctr|refresh [--patch <json>] [--file <mdx>] [-K <keyword>]",
        section: "Path B fix",
    },
    // Audits / reports
    ToolHelp {
        name: "content-audit-report",
        purpose: "Read last content audit report from repo",
        example: "content-audit-report -p <path>",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "run-content-audit",
        purpose: "Run content audit and print result JSON",
        example: "run-content-audit -i <id> -p <path>",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "cannibalization-clusters",
        purpose: "Read cannibalization cluster JSON from repo",
        example: "cannibalization-clusters -p <path>",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "indexing-status",
        purpose: "Indexing health snapshot",
        example: "indexing-status -i <id> -p <path>",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "ctr-health",
        purpose: "CTR health summary across articles",
        example: "ctr-health -i <id> -p <path>",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "framework-files",
        purpose: "Read framework / skill files from project",
        example: "framework-files -p <path> [-f <relative-file>]",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "article-link-graph",
        purpose: "Internal /blog/ link graph scan",
        example: "article-link-graph -i <id> -p <path>",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "research-shortlist",
        purpose: "List research shortlist rows",
        example: "research-shortlist -i <id> [-s pending|researched|covered] [-H health]",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "article-quality-reviews",
        purpose: "Recent article quality review rows",
        example: "article-quality-reviews -i <id> [-l N]",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "compare-rendered",
        purpose: "Compare rendered titles vs frontmatter",
        example: "compare-rendered -p <path> [-m max]",
        section: "Audits / reports",
    },
    ToolHelp {
        name: "write-feature-spec",
        purpose: "Write a feature-spec markdown stub into the repo",
        example: "write-feature-spec -p <path> -T \"title\" [-s severity] [-m impact]",
        section: "Audits / reports",
    },
    // Video clips (context = free desk read; render = operator tier, dev-machine only)
    ToolHelp {
        name: "video-clip-context",
        purpose: "Article context JSON for the video-script skill (desk read)",
        example: "video-clip-context -i <id> -p <path> -S <slug>",
        section: "Video clips",
    },
    ToolHelp {
        name: "video-clip-render",
        purpose: "Render one clip definition via video-engine (operator tier — requires node/ffmpeg)",
        example: "video-clip-render -p <path> --clip <clip.json>",
        section: "Video clips",
    },
];

fn print_help() {
    // Help goes to stdout so agents can pipe it; contract documents this.
    println!(
        r#"pageseeds-cli — individual data tools for agents / KimiCode

Machine contract:
  Success payload:          single JSON value on stdout; exit 0
  Usage / domain hard error: "ERROR: …" on stderr; exit 1; stdout empty
  Outcome envelope:         JSON on stdout with ok/success fields; exit 0 even when
                            ok/success is false — caller inspects JSON (Path B write-submit,
                            merge-submit, etc.)
  License deny:             same as hard error (stderr + exit 1); message includes buy URL
  Help:                     -h/--help, help, no args, or <tool> --help → exit 0; text on stdout

Each subcommand calls one PageSeeds data function. Uses the same SQLite DB as the
desktop app. Prefer the installed binary from any directory.

Usage:
  pageseeds-cli setup --path . --yes          # once per machine/project
  pageseeds-cli <tool> [args]                # after setup, -i/-p optional
  pageseeds-cli <tool> -i <id> -p <path> …  # flags always override defaults
  pageseeds-cli list-projects | create-project | license …
  pageseeds-cli --version | -V
  # install: curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
  # dev: ./scripts/install-cli.sh  /  FROM_SOURCE=1  →  ~/.local/bin/pageseeds-cli

Project context (first match wins): -i/-p → env → .pageseeds.yaml → global config
  → project registry. Missing context: run `pageseeds-cli setup`.

License:
  Free tools (desk, GSC reads, inspect, setup/list/create) always work without a key.
  Paid tools (write/fix/merge, research-pull, task act, audits that write)
  require: pageseeds-cli license activate <key>
  Status:  pageseeds-cli license status
  Buy:     https://pageseeds.com
  Details: docs/CLI_COMMERCIAL.md
"#
    );

    let mut last_section = "";
    for t in TOOLS {
        if t.section != last_section {
            println!("{}:", t.section);
            last_section = t.section;
        }
        println!("  {:28} {}", t.name, t.purpose);
        println!("    ex: pageseeds-cli {}", t.example);
    }

    println!(
        r#"
Semver: flags and subcommand names are a breaking-change surface; call out renames
in release notes. Outcome JSON field shapes may evolve with a minor bump when
documented; silent renames of flags/tools are not allowed.
"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_inventory_lists_required_tools() {
        let names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        for required in [
            "research-context",
            "research-pull",
            "create-reddit-replies",
            "write-submit",
            "write-context",
            "publish-content",
            "fix-context",
            "fix-submit",
            "merge-context",
            "merge-submit",
            "site-overview",
            "ctr-outcomes",
            "articles",
            "article",
        ] {
            assert!(
                names.contains(&required),
                "TOOLS inventory missing required tool: {required}"
            );
        }
    }

    #[test]
    fn help_text_contains_key_tools_and_contract() {
        // Capture print_help by formatting the same sources (names + contract keywords).
        let joined = TOOLS
            .iter()
            .map(|t| t.name)
            .collect::<Vec<_>>()
            .join(" ");
        for needle in [
            "research-context",
            "research-pull",
            "create-reddit-replies",
            "write-submit",
            "fix-context",
        ] {
            assert!(joined.contains(needle), "help inventory missing {needle}");
        }
        // Ensure Path B tools have non-empty purpose + example.
        for t in TOOLS.iter().filter(|t| t.section.starts_with("Path B")) {
            assert!(!t.purpose.is_empty(), "{} missing purpose", t.name);
            assert!(!t.example.is_empty(), "{} missing example", t.name);
        }
    }

    #[test]
    fn wants_help_covers_top_level_and_per_tool() {
        let no_args = vec!["pageseeds-cli".into()];
        assert!(wants_help(&no_args));

        let top_h = vec!["pageseeds-cli".into(), "-h".into()];
        assert!(wants_help(&top_h));

        let top_help = vec!["pageseeds-cli".into(), "--help".into()];
        assert!(wants_help(&top_help));

        let bare = vec!["pageseeds-cli".into(), "help".into()];
        assert!(wants_help(&bare));

        let per_tool = vec![
            "pageseeds-cli".into(),
            "research-pull".into(),
            "--help".into(),
        ];
        assert!(wants_help(&per_tool));

        let real = vec![
            "pageseeds-cli".into(),
            "research-pull".into(),
            "-i".into(),
            "proj".into(),
        ];
        assert!(!wants_help(&real));
    }

    /// Free ∪ paid ∪ operator must cover the help inventory; every paid name must be a real
    /// tool. Prevents silently ungating a paid tool (or leaving a dead paid name) on rename/add.
    /// Operator-tier tools (docs/CLI_COMMERCIAL.md) sit outside the commercial free/paid
    /// boundary: no license, but excluded from the free count.
    #[test]
    fn free_paid_inventory_matches_tools() {
        let paid = pageseeds_core::license::paid_tools();
        let operator = pageseeds_core::license::operator_tools();
        let tool_names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();

        for name in paid {
            assert!(
                tool_names.contains(name),
                "paid tool '{name}' missing from TOOLS help inventory"
            );
        }
        for name in operator {
            assert!(
                tool_names.contains(name),
                "operator tool '{name}' missing from TOOLS help inventory"
            );
            assert!(
                !paid.contains(name),
                "operator tool '{name}' must not be in the paid set"
            );
        }

        let free_count = tool_names
            .iter()
            .filter(|n| {
                !pageseeds_core::license::requires_paid_license(n)
                    && !pageseeds_core::license::is_operator_tool(n)
            })
            .count();
        let paid_in_tools = tool_names
            .iter()
            .filter(|n| pageseeds_core::license::requires_paid_license(n))
            .count();

        assert_eq!(
            paid_in_tools,
            paid.len(),
            "paid set size must equal number of TOOLS names that require a license"
        );
        assert_eq!(
            TOOLS.len(),
            free_count + paid_in_tools + operator.len(),
            "every TOOLS entry must be free, paid, or operator (no double-count / gaps)"
        );
        assert_eq!(
            TOOLS.len(),
            free_count + paid.len() + operator.len(),
            "TOOLS.len() must equal free + paid + operator (paid ∪ operator ⊆ TOOLS)"
        );
        assert_eq!(
            TOOLS.len(),
            57,
            "TOOLS inventory size (free+paid commercial boundary + operator tier)"
        );
        assert_eq!(paid.len(), 25, "paid set size must match docs/CLI_COMMERCIAL.md");
        for meta in ["list-projects", "create-project", "setup", "sync-site-urls"] {
            assert!(
                tool_names.contains(&meta),
                "meta free tool '{meta}' missing from TOOLS"
            );
            assert!(
                !pageseeds_core::license::requires_paid_license(meta),
                "meta tool '{meta}' must be free"
            );
        }
    }
}
