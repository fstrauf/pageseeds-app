/// Smoke test for the cannibalization audit deterministic step.
///
/// Run with:
///   cargo run --example can_audit_smoke
use pageseeds_lib::{
    db,
    engine::{executor, task_store},
    models::{
        project::Project,
        task::{AgentPolicy, FollowUpPolicy, TaskRunPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskStatus},
    },
};

const PROJECT_ID: &str = "supplylah";
const PROJECT_PATH: &str = "/Users/fstrauf/01_code/bigPond";

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();

    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    let db_path = std::path::Path::new("/Users/fstrauf/01_code/pageseeds-app/smoke_can_audit.db");
    let _ = std::fs::remove_file(db_path);
    let conn = db::init(db_path).expect("DB init failed");

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
        agent_provider: Some("copilot".to_string()),
        seo_provider: Some("ahrefs".to_string()),
    };
    task_store::create_project(&conn, &project).expect("create_project failed");
    println!("✓ Project registered\n");

    let now = chrono::Utc::now().to_rfc3339();
    let task_id = format!("smoke-can-audit-{}", chrono::Utc::now().timestamp_millis());
    let task = Task {
        id: task_id.clone(),
        task_type: "cannibalization_audit".to_string(),
        phase: "investigation".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Required,
        title: Some("Smoke: Cannibalization Audit".to_string()),
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

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Executing cannibalization_audit...");
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

    let ctx_path = format!(
        "{}/.github/automation/cannibalization_audit_context.json",
        PROJECT_PATH
    );
    if let Ok(s) = std::fs::read_to_string(&ctx_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            let pairs = v["similarity_pairs"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            let groups = v["keyword_groups"]
                .as_object()
                .map(|o| o.len())
                .unwrap_or(0);
            println!(
                "✓ cannibalization_audit_context.json: {} pairs, {} groups ({} bytes)",
                pairs,
                groups,
                s.len()
            );
        }
    } else {
        println!("! context not found");
    }

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
