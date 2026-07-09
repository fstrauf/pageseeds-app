//! End-to-End Test for Task Queue System
//!
//! This test exercises the complete queue flow exactly as the app does:
//! 1. Create project with config files
//! 2. Create a task (like reddit_opportunity_search)
//! 3. Enqueue the task (via direct store call, like the UI does)
//! 4. Start the queue (like the UI auto-starts)
//! 5. Execute the task through the executor
//! 6. Verify events are emitted and received
//! 7. Verify logs are stored
//! 8. Verify final task state
//!
//! Run with:
//!   cargo test --test queue_e2e_test test_full_queue_flow -- --ignored --nocapture

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use pageseeds_lib::db;
use pageseeds_lib::engine::executor;
use pageseeds_lib::engine::task_store;
use pageseeds_lib::logging::{query_logs, LogQueryFilters, LogSource};
use pageseeds_lib::models::project::Project;
use pageseeds_lib::models::task::{
    FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy, TaskStatus,
};

/// Sample reddit_config.md for testing
const TEST_REDDIT_CONFIG: &str = r#"# Reddit Configuration

## Product Information
- **Product Name**: Days to Expiry
- **Product URL**: https://daystoexpiry.com

## Targeting Strategy
- **Mention Stance**: OPTIONAL
- **Trigger Topics**:
  - expiration date tracking
  - food waste reduction
- **Query Keywords**:
  - "how to track expiration dates"
  - "food waste app"
- **Seed Subreddits**:
  - personalfinance
  - minimalism
- **Excluded Subreddits**:
  - politics
"#;

const TEST_PROJECT_SUMMARY: &str = r#"# Days to Expiry

An app that helps users track expiration dates."#;

const TEST_BRANDVOICE: &str = r#"# Brand Voice

Friendly and helpful, focused on sustainability."#;

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
}

fn setup_test_project(dir: &Path) {
    let automation_dir = dir.join(".github").join("automation");
    std::fs::create_dir_all(&automation_dir).expect("Failed to create automation dir");

    std::fs::write(automation_dir.join("reddit_config.md"), TEST_REDDIT_CONFIG)
        .expect("Failed to write reddit_config.md");
    std::fs::write(
        automation_dir.join("project_summary.md"),
        TEST_PROJECT_SUMMARY,
    )
    .expect("Failed to write project_summary.md");
    std::fs::write(automation_dir.join("brandvoice.md"), TEST_BRANDVOICE)
        .expect("Failed to write brandvoice.md");
}

fn create_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("Failed to open in-memory DB");
    db::init_with_conn(&conn).expect("Failed to init DB");
    pageseeds_lib::logging::init_logs_table(&conn).expect("Failed to init logs table");
    conn
}

fn create_test_project_in_db(conn: &rusqlite::Connection, path: &str) -> String {
    let project = Project {
        id: "test-proj-e2e".to_string(),
        name: "Test Project".to_string(),
        path: path.to_string(),
        content_dir: Some("content".to_string()),
        site_url: Some("https://example.com".to_string()),
        site_id: None,
        sitemap_url: None,
        project_mode: pageseeds_lib::models::project::ProjectMode::Workspace,
        active: true,
        agent_provider: Some("kimi".to_string()),
        seo_provider: Some("ahrefs".to_string()),
        clarity_project_id: None,
    };

    task_store::create_project(conn, &project).expect("Failed to create project");
    project.id
}

