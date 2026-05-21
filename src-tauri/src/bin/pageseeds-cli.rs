/// PageSeeds CLI — individual data tools for KimiCode.
///
/// Each subcommand calls one PageSeeds data function directly and prints
/// JSON to stdout. KimiCode calls tools individually during investigation,
/// deciding which tools to use and in what order.
///
/// Usage:
///   cargo run --bin pageseeds-cli -- <tool> -i <project-id> -p <project-path> [args...]

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        eprintln!(r#"pageseeds-cli — individual data tools for KimiCode

Each subcommand calls one PageSeeds data function and prints JSON to stdout.

Usage:
  cargo run --bin pageseeds-cli -- <tool> -i <project-id> -p <project-path> [args...]

Available tools:

  gsc-performance       Page-level clicks, impressions, CTR, position
  gsc-queries           Search queries driving traffic [--page-url URL]
  gsc-movers            Gaining/declining pages vs previous period
  article-list          All articles with metadata [--status STATUS]
  article-frontmatter   Frontmatter for one article [--slug SLUG]
  article-body-hash     SHA-256 hashes of all bodies, find duplicates
  article-title-scan    Title patterns: dupes, literal vars, truncation
  content-audit-report  Read content_audit.json from disk
  run-content-audit     Run fresh 21-check audit, write to disk
  cannibalization-clusters  Cannibalization clusters + merge recommendations
  indexing-status       GSC URL indexing status
  ctr-health            Per-article CTR health summary
  framework-files       Read layout files, sitemap, robots.txt [--file FILE]
  article-link-graph    Internal link graph, orphan detection
  create-task           Create fix task in PageSeeds [-t TYPE -T TITLE -r WHY]
  write-feature-spec    Write developer spec to target repo

Common flags: -i/--project-id, -p/--project-path

Example:
  cargo run --bin pageseeds-cli -- article-list -i abc123 -p ~/code/mysite
  cargo run --bin pageseeds-cli -- gsc-performance -i abc123 -p ~/code/mysite
  cargo run --bin pageseeds-cli -- article-body-hash -i abc123 -p ~/code/mysite
"#);
        return;
    }

    let tool = &args[1];
    let project_id = get_flag(&args, "--project-id", "-i").unwrap_or_else(|| exit("--project-id required"));
    let project_path = expand_tilde(&get_flag(&args, "--project-path", "-p").unwrap_or_else(|| exit("--project-path required")));

    let db_path = pageseeds_lib::db::default_db_path();
    let db_path_str = db_path.to_string_lossy().to_string();

    let result = run_tool(tool, &project_id, &project_path, &db_path_str, &args);

    match result {
        Ok(json) => println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default()),
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    }
}

fn run_tool(
    tool: &str, project_id: &str, project_path: &str, db_path: &str, args: &[String],
) -> Result<serde_json::Value, String> {
    match tool {
        "gsc-performance" => gsc_performance(project_id, project_path),
        "gsc-queries" => gsc_queries(project_id, project_path, get_flag(args, "--page-url", "-u")),
        "gsc-movers" => gsc_movers(project_id, project_path),
        "article-list" => article_list(project_id, db_path, get_flag(args, "--status", "-s")),
        "article-frontmatter" => article_frontmatter(project_path, &get_flag(args, "--slug", "-S").unwrap_or_else(|| exit("--slug required"))),
        "article-body-hash" => article_body_hash(project_id, project_path, db_path),
        "article-title-scan" => article_title_scan(project_id, db_path),
        "content-audit-report" => content_audit_report(project_path),
        "run-content-audit" => run_content_audit(project_id, project_path, db_path),
        "cannibalization-clusters" => cannibalization_clusters(project_path),
        "indexing-status" => indexing_status(project_id, db_path),
        "ctr-health" => ctr_health(project_id, project_path, db_path),
        "framework-files" => framework_files(project_path, get_flag(args, "--file", "-f")),
        "article-link-graph" => article_link_graph(project_id, project_path, db_path),

        "create-task" => create_task(
            project_id, db_path,
            &get_flag(args, "--task-type", "-t").unwrap_or_default(),
            &get_flag(args, "--title", "-T").unwrap_or_default(),
            &get_flag(args, "--reason", "-r").unwrap_or_default(),
        ),
        "write-feature-spec" => write_feature_spec(
            project_path,
            &get_flag(args, "--issue-title", "-T").unwrap_or_default(),
            &get_flag(args, "--severity", "-s").unwrap_or_else(|| "warning".into()),
            &get_flag(args, "--impact", "-m").unwrap_or_default(),
            &get_flag(args, "--file-to-edit", "-f").unwrap_or_default(),
            &get_flag(args, "--current-code", "-c").unwrap_or_default(),
            &get_flag(args, "--fixed-code", "-F").unwrap_or_default(),
            get_flag(args, "--notes", "-n"),
        ),
        _ => {
            let available = [
                "gsc-performance", "gsc-queries", "gsc-movers", "article-list",
                "article-frontmatter", "article-body-hash", "article-title-scan",
                "content-audit-report", "run-content-audit", "cannibalization-clusters",
                "indexing-status", "ctr-health", "framework-files", "article-link-graph",
                "create-task", "write-feature-spec",
            ];
            Err(format!("Unknown tool '{}'. Available: {}", tool, available.join(", ")))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool implementations (thin wrappers around existing Rust functions)
// ═══════════════════════════════════════════════════════════════════════════════

fn gsc_performance(project_id: &str, project_path: &str) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site_url, token) = rt.block_on(get_gsc_token(project_id, project_path))?;
    let end = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let start = (chrono::Utc::now() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
    let metrics = rt.block_on(pageseeds_lib::gsc::analytics::fetch_page_rows(&token, &site_url, &start, &end, 50))
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(metrics).unwrap_or_default())
}

fn gsc_queries(project_id: &str, project_path: &str, page_url: Option<String>) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site_url, token) = rt.block_on(get_gsc_token(project_id, project_path))?;
    let end = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let start = (chrono::Utc::now() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
    if let Some(url) = page_url {
        let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_queries_for_page(&token, &site_url, &url, &start, &end, 50))
            .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    } else {
        let m = rt.block_on(pageseeds_lib::gsc::analytics::fetch_page_query_rows(&token, &site_url, &start, &end, 50))
            .map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(m).unwrap_or_default())
    }
}

