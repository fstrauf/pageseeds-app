/// PageSeeds CLI — individual data tools for KimiCode.
///
/// Each subcommand calls a shared standalone function from
/// engine/tools/investigate.rs (the same functions the Rig Tool impls use).
/// Zero business logic duplication.
///
/// Usage:
///   cargo run --bin pageseeds-cli -- <tool> -i <project-id> -p <project-path> [args...]

use pageseeds_lib::engine::tools::{investigation_tools, InvestigationContext};
use pageseeds_lib::engine::tools::investigate;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_help();
        return;
    }

    let tool = &args[1];
    let project_id = flag(&args, "--project-id", "-i").unwrap_or_default();
    let project_path = expand_tilde(&flag(&args, "--project-path", "-p").unwrap_or_else(|| exit("--project-path required")));

    let db = pageseeds_lib::db::default_db_path();
    let ctx = InvestigationContext {
        project_id: project_id.clone(),
        project_path: project_path.clone(),
        db_path: db.to_string_lossy().to_string(),
    };

    let result: Result<serde_json::Value, String> = match tool.as_str() {
        // ── GSC tools (async, kept inline since they need tokio) ──
        "gsc-performance" => gsc_perf(&project_id, &project_path),
        "gsc-queries" => gsc_q(&project_id, &project_path, flag(&args, "--page-url", "-u")),
        "gsc-movers" => gsc_mov(&project_id, &project_path),

        // ── Shared functions (single source of truth) ──
        "article-list" => {
            investigate::list_articles_json(&ctx, flag(&args, "--status", "-s").as_deref(), 200)
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "article-frontmatter" => {
            let slug = flag(&args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"));
            article_frontmatter(&project_path, &slug)
        }
        "article-body-hash" => {
            investigate::hash_article_bodies(&ctx)
                .map(|r| serde_json::to_value(r).unwrap_or_default())
                .map_err(|e| e.to_string())
        }
        "article-title-scan" => investigate::scan_article_titles(&ctx).map_err(|e| e.to_string()),
        "content-audit-report" => investigate::read_content_audit_report(&project_path).map_err(|e| e.to_string()),
        "run-content-audit" => run_audit(&project_id, &project_path),
        "cannibalization-clusters" => investigate::read_cannibalization_clusters(&project_path).map_err(|e| e.to_string()),
        "indexing-status" => investigate::get_indexing_status(&ctx).map_err(|e| e.to_string()),
        "ctr-health" => ctr_health(&project_id, &project_path, &db.to_string_lossy()),
        "framework-files" => {
            investigate::read_framework_files(&project_path, flag(&args, "--file", "-f").as_deref())
                .map_err(|e| e.to_string())
        }
        "article-link-graph" => investigate::scan_link_graph(&ctx).map_err(|e| e.to_string()),
        "compare-rendered" => compare_rendered(&project_path, &args),
        "create-task" => create_task(&project_id, &db.to_string_lossy(), &args),
        "write-feature-spec" => write_spec(&project_path, &args),
        _ => Err(format!("Unknown tool '{}'. Run with --help for list.", tool)),
    };

    match result {
        Ok(json) => println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default()),
        Err(e) => { eprintln!("ERROR: {e}"); std::process::exit(1); }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Remaining inline functions (not yet extracted as shared — small or async)
// ═══════════════════════════════════════════════════════════════════════════════

fn article_frontmatter(project_path: &str, slug: &str) -> Result<serde_json::Value, String> {
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
    let content_dir = pageseeds_lib::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
        .map_err(|e| e.to_string())?;
    let candidates = [
        paths.repo_root.join(slug),
        content_dir.join(format!("{}.mdx", slug)), content_dir.join(format!("{}.md", slug)),
    ];
    let fp = candidates.iter().find(|p| p.exists())
        .ok_or_else(|| format!("File not found for slug: {slug}"))?;
    let meta = pageseeds_lib::content::ops::read_file_metadata(fp).map_err(|e| e.to_string())?;
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
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id).map_err(|e| e.to_string())?;
    let summary = pageseeds_lib::content::ops::build_ctr_health_summary(
        std::path::Path::new(project_path), &articles, 0, 0, &conn, project_id,
    );
    Ok(serde_json::to_value(summary).unwrap_or_default())
}

fn create_task(project_id: &str, db_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let tt = flag(args, "--task-type", "-t").unwrap_or_default();
    let title = flag(args, "--title", "-T").unwrap_or_default();
    let reason = flag(args, "--reason", "-r").unwrap_or_default();
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let task = pageseeds_lib::engine::spawner::TaskSpawner::spawn(&conn, pageseeds_lib::engine::spawner::TaskSpec {
        project_id: project_id.to_string(), task_type: tt.clone(),
        title: Some(title.clone()), description: Some(reason),
        priority: pageseeds_lib::models::task::Priority::Medium, ..Default::default()
    }).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"task_id": task.id, "task_type": tt, "title": title}))
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
    let section = format!("\n---\n\n## {title}\n\n**Severity:** {sev} | **Impact:** {impact}\n**File:** `{file}`\n\n**Current:**\n```\n{current}\n```\n\n**Fixed:**\n```\n{fixed}\n```{ns}\n");
    std::fs::write(&spec, format!("{header}{existing}{section}")).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"path": spec.to_string_lossy().to_string(), "issue": title}))
}