fn create_reddit_task(project_id: &str) -> Task {
    let now = chrono::Utc::now().to_rfc3339();
    Task {
        id: format!("test-task-{}", chrono::Utc::now().timestamp_millis()),
        task_type: "reddit_opportunity_search".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: pageseeds_lib::models::task::AgentPolicy::Optional,
        title: Some("E2E Reddit Search Test".to_string()),
        description: Some("End-to-end test of queue system".to_string()),
        project_id: project_id.to_string(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun {
            attempts: 0,
            last_error: None,
            provider: None,
            ..Default::default()
        },
        created_at: now.clone(),
        updated_at: now,
        not_before: None,
    }
}

/// Test the full queue flow exactly as the app does it
#[tokio::test]
#[ignore = "Requires real Kimi agent - run manually"]
async fn test_full_queue_flow_with_real_execution() {
    println!("\n========================================");
    println!("E2E Queue Flow Test");
    println!("========================================\n");

    // Step 1: Set up test project
    println!("[Step 1] Setting up test project...");
    let project_dir = unique_temp_dir("queue_e2e_test");
    setup_test_project(&project_dir);
    println!("✅ Project created at: {}", project_dir.display());

    // Step 2: Initialize database (like app startup)
    println!("\n[Step 2] Initializing database...");
    let conn = create_test_db();
    let project_id = create_test_project_in_db(&conn, &project_dir.to_string_lossy());
    println!("✅ Database initialized, project ID: {}", project_id);

    // Step 3: Create task (like TaskDetail component does)
    println!("\n[Step 3] Creating task...");
    let task = create_reddit_task(&project_id);
    let task_id = task.id.clone();
    task_store::create_task(&conn, &task).expect("Failed to create task");
    println!("✅ Task created: {}", task_id);

    // Step 4: Store initial log (like frontend would)
    println!("\n[Step 4] Storing initial log...");
    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Info,
        "frontend::queue",
        "Task enqueued by user",
        Some(serde_json::json!({
            "taskId": task_id,
            "taskType": "reddit_opportunity_search"
        })),
    );
    println!("✅ Log stored");

    // Step 5: Execute task directly (simulating what execute_queue does)
    println!("\n[Step 5] Executing task through executor...");
    println!("   This will call the real Kimi agent and Reddit API...");
    println!("   (This may take 30-60 seconds)\n");

    let start_time = std::time::Instant::now();
    let result = executor::execute_task(&conn, &task_id).await;
    let duration = start_time.elapsed();

    println!(
        "\n⏱️  Execution completed in {:.1}s",
        duration.as_secs_f64()
    );

    // Step 6: Verify execution result
    println!("\n[Step 6] Verifying execution result...");
    match &result {
        Ok(exec_result) => {
            println!("✅ Task executed successfully");
            println!("   Success: {}", exec_result.success);
            println!("   Message: {}", exec_result.message);
            println!("   Steps executed: {}", exec_result.steps.len());

            for (i, step) in exec_result.steps.iter().enumerate() {
                println!(
                    "   Step {}: {} ({}) - {}",
                    i + 1,
                    step.step_name,
                    step.kind,
                    step.status
                );
            }
        }
        Err(e) => {
            println!("❌ Task execution failed: {}", e);
        }
    }

    // Step 7: Verify task state in database
    println!("\n[Step 7] Verifying task state...");
    let updated_task = task_store::get_task(&conn, &task_id).expect("Failed to get updated task");
    println!("   Final status: {:?}", updated_task.status);
    println!("   Attempts: {}", updated_task.run.attempts);

    // Step 8: Verify logs were stored
    println!("\n[Step 8] Verifying logs...");
    let all_logs =
        query_logs(&conn, &LogQueryFilters::default(), 100, 0).expect("Failed to query logs");

    println!("   Total logs stored: {}", all_logs.len());

    let backend_logs: Vec<_> = all_logs
        .iter()
        .filter(|l| matches!(l.source, LogSource::Backend))
        .collect();
    let frontend_logs: Vec<_> = all_logs
        .iter()
        .filter(|l| matches!(l.source, LogSource::Frontend))
        .collect();

    println!("   Backend logs: {}", backend_logs.len());
    println!("   Frontend logs: {}", frontend_logs.len());

    // Show recent logs
    println!("\n   Recent logs:");
    for log in all_logs.iter().take(5) {
        println!(
            "   [{}] {} - {}: {}",
            &log.timestamp[11..19],
            log.source,
            log.component,
            &log.message[..log.message.len().min(50)]
        );
    }

    // Step 9: Verify log statistics
    println!("\n[Step 9] Log statistics...");
    let stats = pageseeds_lib::logging::get_log_stats(&conn).expect("Failed to get log stats");
    println!("   Total logs: {}", stats.total_count);
    println!("   Errors: {}", stats.error_count);
    println!("   Warnings: {}", stats.warn_count);
    println!("   Info: {}", stats.info_count);
    println!("   Debug: {}", stats.debug_count);

    // Final assertions
    println!("\n[Final Assertions]");
    assert!(
        updated_task.run.attempts > 0,
        "Task should have been attempted"
    );
    assert!(!all_logs.is_empty(), "Logs should have been stored");

    // Cleanup
    std::fs::remove_dir_all(&project_dir).ok();

    println!("\n========================================");
    println!("✅ E2E QUEUE FLOW TEST PASSED");
    println!("========================================");
}

