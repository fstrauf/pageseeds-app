/// Smoke test for the feature-spec pipeline in isolation.
///
/// Run with:
///   cargo run --bin feature_spec_smoke
///
/// What it does:
///   1. Opens the real PageSeeds SQLite DB
///   2. Finds the nz-coffee-hub project (or falls back to a temp project)
///   3. Creates a `generate_feature_spec` task
///   4. Executes the task directly through executor::execute_task
///   5. Prints the result, step breakdown, and the generated spec file path
///
/// Change the constants below to target a different project or provider.
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

/// Path to the project to investigate. Change this to test a different repo.
const PROJECT_PATH: &str = "/Users/fstrauf/01_code/nz-coffee-hub";

/// Project ID in the DB. If the project doesn't exist in the real DB,
/// set this to "" and the smoke test will create a temp project.
const PROJECT_ID: &str = "coffee";

/// LLM provider to use. Options: "kimi", "copilot", "claude", "openai", "ollama".
/// For tool calling, "kimi" (bridge) or "claude" are recommended.
const AGENT_PROVIDER: &str = "kimi";

/// Use the real DB (true) or a temp in-memory DB (false).
/// Real DB is recommended so articles are already populated.
const USE_REAL_DB: bool = true;

#[tokio::main]
async fn main() {
    // ── Logger ──────────────────────────────────────────────────────────────
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    // ── DB ───────────────────────────────────────────────────────────────────
    let (conn, project) = if USE_REAL_DB {
        let db_path = pageseeds_lib::db::default_db_path();
        let conn = db::init(&db_path).expect("DB init failed");

        // Try to find the existing project
        let project = match task_store::get_project(&conn, PROJECT_ID) {
            Ok(p) => {
                println!("✓ Using existing project: {} ({})", p.name, p.path);
                p
            }
            Err(_) => {
                println!("! Project '{}' not found in DB. Creating temp project...", PROJECT_ID);
                create_temp_project(&conn)
            }
        };
        (conn, project)
    } else {
        let db_path = std::path::Path::new("/tmp/feature_spec_smoke.db");
        let _ = std::fs::remove_file(db_path);
        let conn = db::init(db_path).expect("DB init failed");
        let project = create_temp_project(&conn);
        (conn, project)
    };

    // ── Task ─────────────────────────────────────────────────────────────────
    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!(
        "smoke-feature-spec-{}",
        chrono::Utc::now().timestamp_millis()
    );
    let task = Task {
        id: task_id.clone(),
        task_type: "generate_feature_spec".to_string(),
        phase: "investigation".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Required,
        title: Some("Smoke: Feature Spec".to_string()),
        description: Some("Isolated test of the feature spec generator".to_string()),
        project_id: project.id.clone(),
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
    println!("  Executing generate_feature_spec...");
    println!("  Project : {} ({})", project.name, project.path);
    println!("  Provider: {}", AGENT_PROVIDER);
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

    // ── Check generated spec on disk ─────────────────────────────────────────
    println!("━━━━━━━━━━━━━━━━━━━  Generated Spec  ━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    let spec_path = std::path::PathBuf::from(PROJECT_PATH)
        .join(".github")
        .join("automation")
        .join(format!("seo_feature_spec_{}.md", task_id));
    let latest_path = std::path::PathBuf::from(PROJECT_PATH)
        .join(".github")
        .join("automation")
        .join("seo_feature_spec.md");

    if spec_path.exists() {
        let content = std::fs::read_to_string(&spec_path).unwrap_or_default();
        let word_count = content.split_whitespace().count();
        println!("✓ Spec written: {}", spec_path.display());
        println!("  Words    : {}", word_count);
        println!("  Bytes    : {}", content.len());
        println!("  Hard link: {}", latest_path.display());
        println!();
        println!("  ── First 800 chars ──");
        let preview: String = content.chars().take(800).collect();
        println!("{}", preview);
        if content.len() > 800 {
            println!("  ... ({} chars total)", content.len());
        }
    } else {
        println!("✗ Spec not found at {}", spec_path.display());
        println!("  (Task may have failed before writing, or no findings were generated.)");
    }

    // ── Check task artifacts ─────────────────────────────────────────────────
    println!("\n━━━━━━━━━━━━━━━━━━━  Task Artifacts  ━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    let saved_task = task_store::get_task(&conn, &task_id).expect("get_task failed");
    if saved_task.artifacts.is_empty() {
        println!("  (no artifacts)");
    } else {
        for a in &saved_task.artifacts {
            println!(
                "  • key={} type={:?} path={:?} content_len={}",
                a.key,
                a.artifact_type,
                a.path,
                a.content.as_ref().map(|c| c.len()).unwrap_or(0)
            );
        }
    }

    println!();
}

fn create_temp_project(conn: &rusqlite::Connection) -> Project {
    let project = Project {
        id: "smoke-feature-spec".to_string(),
        name: "Smoke Test Project".to_string(),
        path: PROJECT_PATH.to_string(),
        content_dir: None,
        site_url: None,
        site_id: None,
        sitemap_url: None,
        project_mode: ProjectMode::Workspace,
        active: true,
        agent_provider: Some(AGENT_PROVIDER.to_string()),
        seo_provider: Some("ahrefs".to_string()),
    };
    task_store::create_project(conn, &project).expect("create_project failed");
    println!("✓ Temp project registered: {} ({})", project.name, project.path);
    project
}
