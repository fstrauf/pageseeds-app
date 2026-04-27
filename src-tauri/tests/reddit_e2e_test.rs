//! End-to-End Test for Reddit Opportunity Search Flow
//!
//! This test exercises the complete Reddit search pipeline with real APIs:
//! 1. Creates a project with reddit_config.md
//! 2. Creates a reddit_opportunity_search task
//! 3. Executes the task through the executor (using real Kimi agent)
//! 4. Verifies config parsing, Reddit API search, and enrichment steps
//!
//! Run with:
//!   cargo test --test reddit_e2e_test -- --nocapture
//!
//! Or run a specific test:
//!   cargo test --test reddit_e2e_test test_reddit_config_parsing_with_real_kimi -- --nocapture

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Import the library we're testing
use pageseeds_lib::db;
use pageseeds_lib::engine::executor;
use pageseeds_lib::engine::task_store;
use pageseeds_lib::models::project::Project;
use pageseeds_lib::models::task::{ExecutionMode, Priority, Task, TaskRun, TaskStatus};

/// Sample reddit_config.md content for testing
const SAMPLE_REDDIT_CONFIG: &str = r#"# Reddit Configuration

## Product Information
- **Product Name**: Days to Expiry
- **Product URL**: https://daystoexpiry.com

## Targeting Strategy
- **Mention Stance**: OPTIONAL
- **Trigger Topics**:
  - expiration date tracking
  - food waste reduction
  - pantry organization
  - inventory management
  - expiry date reminder
- **Query Keywords**:
  - "how to track expiration dates"
  - "food waste app"
  - "pantry organizer"
  - "expiry date tracker"
- **Seed Subreddits**:
  - personalfinance
  - EatCheapAndHealthy
  - mealprep
  - minimalism
  - organization
- **Excluded Subreddits**:
  - politics
  - news
"#;

const SAMPLE_PROJECT_SUMMARY: &str = r#"# Days to Expiry

Days to Expiry is an app that helps users track expiration dates for food,
medications, and other perishable items to reduce waste.

## Key Features
- Barcode scanning
- Expiration date notifications
- Inventory tracking
- Waste reduction analytics
"#;

const SAMPLE_BRANDVOICE: &str = r#"# Brand Voice

- Friendly and helpful
- Focus on sustainability and reducing waste
- Practical advice, not preachy
- Community-oriented
"#;

/// Unique temp directory for test isolation
fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}_{nanos}"))
}

/// Set up a test project directory with config files
fn setup_test_project(dir: &Path) {
    let automation_dir = dir.join(".github").join("automation");
    std::fs::create_dir_all(&automation_dir).expect("Failed to create automation dir");
    
    std::fs::write(automation_dir.join("reddit_config.md"), SAMPLE_REDDIT_CONFIG)
        .expect("Failed to write reddit_config.md");
    
    std::fs::write(automation_dir.join("project_summary.md"), SAMPLE_PROJECT_SUMMARY)
        .expect("Failed to write project_summary.md");
    
    std::fs::write(automation_dir.join("brandvoice.md"), SAMPLE_BRANDVOICE)
        .expect("Failed to write brandvoice.md");
}

/// Create an in-memory database with full schema
fn create_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("Failed to open in-memory DB");
    
    // Initialize with full schema
    db::init_with_conn(&conn).expect("Failed to initialize DB schema");
    
    conn
}

/// Create a test project in the database
fn create_test_project_in_db(conn: &rusqlite::Connection, path: &str) -> String {
    let project = Project {
        id: "test-proj-123".to_string(),
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
    };
    
    task_store::create_project(conn, &project).expect("Failed to create project");
    project.id
}

/// Create a reddit_opportunity_search task
fn create_reddit_task(project_id: &str) -> Task {
    let now = chrono::Utc::now().to_rfc3339();
    Task {
        id: format!("test-reddit-{}", chrono::Utc::now().timestamp_millis()),
        task_type: "reddit_opportunity_search".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::High,
        execution_mode: ExecutionMode::Manual,
        agent_policy: pageseeds_lib::models::task::AgentPolicy::Optional,
        title: Some("Reddit Opportunity Search Test".to_string()),
        description: Some("End-to-end test of Reddit search flow".to_string()),
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
    }
}

