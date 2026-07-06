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
///
/// This exercises the full ContentHandler → content-write skill → exec_agentic
/// path without the Tauri app, queue, or IPC. Use it to iterate on the
/// content-write skill and review output quality before shipping.
use pageseeds_lib::{
    db,
    engine::{executor, task_store},
    models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    },
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
        println!("  (The agent may have returned content without writing a file,");
        println!("   or execution failed before the write step.)");
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

    // Clean up the smoke task from the DB so it doesn't clutter the queue.
    let _ = task_store::delete_task(&conn, &task_id);
    println!("✓ Cleaned up smoke task from DB.");
}
