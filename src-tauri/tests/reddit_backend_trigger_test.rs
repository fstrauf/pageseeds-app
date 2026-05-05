use rusqlite::Connection;
/// Backend-only test to trigger Reddit search without UI
///
/// This test directly invokes the executor (not through Tauri commands)
/// to diagnose why the app behaves differently from tests.
use std::path::Path;

/// Directly execute a Reddit search task (no Tauri, no frontend)
#[test]
#[ignore = "Requires real Kimi API and Reddit API"]
fn test_reddit_search_direct_execution() {
    println!("\n========================================");
    println!("TEST: Reddit Search - Direct Backend Execution");
    println!("========================================\n");

    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    let db_path = Path::new(project_path)
        .join(".github/automation")
        .join("pageseeds_test.db");

    // Clean up any existing test DB
    let _ = std::fs::remove_file(&db_path);

    println!("1. Creating test database...");
    let conn = Connection::open(&db_path).expect("Failed to open DB");

    // Initialize schema
    pageseeds_lib::db::init_with_conn(&conn).expect("Failed to init DB");
    println!("   ✅ Database initialized at: {:?}", db_path);

    println!("\n2. Creating test project...");
    let project_id = "test-reddit-backend";
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO projects (id, name, path, site_url, active, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        [
            project_id,
            "Test Reddit Backend",
            project_path,
            "https://example.com",
            &now,
        ],
    ).expect("Failed to create project");
    println!("   ✅ Project created: {}", project_id);

    println!("\n3. Creating Reddit search task...");
    let task_id = format!("task-{}-reddit", chrono::Utc::now().timestamp_millis());

    conn.execute(
        "INSERT INTO tasks (id, type, phase, status, priority, execution_mode, agent_policy, project_id, title, description, depends_on, artifacts, created_at, updated_at) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '[]', '[]', ?11, ?11)",
        [
            &task_id,
            "reddit_opportunity_search",
            "research",
            "todo",
            "medium",
            "manual",
            "optional",
            project_id,
            "Reddit Search - Backend Test",
            "Direct backend execution test",
            &now,
        ],
    ).expect("Failed to create task");
    println!("   ✅ Task created: {}", task_id);

    println!("\n4. Checking environment...");
    println!("   PATH: {}", std::env::var("PATH").unwrap_or_default());
    println!("   HOME: {}", std::env::var("HOME").unwrap_or_default());
    println!(
        "   Working dir: {:?}",
        std::env::current_dir().unwrap_or_default()
    );

    // Check for any KIMI env vars
    for (key, value) in std::env::vars() {
        if key.starts_with("KIMI") {
            println!("   {}: {}", key, value);
        }
    }

    println!("\n5. Executing task directly...");
    println!("   (This will call Kimi and Reddit APIs)\n");

    let start = std::time::Instant::now();

    // Execute the task directly (not through Tauri command)
    // Need a runtime since execute_task is async
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let result =
        rt.block_on(async { pageseeds_lib::engine::executor::execute_task(&conn, &task_id).await });

    let elapsed = start.elapsed();

    println!("\n========================================");
    println!("EXECUTION RESULT");
    println!("========================================");
    println!("Duration: {:.1}s", elapsed.as_secs_f64());

    match result {
        Ok(exec_result) => {
            println!("Success: {}", exec_result.success);
            println!("Message: {}", exec_result.message);
            println!("Steps executed: {}", exec_result.steps.len());

            for (i, step) in exec_result.steps.iter().enumerate() {
                println!("\n  Step {}: {}", i + 1, step.step_name);
                println!("    Status: {}", step.status);
                println!("    Message: {}", step.message);
                if let Some(ref output) = step.output {
                    let preview = if output.len() > 200 {
                        format!("{}... ({} chars)", &output[..200], output.len())
                    } else {
                        output.clone()
                    };
                    println!("    Output: {}", preview);
                }
            }

            if !exec_result.follow_up_tasks.is_empty() {
                println!(
                    "\n  Follow-up tasks created: {}",
                    exec_result.follow_up_tasks.len()
                );
                for task in &exec_result.follow_up_tasks {
                    println!("    - {} ({})", task.title, task.task_type);
                }
            }

            // Verify task status in DB
            let final_status: String = conn
                .query_row("SELECT status FROM tasks WHERE id = ?1", [&task_id], |r| {
                    r.get(0)
                })
                .expect("Failed to get final status");

            println!("\n  Final task status in DB: {}", final_status);

            if exec_result.success {
                println!("\n✅ TASK COMPLETED SUCCESSFULLY");
            } else {
                println!("\n❌ TASK FAILED");
            }
        }
        Err(e) => {
            println!("❌ EXECUTION ERROR: {}", e);
            panic!("Task execution failed: {}", e);
        }
    }

    // Check for any artifacts in DB
    println!("\n6. Checking artifacts...");
    let artifacts: String = conn
        .query_row(
            "SELECT artifacts FROM tasks WHERE id = ?1",
            [&task_id],
            |r| r.get(0),
        )
        .unwrap_or_default();

    if artifacts != "[]" {
        println!("   Artifacts: {}", &artifacts[..artifacts.len().min(500)]);
    } else {
        println!("   No artifacts stored");
    }

    // Cleanup
    println!("\n7. Cleanup...");
    let _ = std::fs::remove_file(&db_path);
    println!("   ✅ Test database removed");

    println!("\n========================================");
    println!("TEST COMPLETE");
    println!("========================================");
}

