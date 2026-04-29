use rusqlite::Connection;
/// Simple test to verify task execution works
use std::path::Path;

#[test]
#[ignore = "Requires local project files"]
fn test_simple_task_execution() {
    println!("\n========================================");
    println!("TEST: Simple Task Execution");
    println!("========================================\n");

    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    let db_path = Path::new(project_path)
        .join(".github/automation")
        .join("simple_test.db");
    let _ = std::fs::remove_file(&db_path);

    // Setup
    let conn = Connection::open(&db_path).expect("Failed to open DB");
    pageseeds_lib::db::init_with_conn(&conn).expect("Failed to init DB");

    let project_id = "test-simple";
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO projects (id, name, path, site_url, active) VALUES (?1, ?2, ?3, ?4, 1)",
        [
            project_id,
            "Test Simple",
            project_path,
            "https://example.com",
        ],
    )
    .unwrap();

    let task_id = format!("task-{}", chrono::Utc::now().timestamp_millis());
    conn.execute(
        "INSERT INTO tasks (id, type, phase, status, priority, execution_mode, agent_policy, project_id, title, description, depends_on, artifacts, created_at, updated_at) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '[]', '[]', ?11, ?11)",
        [
            &task_id,
            "test_task",
            "test",
            "todo",
            "medium",
            "manual",
            "none",
            project_id,
            "Simple Test Task",
            "Testing",
            &now,
        ],
    ).unwrap();

    println!("Created task: {}", task_id);

    // Execute
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let result =
        rt.block_on(async { pageseeds_lib::engine::executor::execute_task(&conn, &task_id).await });

    match result {
        Ok(r) => println!("✅ SUCCESS: {}", r.message),
        Err(e) => println!("❌ FAILED: {}", e),
    }

    let _ = std::fs::remove_file(&db_path);
}