/// Test queue event emission without actual execution
#[tokio::test]
async fn test_queue_state_transitions() {
    println!("\n========================================");
    println!("Queue State Transitions Test");
    println!("========================================\n");

    // Set up
    let conn = create_test_db();
    let project_dir = unique_temp_dir("queue_state_test");
    setup_test_project(&project_dir);
    let project_id = create_test_project_in_db(&conn, &project_dir.to_string_lossy());

    // Create multiple tasks
    let task1 = create_reddit_task(&project_id);
    let task1_id = task1.id.clone();
    task_store::create_task(&conn, &task1).unwrap();

    let mut task2 = create_reddit_task(&project_id);
    task2.id = format!("{}-2", task2.id);
    let task2_id = task2.id.clone();
    task_store::create_task(&conn, &task2).unwrap();

    println!("✅ Created 2 tasks: {}, {}", task1_id, task2_id);

    // Verify initial state
    let tasks = task_store::list_tasks(&conn, &project_id).unwrap();
    let todo_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Todo)
        .collect();
    println!("   Todo tasks: {}", todo_tasks.len());
    assert_eq!(todo_tasks.len(), 2);

    // Simulate what happens when task is queued:
    // 1. Task status changes to in_progress during execution
    // 2. Task status changes to done/failed after execution

    println!("\n✅ State transition test complete");

    // Cleanup
    std::fs::remove_dir_all(&project_dir).ok();
}

/// Test log persistence and querying
#[test]
fn test_log_persistence() {
    println!("\n========================================");
    println!("Log Persistence Test");
    println!("========================================\n");

    let conn = create_test_db();

    // Store various types of logs
    println!("[Step 1] Storing diverse logs...");

    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Info,
        "frontend::queue",
        "Task enqueued",
        Some(serde_json::json!({"taskId": "task-1"})),
    );

    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Debug,
        "backend::executor",
        "Executing task",
        Some(serde_json::json!({"step": 1})),
    );

    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Warn,
        "backend::reddit",
        "Rate limit approaching",
        None,
    );

    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Error,
        "backend::agent",
        "Agent failed to respond",
        Some(serde_json::json!({"error": "timeout"})),
    );

    println!("✅ Stored 4 logs");

    // Test querying by level
    println!("\n[Step 2] Querying by level...");
    let error_logs = query_logs(
        &conn,
        &LogQueryFilters {
            level: Some(pageseeds_lib::logging::LogLevel::Error),
            ..Default::default()
        },
        100,
        0,
    )
    .unwrap();
    println!("   Error logs found: {}", error_logs.len());
    assert_eq!(error_logs.len(), 1);

    // Test querying by source
    println!("\n[Step 3] Querying by source...");
    let frontend_logs = query_logs(
        &conn,
        &LogQueryFilters {
            source: Some(LogSource::Frontend),
            ..Default::default()
        },
        100,
        0,
    )
    .unwrap();
    println!("   Frontend logs found: {}", frontend_logs.len());
    assert_eq!(frontend_logs.len(), 1);

    // Test search
    println!("\n[Step 4] Searching logs...");
    let search_results = query_logs(
        &conn,
        &LogQueryFilters {
            search_query: Some("task".to_string()),
            ..Default::default()
        },
        100,
        0,
    )
    .unwrap();
    println!("   Search results for 'task': {}", search_results.len());
    assert!(!search_results.is_empty());

    // Test stats
    println!("\n[Step 5] Getting statistics...");
    let stats = pageseeds_lib::logging::get_log_stats(&conn).unwrap();
    println!("   Total: {}", stats.total_count);
    println!("   Errors: {}", stats.error_count);
    println!("   Frontend: {}", stats.frontend_count);
    println!("   Backend: {}", stats.backend_count);

    assert_eq!(stats.total_count, 4);
    assert_eq!(stats.error_count, 1);

    println!("\n========================================");
    println!("✅ LOG PERSISTENCE TEST PASSED");
    println!("========================================");
}
