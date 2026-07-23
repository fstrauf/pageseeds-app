/// Smoke test for the write_article pipeline in isolation.
///
/// Run with:
///   cargo run --bin write_article_smoke
///
/// What it does:
///   1. Opens the real PageSeeds SQLite DB
///   2. Uses the days_to_expiry project (call-analyzer repo)
///   3. Creates a `write_article` task for TARGET_KEYWORD
///   4. Executes the task directly through executor::execute_task
///   5. Prints the generated MDX so you can review content quality
///   6. Asserts the issue #13 contract:
///      - success ⟹ a new .mdx file exists AND is registered as a draft row
///      - no output ⟹ the task fails loudly (never Done with zero files)
///
/// This exercises the full ContentHandler → content-write skill → exec_agentic
/// path without the Tauri app, queue, or IPC. Use it to iterate on the
/// content-write skill and review output quality before shipping. Nested write
/// requires a file-IO host (`grok`/`kimi`, issue #143); text-only providers
/// fail the host gate before the agent runs.
use pageseeds_lib::{
    db,
    engine::{executor, task_store},
    models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    },
    rig::provider::provider_supports_file_io,
};

/// Target project. days_to_expiry = the call-analyzer repo.
const PROJECT_ID: &str = "days_to_expiry";

/// Keyword to write about. Pick a REAL but UNCOVERED topic so the test article
/// doesn't cannibalize an existing page. "gamma scalping strategy" is in the
/// known content-gap list (competitors cover it, this site doesn't).
const TARGET_KEYWORD: &str = "gamma scalping strategy";

