/// PageSeeds CLI — individual data tools for KimiCode.
///
/// Each subcommand calls a shared standalone function from
/// engine/tools/investigate.rs (the same functions the Rig Tool impls use).
/// Zero business logic duplication.
///
/// Usage (preferred — installed binary, any cwd):
///   pageseeds-cli <tool> -i <project-id> -p <project-path> [args...]
/// Install: ./scripts/install-cli.sh  (from pageseeds-app checkout)
/// Dev only: cargo run --bin pageseeds-cli -- <tool> ...

use pageseeds_lib::engine::tools::{InvestigationContext, investigate};
use pageseeds_lib::models::cannibalization::ApprovalStatus;
use pageseeds_lib::models::task::{Priority, TaskRunPolicy, TaskStatus};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_help();
        return;
    }

    let tool = &args[1];
    let project_id = flag(&args, "--project-id", "-i").unwrap_or_default();
    let project_path = flag(&args, "--project-path", "-p").as_deref().map(expand_tilde);

    let db = pageseeds_lib::db::default_db_path();
    let ctx = InvestigationContext {
        project_id: project_id.clone(),
        project_path: project_path.clone().unwrap_or_default(),
        db_path: db.to_string_lossy().to_string(),
    };

    let require_project_path = || -> String {
        project_path.clone().unwrap_or_else(|| exit("--project-path required"))
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
    let mut tasks = pageseeds_lib::engine::task_store::list_tasks_light(&conn, project_id)
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

    let mut tasks = pageseeds_lib::engine::task_store::list_tasks_light(&conn, project_id)
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
        match pageseeds_lib::engine::task_store::update_task_status(&conn, id, TaskStatus::Cancelled) {
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
    let conn = open_db(db_path)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let result = rt.block_on(async {
        pageseeds_lib::engine::executor::execute_task_with_token(&conn, &task_id, None, None, false).await
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
    let task = pageseeds_lib::engine::task_store::get_task(&conn, &task_id)
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
    let task = pageseeds_lib::engine::task_store::update_task_status(&conn, &task_id, status)
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
    let research_task = pageseeds_lib::engine::task_store::get_task(&conn, &research_task_id)
        .map_err(|e| e.to_string())?;
    let project_id = research_task.project_id.clone();

    let tasks = pageseeds_lib::engine::keyword_selection::create_article_tasks_from_keywords(
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
    let tasks = pageseeds_lib::engine::content_review_selection::spawn_from_selection(
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
        selections.push(pageseeds_lib::models::cannibalization::CannibalizationSelection {
            recommendation_type: rec_type.to_string(),
            recommendation_id: rec_id.to_string(),
        });
    }

    let conn = open_db(db_path)?;
    let tasks = pageseeds_lib::cannibalization::spawn_tasks_from_selection(
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
    pageseeds_lib::engine::task_store::update_task_status(&conn, &task_id, status_enum)?;
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
    let tasks = pageseeds_lib::engine::keyword_selection::create_article_tasks_from_keywords(
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
    let tasks = pageseeds_lib::reddit::spawner::create_reply_tasks_from_opportunities(&conn, &task_id, &post_ids)?;
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
    let strategy = pageseeds_lib::cannibalization::get_strategy_with_reviews(&conn, project_id)
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
    let review = pageseeds_lib::db::set_strategy_review(
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
        pageseeds_lib::cannibalization::resolve_strategy_id(
            &open_db(db_path)?,
            project_id,
        ).map_err(|e| e.to_string())?
    } else {
        strategy_id
    };

    let conn = open_db(db_path)?;
    let created = pageseeds_lib::cannibalization::spawn_tasks_from_approved(&conn, &strategy_id, project_id)
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
    let conn = open_db(db_path)?;
    let project = pageseeds_lib::engine::task_store::get_project(&conn, project_id).map_err(|e| e.to_string())?;

    // Resolve SEO provider (DataForSEO by default).
    let provider_name = project.seo_provider.as_deref().unwrap_or("dataforseo");
    let env = pageseeds_lib::config::env_resolver::EnvResolver::new(project_path);
    let provider = pageseeds_lib::seo::resolve_provider(provider_name, &env).map_err(|e| e.to_string())?;

    // Load published articles with no GSC data or very low impressions (dead weight).
    let max_impressions: f64 = flag(args, "--max-impressions", "-m")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10.0);
    let mut stmt = conn.prepare(
        "SELECT a.id, a.title, a.url_slug, a.target_keyword, a.keyword_difficulty, a.status,
                COALESCE(json_extract(m.payload, '$.impressions'), 0) as impressions
         FROM articles a
         LEFT JOIN article_metadata m ON m.project_id = a.project_id AND m.article_id = a.id AND m.namespace = 'gsc'
         WHERE a.project_id = ?1 AND a.status = 'published'
           AND (m.article_id IS NULL OR json_extract(m.payload, '$.impressions') <= ?2)
         ORDER BY a.id"
    ).map_err(|e| e.to_string())?;

    let max_imp_str = max_impressions.to_string();
    let rows = stmt.query_map([project_id, max_imp_str.as_str()], |row| {
        Ok((
            row.get::<_, i64>("id")?,
            row.get::<_, String>("title")?,
            row.get::<_, String>("url_slug")?,
            row.get::<_, Option<String>>("target_keyword")?,
            row.get::<_, Option<String>>("keyword_difficulty")?,
            row.get::<_, f64>("impressions")?,
        ))
    }).map_err(|e| e.to_string())?;

    let articles: Vec<_> = rows.filter_map(|r| r.ok()).collect();

    // Filter to low/no-impression articles with a target keyword.
    let candidates: Vec<_> = articles
        .into_iter()
        .filter(|(_, _, _, kw, _, imp)| kw.is_some() && *imp <= max_impressions)
        .collect();

    if candidates.is_empty() {
        return Ok(serde_json::json!({
            "scored": 0,
            "message": "no low-impression published articles with target keywords found",
        }));
    }

    // Score each candidate via DataForSEO SERP API + winnability classifier.
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for (id, title, slug, keyword, kd_str, _) in candidates {
        let keyword = keyword.unwrap_or_default();
        if keyword.is_empty() { continue; }

        let assessment = rt.block_on(async {
            match provider.serp_features(&keyword, "us").await {
                Ok(serp) => {
                    let kd = kd_str.as_deref().and_then(|s| s.parse::<f64>().ok());
                    pageseeds_lib::seo::winnability::assess(&keyword, &serp, kd, None)
                }
                Err(e) => pageseeds_lib::seo::winnability::WinnabilityAssessment {
                    keyword: keyword.clone(),
                    bucket: pageseeds_lib::seo::winnability::WinnabilityBucket::Avoid,
                    ai_overview_present: false,
                    featured_snippet_present: false,
                    authority_competitors: vec![],
                    risk_score: 99,
                    reason: format!("SERP lookup failed: {e}"),
                },
            }
        });

        results.push(serde_json::json!({
            "article_id": id,
            "title": title,
            "slug": slug,
            "target_keyword": keyword,
            "bucket": assessment.bucket.as_str(),
            "risk_score": assessment.risk_score,
            "ai_overview_present": assessment.ai_overview_present,
            "featured_snippet_present": assessment.featured_snippet_present,
            "authority_competitors": assessment.authority_competitors,
            "reason": assessment.reason,
        }));
    }

    let avoid: Vec<_> = results.iter().filter(|r| r["bucket"] == "avoid").collect();
    let differentiate: Vec<_> = results.iter().filter(|r| r["bucket"] == "differentiate").collect();
    let target: Vec<_> = results.iter().filter(|r| r["bucket"] == "target").collect();

    Ok(serde_json::json!({
        "scored": results.len(),
        "avoid": { "count": avoid.len(), "articles": avoid },
        "differentiate": { "count": differentiate.len(), "articles": differentiate },
        "target": { "count": target.len(), "articles": target },
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Remaining inline functions (not yet extracted as shared — small or async)
// ═══════════════════════════════════════════════════════════════════════════════

fn article_frontmatter(project_path: &str, slug: &str) -> Result<serde_json::Value, String> {
    // Shared resolver: direct path, then NN_slug.mdx / normalized stem / frontmatter.
    let fp = pageseeds_lib::content::ops::resolve_slug_or_path(
        std::path::Path::new(project_path),
        slug,
    )?;
    let meta = pageseeds_lib::content::ops::read_file_metadata(&fp).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "slug": meta.url_slug, "file": meta.file_name,
        "title": meta.title, "published_date": meta.published_date,
        "status": meta.status, "word_count": meta.word_count,
    }))
}

fn run_audit(project_id: &str, project_path: &str) -> Result<serde_json::Value, String> {
    use pageseeds_lib::models::task::*;
    let task = pageseeds_lib::models::task::Task {
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
    let result = pageseeds_lib::engine::exec::content_audit::exec_content_audit(&task, project_path);
    if !result.success { return Err(result.message); }
    serde_json::from_str(result.output.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())
}

fn ctr_health(project_id: &str, project_path: &str, db_path: &str) -> Result<serde_json::Value, String> {
    let conn = open_db(db_path)?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id).map_err(|e| e.to_string())?;
    let summary = pageseeds_lib::content::ops::build_ctr_health_summary(
        std::path::Path::new(project_path), &articles, 0, 0, &conn, project_id,
    );
    Ok(serde_json::to_value(summary).unwrap_or_default())
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
        let task = pageseeds_lib::engine::content_fix::spawn_fix_content_article_for_slug(
            &conn,
            project_id,
            &slug_val,
            &reason,
            pageseeds_lib::engine::content_fix::SpawnFixForSlugOpts {
                title: if title.is_empty() { None } else { Some(title) },
                priority: priority_enum,
                auto_enqueue,
                source: "pageseeds-cli".to_string(),
            },
        )
        .map_err(|e| e.to_string())?;
        return Ok(serde_json::json!({
            "task_id": task.id,
            "task_type": tt,
            "title": task.title,
            "status": task.status,
        }));
    }

    // fix_ctr_article always attaches a single-article ctr_context (GSC + file
    // excerpt), matching audit-spawned children. Bare TaskSpawner creates omit
    // the artifact and cause analyze to fail.
    if tt == "fix_ctr_article" {
        let slug_val = slug.ok_or_else(|| {
            "--slug required for fix_ctr_article (url slug of the article to fix)".to_string()
        })?;
        let task = pageseeds_lib::engine::ctr_fix::spawn_fix_ctr_article_for_slug(
            &conn,
            project_id,
            project_path,
            &slug_val,
            pageseeds_lib::engine::ctr_fix::SpawnFixCtrForSlugOpts {
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
        return Ok(serde_json::json!({
            "task_id": task.id,
            "task_type": tt,
            "title": task.title,
            "status": task.status,
        }));
    }

    let task = pageseeds_lib::engine::spawner::TaskSpawner::spawn(&conn, pageseeds_lib::engine::spawner::TaskSpec {
        project_id: project_id.to_string(), task_type: tt.clone(),
        title: Some(title.clone()), description: Some(reason),
        priority: priority_enum,
        run_policy: if auto_enqueue { Some(TaskRunPolicy::AutoEnqueue) } else { None },
        ..Default::default()
    }).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"task_id": task.id, "task_type": tt, "title": title, "status": task.status}))
}

fn write_spec(project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let title = flag(args, "--issue-title", "-T").unwrap_or_default();
    let sev = flag(args, "--severity", "-s").unwrap_or_else(|| "warning".into());
    let impact = flag(args, "--impact", "-m").unwrap_or_default();
    let file = flag(args, "--file-to-edit", "-f").unwrap_or_default();
    let current = flag(args, "--current-code", "-c").unwrap_or_default();
    let fixed = flag(args, "--fixed-code", "-F").unwrap_or_default();
    let notes = flag(args, "--notes", "-n");
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
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
    pageseeds_lib::engine::exec::ctr_audit::rendered::compare_rendered_titles(project_path, max)
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
    let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_page_rows(
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
        let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_queries_for_page(
            &token, &site, &url, &start, &end, limit,
        ))
        .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    } else {
        let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_page_query_rows(
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
    let m = rt.block_on(pageseeds_lib::gsc::analytics::compute_movers(
        &token, &site, &cs, &ce, &ps, &pe, limit,
    ))
    .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(m).unwrap_or_default())
}

async fn gsc_token(project_id: &str, project_path: &str) -> Result<(String, String), String> {
    let resolver = pageseeds_lib::config::env_resolver::EnvResolver::new(project_path);
    let sa = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH").map(|(v, _)| v)
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        .ok_or_else(|| "GSC not connected".to_string())?;
    let token = pageseeds_lib::gsc::auth::get_service_account_token(&sa).await.map_err(|e| e.to_string())?;
    let conn = open_db(&pageseeds_lib::db::default_db_path().to_string_lossy())?;
    let project = pageseeds_lib::engine::task_store::get_project(&conn, project_id).map_err(|e| e.to_string())?;
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
    pageseeds_lib::db::init(std::path::Path::new(db_path)).map_err(|e| e.to_string())
}

fn flag(args: &[String], long: &str, short: &str) -> Option<String> {
    for i in 0..args.len() { if args[i] == long || args[i] == short { if i + 1 < args.len() { return Some(args[i + 1].clone()); } } }
    None
}

fn has_flag(args: &[String], long: &str, short: &str) -> bool {
    args.iter().any(|a| a == long || a == short)
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') { std::env::var("HOME").map(|h| path.replacen('~', &h, 1)).unwrap_or_else(|_| path.into()) }
    else { path.into() }
}

fn exit(msg: &str) -> ! { eprintln!("ERROR: {msg}"); std::process::exit(1); }

fn print_help() {
    eprintln!(r#"pageseeds-cli — individual data tools for agents / KimiCode

Each subcommand calls one PageSeeds data function and prints JSON to stdout.
Uses the same SQLite DB as the desktop app. Prefer the installed binary from
any directory (do not require opening pageseeds-app source).

Usage:
  pageseeds-cli <tool> -i <project-id> -p <project-path> [args]
  # install: ./scripts/install-cli.sh  →  ~/.local/bin/pageseeds-cli

Tools:
  gsc-performance  gsc-queries  gsc-movers   [-l/--limit N]  (defaults: perf/queries 50, movers 30; max 200)
  gsc-queries      also accepts -u/--page-url <url>
  article-list  article-frontmatter  article-body-hash  article-title-scan
  content-audit-report  run-content-audit  cannibalization-clusters
  indexing-status  ctr-health  framework-files  article-link-graph
  research-shortlist  article-quality-reviews  compare-rendered  write-feature-spec

Task / queue orchestration:
  list-tasks              -i <id> -p <path> [-t type] [-s status]
  cancel-tasks            -i <id> -p <path> -t type [-s status] [--yes]
  create-task             -i <id> -p <path> -t type [-T title] [-r reason] [-a] [-P high|medium|low]
                          fix_content_article also requires -S/--slug <url-slug> (builds recommendations artifact)
                          fix_ctr_article also requires -S/--slug <url-slug> (builds ctr_context from GSC + file)
  execute-task            -I <task-id>
  get-task                -I <task-id>                          Full task JSON incl. artifacts
  update-task-status      -I <task-id> -s done|cancelled        Close out artifact-review tasks
  select-keywords         -I <research-task-id> -K <kw,kw,...>  Create content tasks, mark research done
  select-content-review   -I <review-task-id> -P <proposal-id,...>  Spawn fix_content_article from content_review
  select-cannibalization  -I <parent-task-id> -S <type:id,...>  Spawn cannibalization fixes, mark parent done
  create-articles-from-keywords -i <id> -I <research-task-id> -k "kw1, kw2, ..."
  set-task-status         -I <task-id> -s <status>              Set any task status

Cannibalization workflow:
  cannibalization-strategy -i <id> -p <path>
  set-review-status        -i <id> -S <strategy-id> -T <type> -I <rec-id> -s approved|rejected|pending
  create-tasks-from-approved -i <id> -S <strategy-id>|latest

Dead-weight remediation (WS4):
  score-zero-impression-articles -i <id> -p <path> [-m <max-impressions>]

Research / quality health:
  research-shortlist        -i <id> [-s pending|researched|covered] [-H promising|unproven|depleted]
  article-quality-reviews   -i <id> [-l <limit>]

Common: -i/--project-id  -p/--project-path
Run with <tool> --help for tool-specific flags.
"#);
}