fn gsc_movers(project_id: &str, project_path: &str) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (site_url, token) = rt.block_on(get_gsc_token(project_id, project_path))?;
    let now = chrono::Utc::now();
    let curr_end = now.format("%Y-%m-%d").to_string();
    let curr_start = (now - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
    let prev_end = (now - chrono::Duration::days(31)).format("%Y-%m-%d").to_string();
    let prev_start = (now - chrono::Duration::days(61)).format("%Y-%m-%d").to_string();
    let movers = rt.block_on(pageseeds_lib::gsc::analytics::compute_movers(&token, &site_url, &curr_start, &curr_end, &prev_start, &prev_end, 30))
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_value(movers).unwrap_or_default())
}

fn article_list(project_id: &str, db_path: &str, status: Option<String>) -> Result<serde_json::Value, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let filtered: Vec<serde_json::Value> = articles.iter()
        .filter(|a| status.as_ref().map_or(true, |s| a.status.to_lowercase() == s.to_lowercase()))
        .take(200)
        .map(|a| serde_json::json!({
            "id": a.id, "title": a.title, "slug": a.url_slug,
            "file": a.file, "status": a.status, "published_date": a.published_date,
            "target_keyword": a.target_keyword, "word_count": a.word_count,
            "page_type": a.page_type,
        }))
        .collect();
    Ok(serde_json::to_value(filtered).unwrap_or_default())
}

fn article_frontmatter(project_path: &str, slug: &str) -> Result<serde_json::Value, String> {
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
    let content_dir = pageseeds_lib::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
        .map_err(|e| e.to_string())?;
    let candidates = [
        paths.repo_root.join(slug),
        content_dir.join(format!("{}.mdx", slug)),
        content_dir.join(format!("{}.md", slug)),
    ];
    let file_path = candidates.iter().find(|p| p.exists())
        .ok_or_else(|| format!("File not found for slug: {slug}"))?;
    let meta = pageseeds_lib::content::ops::read_file_metadata(file_path)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "slug": meta.url_slug, "file": meta.file_name,
        "title": meta.title, "published_date": meta.published_date,
        "status": meta.status, "word_count": meta.word_count,
    }))
}

fn article_body_hash(project_id: &str, project_path: &str, db_path: &str) -> Result<serde_json::Value, String> {
    use sha2::{Digest, Sha256};
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let repo_root = Path::new(project_path);
    let mut groups: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    for a in &articles {
        let source = pageseeds_lib::engine::exec::utils::read_source_file(repo_root, &a.file);
        let (_fm, body) = pageseeds_lib::engine::exec::utils::parse_frontmatter(source.as_deref().unwrap_or(""));
        let mut h = Sha256::new();
        h.update(body.as_bytes());
        let hash = format!("{:x}", h.finalize());
        groups.entry(hash).or_default().push(serde_json::json!({
            "id": a.id, "title": a.title, "slug": a.url_slug, "file": a.file,
        }));
    }
    let duplicates: Vec<serde_json::Value> = groups.into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(hash, arts)| serde_json::json!({"hash": hash, "count": arts.len(), "articles": arts}))
        .collect();
    Ok(serde_json::to_value(duplicates).unwrap_or_default())
}