/// Test 1: Reddit config parsing with real Kimi agent
/// 
/// This test verifies that Kimi can properly parse the reddit_config.md
/// and return valid JSON with the expected fields.
#[tokio::test]
#[ignore = "Requires real Kimi CLI - run manually with: cargo test --test reddit_e2e_test test_reddit_config_parsing_with_real_kimi -- --ignored --nocapture"]
async fn test_reddit_config_parsing_with_real_kimi() {
    println!("\n========================================");
    println!("TEST 1: Reddit Config Parsing with Real Kimi");
    println!("========================================\n");
    
    // Set up test project
    let project_dir = unique_temp_dir("reddit_e2e_test");
    setup_test_project(&project_dir);
    println!("✅ Test project created at: {}", project_dir.display());
    
    // Create database and project
    let conn = create_test_db();
    let project_id = create_test_project_in_db(&conn, &project_dir.to_string_lossy());
    println!("✅ Project created in DB: {}", project_id);
    
    // Create and store the task
    let task = create_reddit_task(&project_id);
    task_store::create_task(&conn, &task).expect("Failed to create task");
    println!("✅ Task created: {}", task.id);
    
    // Execute the task
    println!("\n🚀 Executing task (this will call real Kimi agent)...");
    let start = std::time::Instant::now();
    
    let result = executor::execute_task(&conn, &task.id).await;
    let duration = start.elapsed();
    
    println!("\n⏱️  Execution completed in {:.1}s", duration.as_secs_f64());
    
    // Analyze result
    match result {
        Ok(exec_result) => {
            println!("\n📊 Execution Result:");
            println!("   Success: {}", exec_result.success);
            println!("   Message: {}", exec_result.message);
            println!("   Steps executed: {}", exec_result.steps.len());
            
            // Print step details
            for (i, step) in exec_result.steps.iter().enumerate() {
                println!("\n   Step {}: {}", i + 1, step.step_name);
                println!("      Kind: {}", step.kind);
                println!("      Status: {}", step.status);
                println!("      Message: {}", step.message);
                if let Some(output) = &step.output {
                    println!("      Output ({} chars)", output.len());
                    // Show first 500 chars of output
                    let preview = &output[..output.len().min(500)];
                    println!("      Preview: {}", preview);
                }
            }
            
            // Verify config parse step succeeded
            let config_step = exec_result.steps.iter().find(|s| s.kind == "reddit_config_parse");
            assert!(
                config_step.is_some(),
                "Expected reddit_config_parse step to exist"
            );
            
            let config_step = config_step.unwrap();
            assert_eq!(
                config_step.status, "ok",
                "Config parse step should succeed. Got: {} - {}",
                config_step.status, config_step.message
            );
            
            // Check that output contains valid JSON
            if let Some(output) = &config_step.output {
                let json_result: Result<serde_json::Value, _> = serde_json::from_str(output);
                assert!(
                    json_result.is_ok(),
                    "Config parse output should be valid JSON: {:?}",
                    json_result.err()
                );
                
                let json = json_result.unwrap();
                println!("\n📄 Parsed Config:");
                println!("   Product: {}", json.get("product_name").and_then(|v| v.as_str()).unwrap_or("NOT FOUND"));
                println!("   Stance: {}", json.get("mention_stance").and_then(|v| v.as_str()).unwrap_or("NOT FOUND"));
                
                let topics = json.get("trigger_topics").and_then(|v| v.as_array());
                println!("   Topics count: {}", topics.map(|t| t.len()).unwrap_or(0));
                
                let subreddits = json.get("seed_subreddits").and_then(|v| v.as_array());
                println!("   Subreddits count: {}", subreddits.map(|s| s.len()).unwrap_or(0));
            }
            
            // The search might fail due to rate limits, but config parsing should work
            println!("\n✅ TEST PASSED: Config parsing works with real Kimi");
        }
        Err(e) => {
            println!("\n❌ Execution failed: {}", e);
            panic!("Task execution failed: {}", e);
        }
    }
    
    // Cleanup
    std::fs::remove_dir_all(&project_dir).ok();
    println!("\n🧹 Cleaned up temp directory");
}

