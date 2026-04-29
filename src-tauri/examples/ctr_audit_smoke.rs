/// Smoke test for the CTR audit pipeline on a real project.
///
/// Run with:
///   cargo run --example ctr_audit_smoke
///
/// What it does:
///   1. Opens a temp SQLite DB
///   2. Registers the supplylah project from the real app DB
///   3. Creates a `ctr_audit` task
///   4. Executes the task (deterministic build_context + optional agentic analyze)
///   5. Prints each step result + context JSON
///   6. Prints any spawned fix tasks
use pageseeds_lib::{
    db,
    engine::{executor, task_store},
    models::{
        project::Project,
        task::{AgentPolicy, ExecutionMode, Priority, Task, TaskRun, TaskStatus},
    },
};

const PROJECT_ID: &str = "supplylah";
const PROJECT_PATH: &str = "/Users/fstrauf/01_code/bigPond";
const AGENT_PROVIDER: &str = "copilot";

fn main() {
    // ── Tokio runtime ─────────────────────────────────────────────────────────
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();

    // ── Logger ────────────────────────────────────────────────────────────────
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    // ── DB ────────────────────────────────────────────────────────────────────
    let db_path = std::path::Path::new("/Users/fstrauf/01_code/pageseeds-app/smoke_ctr_audit.db");
    let _ = std::fs::remove_file(db_path);
    let conn = db::init(db_path).expect("DB init failed");

    // ── Project ───────────────────────────────────────────────────────────────
    let project = Project {
        id: PROJECT_ID.to_string(),
        name: "Supplylah".to_string(),
        path: PROJECT_PATH.to_string(),
        content_dir: Some("content/blog".to_string()),
        site_url: None,
        site_id: None,
        sitemap_url: None,
        project_mode: pageseeds_lib::models::project::ProjectMode::Workspace,
        active: true,
        agent_provider: Some(AGENT_PROVIDER.to_string()),
        seo_provider: Some("ahrefs".to_string()),
    };
    task_store::create_project(&conn, &project).expect("create_project failed");
    println!(
        "✓ Project registered: {} ({})\n",
        project.name, PROJECT_PATH
    );

    // ── Task ──────────────────────────────────────────────────────────────────
    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!("smoke-ctr-audit-{}", chrono::Utc::now().timestamp_millis());
    let task = Task {
        id: task_id.clone(),
        task_type: "ctr_audit".to_string(),
        phase: "investigation".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        execution_mode: ExecutionMode::Automatic,
        agent_policy: AgentPolicy::Required,
        title: Some("Smoke: CTR Audit".to_string()),
        description: None,
        project_id: PROJECT_ID.to_string(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
    };
    task_store::create_task(&conn, &task).expect("create_task failed");
    println!("✓ Task created: {}\n", task_id);

    // ── Execute ───────────────────────────────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Executing ctr_audit...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let start = std::time::Instant::now();
    let result = rt
        .block_on(async { executor::execute_task(&conn, &task_id).await })
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
            let preview: String = out.chars().take(800).collect();
            println!("    output: {}", preview);
            if out.len() > 800 {
                println!("    ... ({} chars total)", out.len());
            }
        }
        println!();
    }

    // ── Check context JSON on disk ────────────────────────────────────────────
    let ctx_path = format!("{}/.github/automation/ctr_audit_context.json", PROJECT_PATH);
    if let Ok(s) = std::fs::read_to_string(&ctx_path) {
        let n: usize = serde_json::from_str::<serde_json::Value>(&s)
            .ok()
            .and_then(|v| v["total_articles"].as_i64().map(|n| n as usize))
            .unwrap_or(0);
        println!(
            "✓ ctr_audit_context.json on disk: {} articles ({} bytes)",
            n,
            s.len()
        );
    } else {
        println!("! ctr_audit_context.json not found on disk at {}", ctx_path);
    }

    // ── Check spawned fix tasks ───────────────────────────────────────────────
    println!("\n━━━━━━━━━━━━━━━━━━━  Follow-up  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    let all_tasks = task_store::list_tasks(&conn, PROJECT_ID).expect("list_tasks failed");
    let fix_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| t.task_type.starts_with("fix_"))
        .collect();

    if fix_tasks.is_empty() {
        println!("! No fix tasks were spawned.");
    } else {
        for t in &fix_tasks {
            println!(
                "✓ Fix task created: {} (type={} status={})",
                t.id, t.task_type, t.status
            );
            if let Some(ref title) = t.title {
                println!("  title: {}", title);
            }
        }
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