fn article_title_scan(project_id: &str, db_path: &str) -> Result<serde_json::Value, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let mut missing = 0usize; let mut dup = 0usize; let mut lit = 0usize; let mut long = 0usize;
    let mut examples: Vec<serde_json::Value> = Vec::new();
    for a in &articles {
        let t = a.title.trim();
        if t.is_empty() { missing += 1; continue; }
        let tl = t.to_lowercase();
        if tl.contains("| brand |") || tl.contains("{brand}") || tl.contains("{{title}}") {
            lit += 1;
            if examples.len() < 5 { examples.push(serde_json::json!({"title": t, "slug": a.url_slug, "issue": "literal template variable"})); }
        }
        let tokens: Vec<&str> = tl.split(|c: char| !c.is_alphanumeric()).filter(|s| s.len() > 2).collect();
        let mut counts = std::collections::HashMap::new();
        for tok in &tokens {             *counts.entry(*tok).or_insert(0) += 1; }
        if counts.values().any(|&c| c >= 3) {
            dup += 1;
            if examples.len() < 5 {
                let w = counts.iter().find(|(_, &c)| c >= 3).map(|(w, _)| *w).unwrap_or("");
                examples.push(serde_json::json!({"title": t, "slug": a.url_slug, "issue": format!("token '{}' appears {} times", w, counts[w])}));
            }
        }
        if t.len() > 60 { long += 1; }
    }
    Ok(serde_json::json!({
        "total": articles.len(), "missing_titles": missing,
        "duplicate_token_titles": dup, "literal_var_titles": lit,
        "long_titles": long, "examples": examples,
    }))
}

fn content_audit_report(project_path: &str) -> Result<serde_json::Value, String> {
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
    let p = paths.automation_dir.join("content_audit.json");
    if !p.exists() { return Err("No content_audit.json found. Run run-content-audit first.".into()); }
    let s = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

fn run_content_audit(project_id: &str, project_path: &str, _db_path: &str) -> Result<serde_json::Value, String> {
    use pageseeds_lib::models::task::*;
    let task = pageseeds_lib::models::task::Task {
        id: "cli-audit".into(), task_type: "content_audit".into(),
        project_id: project_id.to_string(),
        title: Some("CLI content audit".into()), description: None,
        status: TaskStatus::InProgress, phase: "audit".into(),
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

fn cannibalization_clusters(project_path: &str) -> Result<serde_json::Value, String> {
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
    let p = paths.automation_dir.join("cannibalization_strategy.json");
    if !p.exists() { return Ok(serde_json::json!({"clusters": [], "note": "No strategy found"})); }
    let s = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

fn indexing_status(project_id: &str, db_path: &str) -> Result<serde_json::Value, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let statuses = pageseeds_lib::gsc::db::list_by_project(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let total = statuses.len();
    let indexed = statuses.iter().filter(|s| s.last_reason_code.as_deref() == Some("indexed_pass")).count();
    let mut reasons: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in &statuses {
        if let Some(r) = &s.last_reason_code { if r != "indexed_pass" { *reasons.entry(r.clone()).or_default() += 1; } }
    }
    Ok(serde_json::json!({"total_urls": total, "indexed": indexed, "not_indexed": total - indexed,
        "issues_by_reason": reasons.iter().map(|(k, v)| serde_json::json!({"reason": k, "count": v})).collect::<Vec<_>>()}))
}

fn ctr_health(project_id: &str, project_path: &str, db_path: &str) -> Result<serde_json::Value, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let summary = pageseeds_lib::content::ops::build_ctr_health_summary(
        Path::new(project_path), &articles, 0, 0, &conn, project_id,
    );
    Ok(serde_json::to_value(summary).unwrap_or_default())
}

fn framework_files(project_path: &str, file: Option<String>) -> Result<serde_json::Value, String> {
    let root = Path::new(project_path);
    let candidates = [
        ("app/layout.tsx", "Next.js app layout"),
        ("pages/_app.tsx", "Next.js pages app"),
        ("next.config.js", "Next.js config"),
        ("next-sitemap.config.js", "Sitemap config"),
        ("app/sitemap.ts", "App router sitemap"),
        ("robots.txt", "Robots exclusion"),
    ];
    if let Some(ref f) = file {
        let p = root.join(f);
        if !p.exists() { return Err(format!("File not found: {}", f)); }
        let content = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let truncated = if content.len() > 8000 { format!("{}...\n[truncated from {}]", &content[..8000], content.len()) } else { content };
        Ok(serde_json::json!({"file": f, "content": truncated}))
    } else {
        let found: Vec<serde_json::Value> = candidates.iter().map(|(f, desc)| {
            serde_json::json!({"path": f, "description": desc, "exists": root.join(f).exists()})
        }).collect();
        Ok(serde_json::json!({"files": found}))
    }
}

fn article_link_graph(project_id: &str, project_path: &str, db_path: &str) -> Result<serde_json::Value, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let articles = pageseeds_lib::engine::task_store::list_articles(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
    let content_dir = pageseeds_lib::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
        .map_err(|e| e.to_string())?;
    let scan = pageseeds_lib::content::linking::scan_links(&content_dir, &articles)
        .map_err(|e| e.to_string())?;
    let orphans: Vec<serde_json::Value> = scan.orphan_ids.iter().map(|&id| {
        let a = articles.iter().find(|a| a.id == id);
        serde_json::json!({"id": id, "title": a.map(|a| a.title.as_str()).unwrap_or(""), "slug": a.map(|a| a.url_slug.as_str()).unwrap_or("")})
    }).collect();
    Ok(serde_json::json!({"total_articles": scan.total_articles, "total_internal_links": scan.total_internal_links, "orphan_count": scan.orphan_ids.len(), "orphans": orphans}))
}

fn create_task(project_id: &str, db_path: &str, task_type: &str, title: &str, reason: &str) -> Result<serde_json::Value, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let task = pageseeds_lib::engine::spawner::TaskSpawner::spawn(&conn, pageseeds_lib::engine::spawner::TaskSpec {
        project_id: project_id.to_string(), task_type: task_type.to_string(),
        title: Some(title.to_string()), description: Some(reason.to_string()),
        priority: pageseeds_lib::models::task::Priority::Medium,
        ..Default::default()
    }).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"task_id": task.id, "task_type": task_type, "title": title}))
}

