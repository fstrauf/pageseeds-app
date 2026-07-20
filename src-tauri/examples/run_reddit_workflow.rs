/// Run the Reddit opportunity-search workflow for the days_to_expiry project.
///
/// Usage:
///   cargo run --example run_reddit_workflow
///
/// This uses the real PageSeeds SQLite database and project config, so it will
/// call the Kimi agent and the Reddit API.
use std::path::Path;

use pageseeds_lib::db;
use pageseeds_lib::engine::executor;
use pageseeds_lib::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use pageseeds_lib::models::task::{AgentPolicy, Priority, TaskStatus};

#[tokio::main]
async fn main() {
    env_logger::init();

    let db_path = Path::new(
        "/Users/fstrauf/Library/Application Support/com.pageseeds.app/pageseeds.db",
    );
    let project_id = "days_to_expiry";

    println!("========================================");
    println!("Reddit workflow — Days to Expiry");
    println!("DB: {}", db_path.display());
    println!("Project: {}", project_id);
    println!("========================================\n");

    println!("[1/4] Opening database...");
    let conn = rusqlite::Connection::open(db_path).expect("Failed to open DB");
    db::init_with_conn(&conn).expect("Failed to init DB schema");
    println!("      ✅ Opened\n");

    println!("[2/4] Spawning reddit_opportunity_search task...");
    let now = chrono::Utc::now();
    let title = format!("Reddit Search — {}", now.format("%-d/%-m/%Y"));
    let idempotency_key = format!(
        "reddit_opportunity_search:{}:{}",
        project_id,
        now.format("%Y-%m-%d")
    );

    let spec = TaskSpec {
        project_id: project_id.to_string(),
        task_type: "reddit_opportunity_search".to_string(),
        title: Some(title),
        description: Some("Weekly Reddit opportunity search for Days to Expiry".to_string()),
        priority: Priority::High,
        agent_policy: AgentPolicy::Optional,
        idempotency_key: Some(idempotency_key),
        dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
        ..Default::default()
    };

    let task = match TaskSpawner::spawn(&conn, spec) {
        Ok(t) => {
            println!("      ✅ Created task {}\n", t.id);
            t
        }
        Err(e) => {
            println!("      ❌ Failed to spawn task: {}\n", e);
            std::process::exit(1);
        }
    };

    println!("[3/4] Executing workflow...");
    println!("      This calls the Kimi agent and Reddit API.");
    let start = std::time::Instant::now();
    let result = executor::execute_task(&conn, &task.id).await;
    let elapsed = start.elapsed();
    println!("      ⏱️  Done in {:.1}s\n", elapsed.as_secs_f64());

    println!("========================================");
    println!("Execution Result");
    println!("========================================");

    match result {
        Ok(exec_result) => {
            println!("Success: {}", exec_result.success);
            println!("Message: {}", exec_result.message);
            println!("Steps:   {}\n", exec_result.steps.len());

            for (i, step) in exec_result.steps.iter().enumerate() {
                println!(
                    "  Step {}: {} ({})",
                    i + 1,
                    step.step_name,
                    step.kind
                );
                println!("    Status:  {}", step.status);
                println!("    Message: {}", step.message);
                if let Some(ref output) = step.output {
                    let preview = if output.len() > 300 {
                        format!("{}... ({} chars)", &output[..300], output.len())
                    } else {
                        output.clone()
                    };
                    println!("    Output:  {}", preview);
                }
            }

            if !exec_result.follow_up_tasks.is_empty() {
                println!(
                    "\nFollow-up tasks created: {}",
                    exec_result.follow_up_tasks.len()
                );
                for t in &exec_result.follow_up_tasks {
                    println!("  - {} ({}) [{}]", t.title, t.task_type, t.status);
                }
            }

            // Show final task status
            let final_status: String = conn
                .query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    [&task.id],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| "unknown".to_string());
            println!("\nFinal task status: {}", final_status);

            // Count pending opportunities
            let pending_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM reddit_opportunities WHERE project_id = ?1 AND reply_status = 'pending'",
                    [project_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            println!("Pending opportunities in DB: {}", pending_count);

            if exec_result.success {
                println!("\n✅ Reddit workflow completed successfully");
            } else {
                println!("\n⚠️  Reddit workflow completed but reported failure");
            }
        }
        Err(e) => {
            println!("❌ Execution error: {}", e);
            std::process::exit(1);
        }
    }

    println!("\n========================================");
    println!("Task ID: {}", task.id);
    println!("========================================");
}
