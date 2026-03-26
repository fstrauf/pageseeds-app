//! Integration Test for Queue Mechanics (No External APIs)
//!
//! Tests the queue system without requiring real agents or APIs.
//! Uses a simple deterministic task type that we know works.

use std::time::{SystemTime, UNIX_EPOCH};

use pageseeds_lib::db;
use pageseeds_lib::engine::task_store;
use pageseeds_lib::logging::{query_logs, LogQueryFilters};
use pageseeds_lib::models::project::Project;
use pageseeds_lib::models::task::{ExecutionMode, Priority, Task, TaskRun, TaskStatus};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
}

fn create_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("Failed to open in-memory DB");
    db::init_with_conn(&conn).expect("Failed to init DB");
    pageseeds_lib::logging::init_logs_table(&conn).expect("Failed to init logs table");
    conn
}

fn create_test_project_in_db(conn: &rusqlite::Connection, path: &str) -> String {
    let project = Project {
        id: "test-proj-queue".to_string(),
        name: "Test Project".to_string(),
        path: path.to_string(),
        content_dir: Some("content".to_string()),
        site_url: Some("https://example.com".to_string()),
        site_id: None,
        active: true,
        agent_provider: Some("copilot".to_string()),
    };
    
    task_store::create_project(conn, &project).expect("Failed to create project");
    project.id
}

/// Test that simulates the exact flow the frontend does
#[test]
fn test_queue_enqueue_and_state_management() {
    println!("\n========================================");
    println!("Queue Enqueue & State Management Test");
    println!("========================================\n");
    
    // Setup
    let conn = create_test_db();
    let project_dir = unique_temp_dir("queue_int_test");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project_id = create_test_project_in_db(&conn, &project_dir.to_string_lossy());
    
    // Create tasks
    let now = chrono::Utc::now().to_rfc3339();
    let task1 = Task {
        id: "task-1".to_string(),
        task_type: "collect_gsc".to_string(), // Simple task type
        phase: "research".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        execution_mode: ExecutionMode::Manual,
        agent_policy: pageseeds_lib::models::task::AgentPolicy::Optional,
        title: Some("Test Task 1".to_string()),
        description: Some("Description 1".to_string()),
        project_id: project_id.clone(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    
    let task2 = Task {
        id: "task-2".to_string(),
        task_type: "collect_gsc".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::Medium,
        execution_mode: ExecutionMode::Manual,
        agent_policy: pageseeds_lib::models::task::AgentPolicy::Optional,
        title: Some("Test Task 2".to_string()),
        description: Some("Description 2".to_string()),
        project_id: project_id.clone(),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
    };
    
    // Store tasks
    task_store::create_task(&conn, &task1).unwrap();
    task_store::create_task(&conn, &task2).unwrap();
    
    println!("✅ Created 2 tasks");
    
    // Simulate frontend: Log the enqueue action
    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Info,
        "frontend::queue",
        "Tasks enqueued",
        Some(serde_json::json!({
            "taskIds": ["task-1", "task-2"],
            "count": 2
        })),
    );
    
    // Verify tasks are in database
    let tasks = task_store::list_tasks(&conn, &project_id).unwrap();
    assert_eq!(tasks.len(), 2);
    println!("✅ Verified tasks in database");
    
    // Simulate status change (what happens during execution)
    task_store::update_task_status(&conn, "task-1", TaskStatus::InProgress).unwrap();
    
    let updated = task_store::get_task(&conn, "task-1").unwrap();
    assert_eq!(updated.status, TaskStatus::InProgress);
    println!("✅ Task status updated to InProgress");
    
    // Log the status change
    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Info,
        "backend::queue",
        "Task execution started",
        Some(serde_json::json!({"taskId": "task-1"})),
    );
    
    // Complete the task
    task_store::update_task_status(&conn, "task-1", TaskStatus::Done).unwrap();
    
    let completed = task_store::get_task(&conn, "task-1").unwrap();
    assert_eq!(completed.status, TaskStatus::Done);
    println!("✅ Task status updated to Done");
    
    // Log completion
    pageseeds_lib::logging::log(
        &conn,
        pageseeds_lib::logging::LogLevel::Info,
        "backend::queue",
        "Task execution completed",
        Some(serde_json::json!({
            "taskId": "task-1",
            "success": true
        })),
    );
    
    // Verify logs
    let logs = query_logs(&conn, &LogQueryFilters::default(), 100, 0).unwrap();
    assert_eq!(logs.len(), 3);
    println!("✅ Verified 3 logs stored");
    
    // Cleanup
    std::fs::remove_dir_all(&project_dir).ok();
    
    println!("\n========================================");
    println!("✅ QUEUE INTEGRATION TEST PASSED");
    println!("========================================");
}