/// Test that specifically checks the reddit_config_parse step
#[test]
#[ignore = "Requires real Kimi API"]
fn test_reddit_config_parse_step_only() {
    println!("\n========================================");
    println!("TEST: Reddit Config Parse - Step Only");
    println!("========================================\n");

    let project_path = "/Users/fstrauf/01_code/call-analyzer";

    println!("Environment:");
    println!("  PATH: {}", std::env::var("PATH").unwrap_or_default());
    println!("  HOME: {}", std::env::var("HOME").unwrap_or_default());
    println!();

    // Call the parse function directly
    use pageseeds_lib::engine::exec::reddit::exec_reddit_config_parse;
    use pageseeds_lib::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    // Create a minimal task for context
    let task = Task {
        id: "test-task".to_string(),
        project_id: "test-project".to_string(),
        task_type: "reddit_opportunity_search".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::InProgress,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some("Test".to_string()),
        description: None,
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun::default(),
        created_at: chrono::Utc::now().to_rfc3339(),
        not_before: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    println!("Calling exec_reddit_config_parse...\n");
    let start = std::time::Instant::now();

    let result = exec_reddit_config_parse(&task, project_path, "kimi");

    let elapsed = start.elapsed();

    println!("\n========================================");
    println!("RESULT (took {:.1}s)", elapsed.as_secs_f64());
    println!("========================================");
    println!("Success: {}", result.success);
    println!("Message: {}", result.message);

    if let Some(ref output) = result.output {
        let preview = if output.len() > 500 {
            format!("{}... ({} chars total)", &output[..500], output.len())
        } else {
            output.clone()
        };
        println!("\nOutput:\n{}", preview);
    }

    assert!(
        result.success,
        "Config parse should succeed: {}",
        result.message
    );

    // Try to parse the output as RedditSearchParams
    if let Some(ref output) = result.output {
        match pageseeds_lib::engine::text::extract_json_string(output).ok_or("No JSON found") {
            Ok(json_str) => {
                println!("\n✅ Extracted JSON ({} chars)", json_str.len());

                match serde_json::from_str::<serde_json::Value>(&json_str) {
                    Ok(parsed) => {
                        println!("✅ Parsed as JSON successfully");
                        println!("   product_name: {:?}", parsed.get("product_name"));
                        println!(
                            "   trigger_topics count: {:?}",
                            parsed
                                .get("trigger_topics")
                                .and_then(|v| v.as_array())
                                .map(|a| a.len())
                        );
                    }
                    Err(e) => {
                        println!("❌ Failed to parse as JSON: {}", e);
                        panic!("JSON parse failed");
                    }
                }
            }
            Err(e) => {
                println!("❌ JSON extraction failed: {}", e);
                panic!("Extraction failed");
            }
        }
    }

    println!("\n✅ CONFIG PARSE TEST PASSED");
}

/// Simulate the exact same call that happens in the Tauri app
#[test]
#[ignore = "Requires real Kimi API"]
fn test_reddit_config_parse_simulate_app_call() {
    println!("\n========================================");
    println!("TEST: Simulate Exact Tauri App Call");
    println!("========================================\n");

    let project_path = "/Users/fstrauf/01_code/call-analyzer";
    let automation_dir = std::path::Path::new(project_path).join(".github/automation");

    // Read the exact same files the app reads
    let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .expect("Failed to read reddit_config.md");
    let project_summary =
        std::fs::read_to_string(automation_dir.join("project_summary.md")).unwrap_or_default();
    let brandvoice =
        std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();

    // Build the exact same prompt the app builds
    let prompt = format!(
        "Extract Reddit search parameters from the config files below. Return ONLY a JSON object.\n\n\
        ## reddit_config.md\n\
        ```markdown\n\
        {reddit_config}\n\
        ```\n\n\
        ## project_summary.md\n\
        ```markdown\n\
        {project_summary}\n\
        ```\n\n\
        ## brandvoice.md\n\
        ```markdown\n\
        {brandvoice}\n\
        ```\n\n\
        ## Required JSON Output\n\
        Return a JSON object with these exact keys:\n\
        - product_name: string\n\
        - mention_stance: string (REQUIRED, RECOMMENDED, OPTIONAL, or OMIT)\n\
        - trigger_topics: array of strings\n\
        - query_keywords: array of strings (use same as trigger_topics)\n\
        - seed_subreddits: array of strings (WITHOUT r/ prefix)\n\
        - excluded_subreddits: array of strings\n\n\
        ## Example\n\
        If the config has Product Name: Days to Expiry, then return:\n\
        {{\"product_name\": \"Days to Expiry\", \"mention_stance\": \"RECOMMENDED\", \"trigger_topics\": [\"topic1\"], \"query_keywords\": [\"topic1\"], \"seed_subreddits\": [\"subreddit1\"], \"excluded_subreddits\": []}}\n\n\
        Do NOT return placeholder text like \"<actual product name>\".\n\
        Return ONLY the JSON object, starting with {{ and ending with }}.",
        reddit_config = reddit_config,
        project_summary = project_summary,
        brandvoice = brandvoice
    );

    println!("Prompt length: {} chars", prompt.len());
    println!("Environment:");
    println!("  PATH: {}", std::env::var("PATH").unwrap_or_default());
    println!("  HOME: {}", std::env::var("HOME").unwrap_or_default());
    println!("\nCalling agent (this is exactly what the Tauri app does)...\n");

    let start = std::time::Instant::now();

    // Call the agent module directly (same as Tauri app)
    let output = pageseeds_lib::engine::agent::run_agent("kimi", &prompt, Path::new(project_path));

    let elapsed = start.elapsed();

    println!("\n========================================");
    println!("AGENT RESPONSE (took {:.1}s)", elapsed.as_secs_f64());
    println!("========================================");

    match output {
        Ok(content) => {
            println!("Output size: {} bytes", content.len());
            println!("\nFirst 500 chars:\n{}", &content[..content.len().min(500)]);

            if content.len() > 500 {
                println!("\n... ({} more chars)", content.len() - 500);
            }

            // Now extract JSON
            println!("\nExtracting JSON...");
            match pageseeds_lib::engine::text::extract_json_string(&content).ok_or("No JSON found")
            {
                Ok(json_str) => {
                    println!("✅ Extracted JSON ({} chars)", json_str.len());
                    println!(
                        "\nExtracted content:\n{}",
                        &json_str[..json_str.len().min(1000)]
                    );

                    // Try to parse it
                    match serde_json::from_str::<serde_json::Value>(&json_str) {
                        Ok(parsed) => {
                            println!("\n✅ Successfully parsed as JSON");
                            println!(
                                "   Keys: {:?}",
                                parsed.as_object().map(|o| o.keys().collect::<Vec<_>>())
                            );
                        }
                        Err(e) => {
                            println!("\n❌ JSON parse error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("❌ Extraction failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ Agent failed: {}", e);
        }
    }

    println!("\n========================================");
    println!("TEST COMPLETE");
    println!("========================================");
}
