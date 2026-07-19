/// Execute existing reddit_reply tasks by their task IDs.
///
/// Usage:
///   cargo run --example run_reddit_reply_tasks -- <task_id1> <task_id2> ...
use std::path::Path;

use pageseeds_lib::db;
use pageseeds_lib::engine::executor;

#[tokio::main]
async fn main() {
    env_logger::init();

    let db_path = Path::new(
        "/Users/fstrauf/Library/Application Support/com.pageseeds.app/pageseeds.db",
    );

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --example run_reddit_reply_tasks -- <task_id1> [task_id2] ...");
        std::process::exit(1);
    }

    let task_ids: Vec<String> = args[1..].to_vec();

    println!("========================================");
    println!("Reddit Reply Task Executor");
    println!("DB: {}", db_path.display());
    println!("Tasks: {:?}", task_ids);
    println!("========================================\n");

    println!("[1/2] Opening database...");
    let conn = rusqlite::Connection::open(db_path).expect("Failed to open DB");
    db::init_with_conn(&conn).expect("Failed to init DB schema");
    println!("      ✅ Opened\n");

    println!("[2/2] Executing reply tasks...");
    let mut results = Vec::new();
    for task_id in &task_ids {
        println!("\n  → Task {} — posting reply...", task_id);
        let start = std::time::Instant::now();
        let result = executor::execute_task(&conn, task_id).await;
        let elapsed = start.elapsed();

        match result {
            Ok(exec_result) => {
                println!(
                    "      Status: {} ({:.1}s)",
                    exec_result.message,
                    elapsed.as_secs_f64()
                );
                results.push((task_id.clone(), exec_result.success, exec_result.message));
            }
            Err(e) => {
                println!("      Error: {} ({:.1}s)", e, elapsed.as_secs_f64());
                results.push((task_id.clone(), false, e));
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
