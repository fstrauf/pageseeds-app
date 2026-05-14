/// Smoke test for the full content-review pipeline.
///
/// Run with:
///   cargo run --bin content_review_smoke
///
/// What it does:
///   1. Opens a temp in-memory SQLite DB
///   2. Registers the learnedlate project (no Tauri runtime needed)
///   3. Creates a `content_review` task
///   4. Executes the task through execute_task (same path as the real app)
///   5. Prints each step result + the final task status
///   6. Prints the created fix_content_article tasks and their recommendations artifacts
///
/// To test with a different project, change PROJECT_PATH below.
use pageseeds_lib::{
    db,
    engine::{executor, task_store},
    models::{
        project::{Project, ProjectMode},
        task::{
            AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
            TaskStatus,
        },
    },
};

const PROJECT_PATH: &str = "/Users/fstrauf/01_code/learnedlate";
const AGENT_PROVIDER: &str = "copilot"; // change to "kimi" if needed

#[tokio::main]
async fn main() {
    // ── Logger ──────────────────────────────────────────────────────────────
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    // ── DB ───────────────────────────────────────────────────────────────────
    let db_path = std::path::Path::new("/tmp/smoke_test_pageseeds.db");
    // Fresh db each run
    let _ = std::fs::remove_file(db_path);
    let conn = db::init(db_path).expect("DB init failed");

    // ── Project ───────────────────────────────────────────────────────────────
    let manifest_path = format!("{}/.github/automation/manifest.json", PROJECT_PATH);
    let manifest: serde_json::Value = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let site_url = manifest
        .get("url")
        .or_else(|| manifest.get("gsc_site"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let project = Project {
        id: "smoke-project".to_string(),
        name: "LearnedLate (smoke test)".to_string(),
        path: PROJECT_PATH.to_string(),
        content_dir: None,
        site_url: if site_url.is_empty() {
            None
        } else {
            Some(site_url)
        },
        site_id: None,
        sitemap_url: None,
        project_mode: ProjectMode::Workspace,
        active: true,
        agent_provider: Some(AGENT_PROVIDER.to_string()),
        seo_provider: Some("ahrefs".to_string()),
    };
    task_store::create_project(&conn, &project).expect("create_project failed");
    println!("✓ Project registered: {} ({})", project.name, PROJECT_PATH);

    // ── Task ─────────────────────────────────────────────────────────────────
    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!(
        "smoke-content-review-{}",
        chrono::Utc::now().timestamp_millis()
    );
    let task = Task {
        id: task_id.clone(),
        task_type: "content_review".to_string(),
        phase: "investigation".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::FollowUpTasks,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        agent_policy: AgentPolicy::Required,
        title: Some("Smoke: Content Review".to_string()),
        description: None,
        project_id: "smoke-project".to_string(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
        not_before: None,
    };
    task_store::create_task(&conn, &task).expect("create_task failed");
    println!("✓ Task created: {}\n", task_id);

    // ── Execute ───────────────────────────────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Executing content_review...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let start = std::time::Instant::now();
    let result = executor::execute_task(&conn, &task_id)
        .await
        .expect("execute_task panic");

    println!("\n━━━━━━━━━━━━━━━━━━━  Results  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
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
        println!(
            "  {} [{}] {} — {}",
            icon, step.status, step.step_name, step.message
        );
        if let Some(ref out) = step.output {
            let preview: String = out.chars().take(400).collect();
            println!("    output: {}", preview);
            if out.len() > 400 {
                println!("    ... ({} chars total)", out.len());
            }
        }
        println!();
    }

    // ── Check spawned apply task ──────────────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━  Follow-up  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    let all_tasks = task_store::list_tasks(&conn, "smoke-project").expect("list_tasks failed");
    let fix_tasks: Vec<&Task> = all_tasks
        .iter()
        .filter(|t| t.task_type == "fix_content_article")
        .collect();

    if fix_tasks.is_empty() {
        println!("✗  No fix_content_article tasks were created.");
        println!("   Possible reasons:");
        println!("   - content_review_recommend step failed");
        println!("   - No priority articles found (all healthy)");
        println!("   - recommendations.json has 0 articles");
    } else {
        for t in &fix_tasks {
            println!("✓ Fix task created: {} (status={})", t.id, t.status);
            println!("  title: {:?}", t.title);
            for a in &t.artifacts {
                println!(
                    "  artifact: key={} path={:?} content_len={}",
                    a.key,
                    a.path,
                    a.content.as_ref().map(|c| c.len()).unwrap_or(0),
                );
                if a.key == "recommendations" {
                    if let Some(ref c) = a.content {
                        println!("\n  ── recommendations.json (first 1500 chars) ──");
                        let preview: String = c.as_str().chars().take(1500).collect();
                        println!("{}", preview);
                        if c.len() > 1500 {
                            println!("  ... ({} chars total)", c.len());
                        }
                    }
                }
            }
        }
        println!();
        println!("Next: run a fix_content_article task to edit the actual article files.");
        println!("  let fix_id = {:?};", fix_tasks[0].id);
        println!("  executor::execute_task(&conn, &fix_id);");
    }

    // ── Check recommendations.json on disk ───────────────────────────────────
    let rec_path = format!("{}/.github/automation/recommendations.json", PROJECT_PATH);
    if let Ok(s) = std::fs::read_to_string(&rec_path) {
        let n: usize = serde_json::from_str::<serde_json::Value>(&s)
            .ok()
            .and_then(|v| v["articles"].as_array().map(|a| a.len()))
            .unwrap_or(0);
        println!(
            "\n✓ recommendations.json on disk: {} articles ({} bytes)",
            n,
            s.len()
        );
    } else {
        println!("\n! recommendations.json not found on disk at {}", rec_path);
    }

    // Final verdict
    println!(
        "\n{}",
        if result.success {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );
    std::process::exit(if result.success { 0 } else { 1 });
}
