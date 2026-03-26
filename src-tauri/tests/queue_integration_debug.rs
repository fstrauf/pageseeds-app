/// Integration test to debug queue execution issues
/// 
/// This test creates a task and tries to execute it through the queue system,
/// printing detailed debug info at each step.

use std::path::Path;
use rusqlite::Connection;

#[test]
fn test_queue_item_struct() {
    println!("\n========================================");
    println!("TEST: QueueItem Struct Serialization");
    println!("========================================\n");
    
    // Test that QueueItem serializes/deserializes correctly
    let item = pageseeds_lib::commands::executor::QueueItem {
        task_id: "test-task-123".to_string(),
        project_id: "test-project".to_string(),
        title: "Test Task".to_string(),
        task_type: "reddit_search".to_string(),
        project_name: Some("Test Project Name".to_string()),
    };
    
    // Serialize to JSON (like what frontend sends)
    let json = serde_json::to_string(&item).expect("Should serialize");
    println!("Serialized QueueItem:");
    println!("{}", json);
    
    // Deserialize back
    let deserialized: pageseeds_lib::commands::executor::QueueItem = 
        serde_json::from_str(&json).expect("Should deserialize");
    
    assert_eq!(item.task_id, deserialized.task_id);
    assert_eq!(item.project_name, deserialized.project_name);
    println!("\n✅ Serialization round-trip works!");
}

#[test]
fn test_event_struct_serialization() {
    println!("\n========================================");
    println!("TEST: Event Struct Serialization");
    println!("========================================\n");
    
    // Test QueueProgressEvent
    let event = pageseeds_lib::commands::executor::QueueProgressEvent {
        event_type: "started".to_string(),
        task_id: "task-123".to_string(),
        project_id: "project-456".to_string(),
        payload: serde_json::json!({
            "index": 0,
            "total": 1,
            "title": "Test Task",
        }),
    };
    
    let json = serde_json::to_string(&event).expect("Should serialize");
    println!("Serialized QueueProgressEvent:");
    println!("{}", json);
    
    // Verify field names are camelCase
    assert!(json.contains("\"eventType\""));
    assert!(json.contains("\"taskId\""));
    assert!(json.contains("\"projectId\""));
    println!("\n✅ Event serialization uses camelCase field names!");
}

#[test]
fn test_task_creation_and_execution() {
    println!("\n========================================");
    println!("TEST: Task Creation and Direct Execution");
    println!("========================================\n");
    
    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    let db_path = Path::new(project_path).join(".github/automation").join("queue_debug_test.db");
    let _ = std::fs::remove_file(&db_path);
    
    // Setup DB
    let conn = Connection::open(&db_path).expect("Failed to open DB");
    pageseeds_lib::db::init_with_conn(&conn).expect("Failed to init DB");
    
    // Create project
    let project_id = "test-queue-debug";
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO projects (id, name, path, site_url, active, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        [project_id, "Test Queue Debug", project_path, "https://example.com", &now],
    ).unwrap();
    println!("✅ Created project: {}", project_id);
    
    // Create a task
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
            "Debug Test Task",
            "Testing task execution",
            &now,
        ],
    ).unwrap();
    println!("✅ Created task: {}", task_id);
    
    // Try to execute the task directly (not through queue)
    println!("\n📋 Executing task directly...");
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let result = rt.block_on(async {
        pageseeds_lib::engine::executor::execute_task(&conn, &task_id).await
    });
    
    match result {
        Ok(exec_result) => {
            println!("✅ Task execution succeeded!");
            println!("   Success: {}", exec_result.success);
            println!("   Message: {}", exec_result.message);
        }
        Err(e) => {
            println!("❌ Task execution failed: {}", e);
        }
    }
    
    // Cleanup
    let _ = std::fs::remove_file(&db_path);
    println!("\n✅ Test complete!");
}

#[test]
fn test_type_matching() {
    println!("\n========================================");
    println!("TEST: TypeScript vs Rust Type Matching");
    println!("========================================\n");
    
    // Simulate what TypeScript sends
    let ts_json = r#"{
        "taskId": "task-123",
        "projectId": "proj-456",
        "title": "Test Task",
        "taskType": "reddit_search",
        "projectName": "My Project",
        "status": "pending"
    }"#;
    
    println!("TypeScript JSON:");
    println!("{}", ts_json);
    
    // Try to deserialize in Rust
    let result: Result<pageseeds_lib::commands::executor::QueueItem, _> = 
        serde_json::from_str(ts_json);
    
    match result {
        Ok(item) => {
            println!("\n✅ Successfully deserialized TypeScript JSON!");
            println!("   task_id: {}", item.task_id);
            println!("   project_name: {:?}", item.project_name);
        }
        Err(e) => {
            println!("\n❌ Failed to deserialize: {}", e);
            panic!("Type mismatch between TypeScript and Rust!");
        }
    }
    
    // Test without optional fields
    let ts_json_minimal = r#"{
        "taskId": "task-789",
        "projectId": "proj-abc",
        "title": "Minimal Task",
        "taskType": "test"
    }"#;
    
    let result2: Result<pageseeds_lib::commands::executor::QueueItem, _> = 
        serde_json::from_str(ts_json_minimal);
    
    match result2 {
        Ok(item) => {
            println!("\n✅ Minimal JSON also works!");
            println!("   project_name: {:?}", item.project_name);
        }
        Err(e) => {
            println!("\n❌ Failed to deserialize minimal: {}", e);
            panic!("Type mismatch!");
        }
    }
}
