/// Create and execute reddit_reply tasks for selected opportunities from a completed
/// reddit_opportunity_search task.
///
/// Usage:
///   cargo run --example run_reddit_replies -- <parent_task_id> <post_id1> <post_id2> ...
///
/// Example:
///   cargo run --example run_reddit_replies -- task-94187aa2-542d-47f3-90af-3e0220460281 1uul2zh 1uwby1x 1uvihti 1uwmgqv
use std::path::Path;

use pageseeds_lib::db;
use pageseeds_lib::engine::executor;
use pageseeds_lib::reddit::spawner::create_reply_tasks_from_opportunities;

#[tokio::main]
async fn main() {
    env_logger::init();

    let db_path = Path::new(
        "/Users/fstrauf/Library/Application Support/com.pageseeds.app/pageseeds.db",
    );

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "Usage: cargo run --example run_reddit_replies -- <parent_task_id> <post_id1> [post_id2] ..."
        );
        std::process::exit(1);
    }

    let parent_task_id = &args[1];
    let post_ids: Vec<String> = args[2..].to_vec();

    println!("========================================");
    println!("Reddit Reply Runner");
    println!("DB: {}", db_path.display());
    println!("Parent task: {}", parent_task_id);
    println!("Post IDs: {:?}", post_ids);
    println!("========================================\n");

    println!("[1/3] Opening database...");
    let conn = rusqlite::Connection::open(db_path).expect("Failed to open DB");
    db::init_with_conn(&conn).expect("Failed to init DB schema");
    println!("      ✅ Opened\n");

    println!("[2/3] Creating reply tasks from selected opportunities...");
    let tasks = match create_reply_tasks_from_opportunities(&conn, parent_task_id, &post_ids) {
        Ok(t) => {
            println!("      ✅ Created {} reply task(s)\n", t.len());
            t
        }
        Err(e) => {
            println!("      ❌ Failed to create reply tasks: {}\n", e);
            std::process::exit(1);
        }
    };

    println!("[3/3] Executing reply tasks (posting to Reddit)...");
    let mut results = Vec::new();
    for task in &tasks {
        println!("\n  → Task {} — posting reply...", task.id);
        let start = std::time::Instant::now();
        let result = executor::execute_task(&conn, &task.id).await;
        let elapsed = start.elapsed();

        match result {
            Ok(exec_result) => {
                println!("      Status: {} ({:.1}s)", exec_result.message, elapsed.as_secs_f64());
                results.push((task.id.clone(), exec_result.success, exec_result.message));
            }
            Err(e) => {
                println!("      Error: {} ({:.1}s)", e, elapsed.as_secs_f64());
                results.push((task.id.clone(), false, e));
            }
        }
    }

    println!("\n========================================");
    println!("Summary");
    println!("========================================");
    let success_count = results.iter().filter(|(_, s, _)| *s).count();
    println!("Posted: {}/{}", success_count, results.len());
    for (id, success, message) in &results {
        let icon = if *success { "✅" } else { "❌" };
        println!("  {} {} — {}", icon, id, message);
    }
    println!("========================================");
}