/// Metrics injected into the task description (same shape the research picker
/// produces). These are illustrative for the smoke test.
const KEYWORD_KD: u32 = 35;
const KEYWORD_VOLUME: u32 = 3000;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    // ── DB + project ────────────────────────────────────────────────────────
    let db_path = pageseeds_lib::db::default_db_path();
    let conn = db::init(&db_path).expect("DB init failed");

    let project = match task_store::get_project(&conn, PROJECT_ID) {
        Ok(p) => {
            println!("✓ Using project: {} ({})", p.name, p.path);
            p
        }
        Err(e) => {
            eprintln!("✗ Project '{}' not found in DB: {}", PROJECT_ID, e);
            std::process::exit(1);
        }
    };

    let provider = db::global_settings::resolve_agent_provider(&conn, project.agent_provider.as_deref());
    println!(
        "✓ Agent provider: {} ({})",
        provider,
        if provider_supports_file_io(&provider) {
            "file-IO capable — agent writes the file itself"
        } else {
            "text-only — nested content write will fail the #143 host gate"
        }
    );

    // Snapshot the content dir so we can detect the newly written file.
    let content_blog = std::path::Path::new(&project.path)
        .join("webapp")
        .join("content")
        .join("blog");
    let before: std::collections::HashSet<String> = std::fs::read_dir(&content_blog)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    println!(
        "✓ Content dir: {} ({} existing .mdx files)",
        content_blog.display(),
        before.len()
    );

    // ── Task ────────────────────────────────────────────────────────────────
    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!(
        "smoke-write-article-{}",
        chrono::Utc::now().timestamp_millis()
    );
    let description = format!(
        "Target keyword: {}\nKD: {}\nVolume: {}\nIntent: informational",
        TARGET_KEYWORD, KEYWORD_KD, KEYWORD_VOLUME
    );
    let task = Task {
        id: task_id.clone(),
        task_type: "write_article".to_string(),
        phase: "implementation".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        agent_policy: AgentPolicy::Required,
        title: Some(format!("Smoke: write_article — {}", TARGET_KEYWORD)),
        description: Some(description),
        project_id: project.id.clone(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
        not_before: None,
    };
    task_store::create_task(&conn, &task).expect("create_task failed");
    println!("✓ Task created: {} ({})", task_id, TARGET_KEYWORD);

    // ── Execute ─────────────────────────────────────────────────────────────
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Executing write_article...");
    println!("  Project: {} ({})", project.name, project.path);
    println!("  Keyword: {}", TARGET_KEYWORD);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let start = std::time::Instant::now();
    let result = executor::execute_task(&conn, &task_id)
        .await
        .expect("execute_task panic");

    println!("\n━━━━━━━━━━━━━━━━━━━  Result  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    println!("Success : {}", result.success);
    println!("Message : {}", result.message);
    println!("Duration: {:.1}s", start.elapsed().as_secs_f64());
    println!();

    for step in &result.steps {
        let icon = match step.status.as_str() {
            "ok" => "✓",
            "failed" => "✗",
            "skipped" => "~",
            _ => "·",
        };
        println!("  {} [{}] {}", icon, step.status, step.step_name);
        if !step.message.is_empty() {
            println!("      {}", step.message);
        }
        println!();
    }

    // ── Find + print the generated MDX ──────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━  Generated Article  ━━━━━━━━━━━━━━━━━━━━━━━\n");
    let after: Vec<String> = std::fs::read_dir(&content_blog)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    let new_files: Vec<&String> = after.iter().filter(|f| !before.contains(*f)).collect();

    if new_files.is_empty() {
        println!("✗ No new .mdx file detected in {}", content_blog.display());
        if result.success {
            println!("  (This is the issue #13 silent no-op — it must NOT happen anymore.");
        } else {
            println!("  (The task failed loudly, which is the intended behavior when the");
            println!("   provider produced no file and no parseable MDX output.)");
        }
    } else {
        for f in &new_files {
            let path = content_blog.join(f);
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let words = content.split_whitespace().count();
                    println!("✓ Written: {}", path.display());
                    println!("  Words: {}", words);
                    println!();
                    println!("────────  FULL CONTENT  ────────");
                    println!("{}", content);
                    println!("────────  END CONTENT  ────────\n");
                }
                Err(e) => println!("✗ Could not read {}: {}", path.display(), e),
            }
        }
    }

    // ── Assert the issue #13 contract ───────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━  Assertions  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    let mut failures: Vec<String> = vec![];

    let verify_step = result
        .steps
        .iter()
        .find(|s| s.step_name == "content_write_verify");
    match verify_step {
        Some(s) => println!("✓ content_write_verify step present (status: {})", s.status),
        None => failures.push("content_write_verify step missing from the step graph".to_string()),
    }

    if result.success {
        if new_files.is_empty() {
            failures.push(
                "task succeeded but no new .mdx file was written (silent no-op)".to_string(),
            );
        }
        if matches!(verify_step, Some(s) if s.status != "ok") {
            failures.push("task succeeded but content_write_verify did not pass".to_string());
        }
        for f in &new_files {
            let registered: bool = conn
                .query_row(
                    "SELECT 1 FROM articles \
                     WHERE project_id = ?1 AND file LIKE ?2 AND status = 'draft' LIMIT 1",
                    rusqlite::params![PROJECT_ID, format!("%{}", f)],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if registered {
                println!("✓ {} registered as a draft row in SQLite", f);
            } else {
                failures.push(format!("{} written but no draft row registered in SQLite", f));
            }
        }
    } else {
        let status = task_store::get_task(&conn, &task_id)
            .map(|t| t.status)
            .unwrap_or(TaskStatus::Failed);
        if status == TaskStatus::Failed {
            println!("✓ failure path: task status is Failed (loud, retryable)");
        } else {
            failures.push(format!(
                "task execution failed but DB status is {:?}, expected Failed",
                status
            ));
        }
    }

    // Clean up the smoke task from the DB so it doesn't clutter the queue.
    let _ = task_store::delete_task(&conn, &task_id);
    println!("✓ Cleaned up smoke task from DB.");

    if failures.is_empty() {
        println!("\n✓ All assertions passed.");
    } else {
        println!("\n✗ {} assertion(s) failed:", failures.len());
        for f in &failures {
            println!("  - {}", f);
        }
        std::process::exit(1);
    }
}