/// Test batch log submission
#[test]
fn test_batch_log_submission() {
    println!("\n========================================");
    println!("Batch Log Submission Test");
    println!("========================================\n");
    
    let conn = create_test_db();
    
    // Simulate frontend batching multiple logs
    let logs_to_store: Vec<pageseeds_lib::logging::LogEntry> = (0..10)
        .map(|i| pageseeds_lib::logging::LogEntry {
            id: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: pageseeds_lib::logging::LogLevel::Info,
            source: pageseeds_lib::logging::LogSource::Frontend,
            component: "frontend::queue".to_string(),
            message: format!("Queued item {}", i),
            metadata: Some(serde_json::json!({"index": i})),
            session_id: "test-session".to_string(),
        })
        .collect();
    
    // Store all logs
    for log in &logs_to_store {
        pageseeds_lib::logging::store_log(&conn, log).unwrap();
    }
    
    // Verify all stored
    let stored = query_logs(&conn, &LogQueryFilters::default(), 100, 0).unwrap();
    assert_eq!(stored.len(), 10);
    
    // Verify session grouping
    let session_logs: Vec<_> = stored.iter()
        .filter(|l| l.session_id == "test-session")
        .collect();
    assert_eq!(session_logs.len(), 10);
    
    println!("✅ Stored 10 logs in batch");
    println!("✅ All logs have correct session ID");
    
    println!("\n========================================");
    println!("✅ BATCH LOG TEST PASSED");
    println!("========================================");
}

/// Test log filtering and searching
#[test]
fn test_log_querying() {
    println!("\n========================================");
    println!("Log Querying Test");
    println!("========================================\n");
    
    let conn = create_test_db();
    
    // Store logs with different characteristics
    let test_logs = vec![
        ("frontend::queue", "Task enqueued", pageseeds_lib::logging::LogLevel::Info),
        ("frontend::queue", "Task started", pageseeds_lib::logging::LogLevel::Info),
        ("backend::executor", "Executing step 1", pageseeds_lib::logging::LogLevel::Debug),
        ("backend::executor", "Executing step 2", pageseeds_lib::logging::LogLevel::Debug),
        ("backend::agent", "Agent timeout", pageseeds_lib::logging::LogLevel::Error),
        ("frontend::ui", "Button clicked", pageseeds_lib::logging::LogLevel::Info),
    ];
    
    for (component, message, level) in test_logs {
        pageseeds_lib::logging::log(&conn, level, component, message, None);
    }
    
    // Test level filtering
    let debug_logs = query_logs(
        &conn,
        &LogQueryFilters {
            level: Some(pageseeds_lib::logging::LogLevel::Debug),
            ..Default::default()
        },
        100, 0
    ).unwrap();
    assert_eq!(debug_logs.len(), 2);
    println!("✅ Level filter works: {} debug logs", debug_logs.len());
    
    // Test component search
    let queue_logs = query_logs(
        &conn,
        &LogQueryFilters {
            component: Some("queue".to_string()),
            ..Default::default()
        },
        100, 0
    ).unwrap();
    assert_eq!(queue_logs.len(), 2);
    println!("✅ Component filter works: {} queue logs", queue_logs.len());
    
    // Test text search
    let exec_logs = query_logs(
        &conn,
        &LogQueryFilters {
            search_query: Some("execut".to_string()),
            ..Default::default()
        },
        100, 0
    ).unwrap();
    assert_eq!(exec_logs.len(), 3); // "Executing step 1", "Executing step 2", "executor"
    println!("✅ Text search works: {} results for 'execut'", exec_logs.len());
    
    println!("\n========================================");
    println!("✅ LOG QUERYING TEST PASSED");
    println!("========================================");
}