fn write_feature_spec(project_path: &str, title: &str, severity: &str, impact: &str, file: &str, current: &str, fixed: &str, notes: Option<String>) -> Result<serde_json::Value, String> {
    let paths = pageseeds_lib::engine::project_paths::ProjectPaths::from_path(project_path);
    let spec_path = paths.automation_dir.join("seo_feature_spec.md");
    let header = if spec_path.exists() { String::new() } else {
        format!("# SEO Feature Specification\n\nGenerated by PageSeeds on {}\n\n", chrono::Utc::now().format("%Y-%m-%d"))
    };
    let existing = if spec_path.exists() { std::fs::read_to_string(&spec_path).unwrap_or_default() } else { String::new() };
    let notes_s = notes.map(|n| format!("\n**Notes:** {n}\n")).unwrap_or_default();
    let section = format!("\n---\n\n## {title}\n\n**Severity:** {severity} | **Impact:** {impact}\n**File:** `{file}`\n\n**Current:**\n```\n{current}\n```\n\n**Fixed:**\n```\n{fixed}\n```{notes_s}\n");
    std::fs::write(&spec_path, format!("{header}{existing}{section}")).map_err(|e| e.to_string())?;
    let count = format!("{header}{existing}{section}").matches("\n## ").count();
    Ok(serde_json::json!({"path": spec_path.to_string_lossy().to_string(), "issues": count}))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn get_flag(args: &[String], long: &str, short: &str) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == long || args[i] == short {
            if i + 1 < args.len() { return Some(args[i + 1].clone()); }
        }
    }
    None
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        std::env::var("HOME").map(|h| path.replacen('~', &h, 1)).unwrap_or_else(|_| path.into())
    } else { path.into() }
}

fn exit(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    std::process::exit(1);
}

async fn get_gsc_token(project_id: &str, project_path: &str) -> Result<(String, String), String> {
    let resolver = pageseeds_lib::config::env_resolver::EnvResolver::new(project_path);
    let sa_path = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
        .map(|(v, _)| v)
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        .ok_or_else(|| "GSC not connected. Set GSC_SERVICE_ACCOUNT_PATH.".to_string())?;
    let token = pageseeds_lib::gsc::auth::get_service_account_token(&sa_path)
        .await.map_err(|e| e.to_string())?;
    let db_path = pageseeds_lib::db::default_db_path();
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
    let project = pageseeds_lib::engine::task_store::get_project(&conn, project_id)
        .map_err(|e| e.to_string())?;
    let site_url = project.site_url.unwrap_or_default();
    if site_url.is_empty() { return Err("No site_url configured".into()); }
    Ok((site_url, token.access_token))
}