/// Compare source frontmatter titles with what Google actually sees (live HTML).
fn compare_rendered(project_path: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let max: usize = flag(args, "--max", "-m").and_then(|s| s.parse().ok()).unwrap_or(25);
    pageseeds_lib::engine::exec::ctr_audit::rendered::compare_rendered_titles(project_path, max)
}

// ── GSC (async — kept inline since they need tokio runtime) ──────────────────

fn gsc_perf(project_id: &str, project_path: &str) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site, token) = rt.block_on(gsc_token(project_id, project_path))?;
    let end = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let start = (chrono::Utc::now() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
    let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_page_rows(&token, &site, &start, &end, 50))
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(m).unwrap_or_default())
}

fn gsc_q(project_id: &str, project_path: &str, page: Option<String>) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site, token) = rt.block_on(gsc_token(project_id, project_path))?;
    let end = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let start = (chrono::Utc::now() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
    if let Some(url) = page {
        let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_queries_for_page(&token, &site, &url, &start, &end, 50))
            .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    } else {
        let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_page_query_rows(&token, &site, &start, &end, 50))
            .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    }
}

fn gsc_mov(project_id: &str, project_path: &str) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site, token) = rt.block_on(gsc_token(project_id, project_path))?;
    let now = chrono::Utc::now();
    let ce = now.format("%Y-%m-%d").to_string(); let cs = (now - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
    let pe = (now - chrono::Duration::days(31)).format("%Y-%m-%d").to_string(); let ps = (now - chrono::Duration::days(61)).format("%Y-%m-%d").to_string();
    let m = rt.block_on(pageseeds_lib::gsc::analytics::compute_movers(&token, &site, &cs, &ce, &ps, &pe, 30))
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(m).unwrap_or_default())
}

async fn gsc_token(project_id: &str, project_path: &str) -> Result<(String, String), String> {
    let resolver = pageseeds_lib::config::env_resolver::EnvResolver::new(project_path);
    let sa = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH").map(|(v, _)| v)
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        .ok_or_else(|| "GSC not connected".to_string())?;
    let token = pageseeds_lib::gsc::auth::get_service_account_token(&sa).await.map_err(|e| e.to_string())?;
    let conn = rusqlite::Connection::open(pageseeds_lib::db::default_db_path().to_string_lossy().to_string()).map_err(|e| e.to_string())?;
    let project = pageseeds_lib::engine::task_store::get_project(&conn, project_id).map_err(|e| e.to_string())?;
    let site = project.site_url.unwrap_or_default();
    if site.is_empty() { return Err("No site_url configured".into()); }
    Ok((site, token.access_token))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn flag(args: &[String], long: &str, short: &str) -> Option<String> {
    for i in 0..args.len() { if args[i] == long || args[i] == short { if i + 1 < args.len() { return Some(args[i + 1].clone()); } } }
    None
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') { std::env::var("HOME").map(|h| path.replacen('~', &h, 1)).unwrap_or_else(|_| path.into()) }
    else { path.into() }
}

fn exit(msg: &str) -> ! { eprintln!("ERROR: {msg}"); std::process::exit(1); }

fn print_help() {
    eprintln!(r#"pageseeds-cli — individual data tools for KimiCode

Each subcommand calls one PageSeeds data function and prints JSON to stdout.

Usage:
  cargo run --bin pageseeds-cli -- <tool> -i <project-id> -p <project-path> [args]

Tools:  gsc-performance  gsc-queries  gsc-movers  article-list  article-frontmatter
        article-body-hash  article-title-scan  content-audit-report  run-content-audit
        cannibalization-clusters  indexing-status  ctr-health  framework-files
        article-link-graph  compare-rendered  create-task  write-feature-spec

Common: -i/--project-id  -p/--project-path
Run with <tool> --help for tool-specific flags.
"#);
}