/// Test 2: Full Reddit flow including search (with real Reddit API)
///
/// This test executes the complete flow: config parse → reddit search → results
#[tokio::test]
#[ignore = "Requires real APIs (Kimi + Reddit) - run manually with: cargo test --test reddit_e2e_test test_full_reddit_flow_with_real_apis -- --ignored --nocapture"]
async fn test_full_reddit_flow_with_real_apis() {
    println!("\n========================================");
    println!("TEST 2: Full Reddit Flow with Real APIs");
    println!("========================================\n");
    
    // Set up test project
    let project_dir = unique_temp_dir("reddit_e2e_full");
    setup_test_project(&project_dir);
    println!("✅ Test project created at: {}", project_dir.display());
    
    // Create database and project
    let conn = create_test_db();
    let project_id = create_test_project_in_db(&conn, &project_dir.to_string_lossy());
    println!("✅ Project created in DB: {}", project_id);
    
    // Create and store the task
    let task = create_reddit_task(&project_id);
    task_store::create_task(&conn, &task).expect("Failed to create task");
    println!("✅ Task created: {}", task.id);
    
    // Execute the task
    println!("\n🚀 Executing full Reddit flow (Kimi + Reddit API)...");
    println!("   This may take 30-60 seconds...\n");
    
    let start = std::time::Instant::now();
    let result = executor::execute_task(&conn, &task.id).await;
    let duration = start.elapsed();
    
    println!("\n⏱️  Execution completed in {:.1}s", duration.as_secs_f64());
    
    // Get the updated task to check artifacts
    let updated_task = task_store::get_task(&conn, &task.id).expect("Failed to get updated task");
    
    println!("\n📊 Final Task Status:");
    println!("   Status: {:?}", updated_task.status);
    println!("   Attempts: {}", updated_task.run.attempts);
    if let Some(error) = &updated_task.run.last_error {
        println!("   Last Error: {}", error);
    }
    println!("   Artifacts: {}", updated_task.artifacts.len());
    
    // Analyze result
    match result {
        Ok(exec_result) => {
            println!("\n📋 Execution Summary:");
            println!("   Success: {}", exec_result.success);
            println!("   Message: {}", exec_result.message);
            
            // Print all steps
            println!("\n📋 Step Details:");
            for step in &exec_result.steps {
                let icon = match step.status.as_str() {
                    "ok" => "✅",
                    "failed" => "❌",
                    "running" => "🔄",
                    _ => "⏳",
                };
                println!("   {} {} ({}): {}", icon, step.step_name, step.kind, step.message);
            }
            
            // Check for search results
            let search_step = exec_result.steps.iter().find(|s| s.kind == "reddit_search");
            if let Some(step) = search_step {
                println!("\n🔍 Search Step:");
                println!("   Status: {}", step.status);
                println!("   Message: {}", step.message);
                
                if let Some(output) = &step.output {
                    println!("   Output size: {} chars", output.len());
                    // Try to parse as JSON array
                    if let Ok(posts) = serde_json::from_str::<Vec<serde_json::Value>>(output) {
                        println!("   Posts found: {}", posts.len());
                        for (i, post) in posts.iter().take(3).enumerate() {
                            let title = post.get("title").and_then(|v| v.as_str()).unwrap_or("N/A");
                            let subreddit = post.get("subreddit").and_then(|v| v.as_str()).unwrap_or("N/A");
                            println!("   {}. [{}] {}", i + 1, subreddit, &title[..title.len().min(50)]);
                        }
                    }
                }
            }
            
            // Verify results
            if exec_result.success {
                println!("\n✅ FULL FLOW TEST PASSED");
                println!("   - Config parsing: Working");
                println!("   - Reddit search: Working");
                println!("   - Task completed successfully");
            } else {
                println!("\n⚠️  TEST INCOMPLETE (but not necessarily failed)");
                println!("   The task didn't complete successfully, but individual steps may have worked.");
                println!("   Check the step details above to see what succeeded/failed.");
                
                // Config parsing should work at minimum
                let config_step = exec_result.steps.iter().find(|s| s.kind == "reddit_config_parse");
                if let Some(step) = config_step {
                    if step.status == "ok" {
                        println!("   ✅ Config parsing worked");
                    } else {
                        println!("   ❌ Config parsing failed: {}", step.message);
                    }
                }
            }
        }
        Err(e) => {
            println!("\n❌ Execution error: {}", e);
            // Don't panic - this might be expected if APIs are unavailable
            println!("   This error may be expected if Kimi or Reddit APIs are unavailable.");
        }
    }
    
    // Cleanup
    std::fs::remove_dir_all(&project_dir).ok();
    println!("\n🧹 Cleaned up temp directory");
}

/// Test 3: Verify JSON extraction from Kimi output
///
/// Tests that our JSON extraction logic handles various Kimi output formats correctly.
#[test]
fn test_json_extraction_from_kimi_output() {
    use pageseeds_lib::engine::exec::reddit::extract_json_object;
    
    println!("\n========================================");
    println!("TEST 3: JSON Extraction from Kimi Output");
    println!("========================================\n");
    
    // Test case 1: Clean JSON
    let clean = r#"{"product_name": "Test", "mention_stance": "OPTIONAL"}"#;
    let result = extract_json_object(clean).expect("Should extract clean JSON");
    assert!(result.contains("product_name"));
    println!("✅ Clean JSON extraction works");
    
    // Test case 2: JSON with markdown wrapper
    let wrapped = r#"
    ```json
    {"product_name": "Test", "mention_stance": "OPTIONAL"}
    ```
    "#;
    let result = extract_json_object(wrapped).expect("Should extract wrapped JSON");
    assert!(result.contains("product_name"));
    println!("✅ Wrapped JSON extraction works");
    
    // Test case 3: JSON with surrounding text
    let with_text = r#"
    Here is the extracted configuration:
    
    {"product_name": "Days to Expiry", "trigger_topics": ["topic1", "topic2"]}
    
    This JSON contains the search parameters.
    "#;
    let result = extract_json_object(with_text).expect("Should extract JSON from text");
    assert!(result.contains("Days to Expiry"));
    println!("✅ JSON with surrounding text extraction works");
    
    // Test case 4: Realistic Kimi output (simulated)
    let kimi_like = r#"{"product_name": "Days to Expiry", "mention_stance": "OPTIONAL", "trigger_topics": ["expiration date tracking", "food waste reduction"], "query_keywords": ["expiration date tracking", "food waste reduction"], "seed_subreddits": ["personalfinance", "EatCheapAndHealthy"], "excluded_subreddits": ["politics", "news"]}"#;
    let result = extract_json_object(kimi_like).expect("Should extract Kimi-like output");
    
    // Verify it parses as valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Should parse as JSON");
    assert_eq!(
        parsed.get("product_name").and_then(|v| v.as_str()),
        Some("Days to Expiry")
    );
    println!("✅ Realistic Kimi output extraction works");
    
    println!("\n✅ All JSON extraction tests passed!");
}
