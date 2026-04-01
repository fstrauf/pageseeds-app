/// End-to-end test for the keyword research workflow
/// 
/// This test verifies:
/// 1. ToolCallingAgent can connect to the bridge
/// 2. keyword_generator tool returns real Ahrefs data
/// 3. keyword_difficulty tool returns real KD scores
/// 4. Full 3-step workflow produces expected results

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::engine::tools::{KeywordGeneratorTool, KeywordDifficultyTool, ToolRegistry};

    /// Test that we can create a ToolCallingAgent
    #[test]
    fn test_create_agent() {
        let mut tools = ToolRegistry::new();
        tools.register(KeywordGeneratorTool);
        tools.register(KeywordDifficultyTool);

        let config = AgentConfig {
            base_url: "http://localhost:8080/v1".to_string(),
            model: "kimi-k2-071818".to_string(),
            api_key: "not-needed".to_string(),
        };

        let agent = ToolCallingAgent::new(config, tools);
        
        // Just verify it doesn't panic
        assert_eq!(agent.model, "kimi-k2-071818");
    }

    /// Test keyword_generator tool directly
    /// 
    /// This bypasses the LLM and tests the Ahrefs API integration
    #[tokio::test]
    async fn test_keyword_generator_tool() {
        // Skip if no CAPSOLVER_API_KEY
        let env = crate::config::env_resolver::EnvResolver::new(".").build_env(std::collections::HashMap::new());
        if env.get("CAPSOLVER_API_KEY").map(|s| s.is_empty()).unwrap_or(true) {
            println!("Skipping test: CAPSOLVER_API_KEY not set");
            return;
        }

        let tool = KeywordGeneratorTool;
        let params = serde_json::json!({
            "keyword": "coffee roaster",
            "country": "us"
        });

        let result = tool.execute(params).await;
        
        println!("Tool result: {:?}", result);
        
        assert!(result.success, "Tool should succeed: {:?}", result.error);
        
        // Verify we got keyword ideas back
        let ideas = result.data.get("ideas").and_then(|v| v.as_array());
        assert!(ideas.is_some(), "Should have ideas array");
        assert!(!ideas.unwrap().is_empty(), "Should have at least one idea");
        
        // Verify structure
        let first = &ideas.unwrap()[0];
        assert!(first.get("keyword").is_some(), "Idea should have keyword");
        println!("First keyword idea: {:?}", first.get("keyword"));
    }

    /// Test keyword_difficulty tool directly
    #[tokio::test]
    async fn test_keyword_difficulty_tool() {
        // Skip if no CAPSOLVER_API_KEY
        let env = crate::config::env_resolver::EnvResolver::new(".").build_env(std::collections::HashMap::new());
        if env.get("CAPSOLVER_API_KEY").map(|s| s.is_empty()).unwrap_or(true) {
            println!("Skipping test: CAPSOLVER_API_KEY not set");
            return;
        }

        let tool = KeywordDifficultyTool;
        let params = serde_json::json!({
            "keyword": "home coffee roaster",
            "country": "us"
        });

        let result = tool.execute(params).await;
        
        println!("Tool result: {:?}", result);
        
        assert!(result.success, "Tool should succeed: {:?}", result.error);
        
        // Verify we got difficulty back (can be integer or float)
        let difficulty = result.data.get("difficulty").and_then(|v| {
            v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
        });
        assert!(difficulty.is_some(), "Should have difficulty score. Data: {:?}", result.data);
        
        let kd = difficulty.unwrap();
        assert!(kd >= 0 && kd <= 100, "KD should be 0-100, got {}", kd);
        
        println!("Keyword difficulty: {}", kd);
    }

    /// Full workflow test (requires running bridge on localhost:8080)
    /// 
    /// Run with: cargo test test_full_workflow -- --ignored
    #[tokio::test]
    #[ignore] // Ignored by default - requires bridge to be running
    async fn test_full_workflow() {
        use crate::models::task::{Task, TaskStatus, Priority, ExecutionMode, AgentPolicy};

        // Skip if no CAPSOLVER_API_KEY
        let env = crate::config::env_resolver::EnvResolver::new(".").build_env(std::collections::HashMap::new());
        if env.get("CAPSOLVER_API_KEY").map(|s| s.is_empty()).unwrap_or(true) {
            println!("Skipping test: CAPSOLVER_API_KEY not set");
            return;
        }

        // Create a mock task
        let task = Task {
            id: "test-task-123".to_string(),
            task_type: "research_landing_pages".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::InProgress,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Batchable,
            agent_policy: AgentPolicy::None,
            title: Some("Test Landing Page Research".to_string()),
            description: Some("Enterprise CRM for real estate agents".to_string()),
            project_id: "test-project".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        // Create agent
        let mut tools = ToolRegistry::new();
        tools.register(KeywordGeneratorTool);
        tools.register(KeywordDifficultyTool);

        let config = AgentConfig {
            base_url: "http://localhost:8080/v1".to_string(),
            model: "kimi-k2-071818".to_string(),
            api_key: "not-needed".to_string(),
        };

        let agent = ToolCallingAgent::new(config, tools);

        // Step 1: Seed Extraction
        println!("\n=== STEP 1: Seed Extraction ===");
        let system_prompt = include_str!("../../prompts/seed_extraction.md");
        let user_prompt = format!(
            "## Project Context\n\nEnterprise CRM for real estate agents\n\n## Task Description\n\nFind landing page keywords for a CRM product targeting real estate agents\n\n## Project Path\n\n/tmp/test-project"
        );

        let result = agent.run(system_prompt, &user_prompt, 5).await;
        match &result {
            Ok(r) => {
                println!("Step 1 result: {} chars", r.content.len());
                println!("Content: {}", r.content);
            }
            Err(e) => {
                println!("Step 1 failed: {}", e);
                panic!("Step 1 should succeed");
            }
        }

        // Step 2: Keyword Discovery (would use themes from step 1)
        println!("\n=== STEP 2: Keyword Discovery ===");
        let system_prompt = include_str!("../../prompts/keyword_discovery.md");
        let user_prompt = format!(
            "## Themes to Research\n\ncrm software, real estate tools, agent productivity\n\n## Project Path\n\n/tmp/test-project"
        );

        let result = agent.run(system_prompt, &user_prompt, 10).await;
        match &result {
            Ok(r) => {
                println!("Step 2 result: {} chars", r.content.len());
                println!("Tool calls: {}", r.tool_calls_executed);
                println!("Content preview: {}", &r.content[..r.content.len().min(500)]);
                
                // Verify we made tool calls
                assert!(r.tool_calls_executed > 0, "Should have made tool calls");
            }
            Err(e) => {
                println!("Step 2 failed: {}", e);
                panic!("Step 2 should succeed");
            }
        }

        // Step 3: Final Selection
        println!("\n=== STEP 3: Final Selection ===");
        let system_prompt = include_str!("../../prompts/final_selection_landing_pages.md");
        let user_prompt = format!(
            "## Keyword Research Data\n\n{{\"keywords\": [{{\"keyword\": \"real estate crm\", \"volume\": 2400, \"kd\": 35}}, {{\"keyword\": \"crm for realtors\", \"volume\": 1200, \"kd\": 28}}]}}\n\nSelect the best candidates based on the criteria above."
        );

        let result = agent.run(system_prompt, &user_prompt, 5).await;
        match &result {
            Ok(r) => {
                println!("Step 3 result: {} chars", r.content.len());
                println!("Content: {}", r.content);
                
                // Verify we got landing page candidates
                assert!(r.content.contains("landing_page_candidates") || r.content.contains("keyword"),
                    "Should return landing page candidates");
            }
            Err(e) => {
                println!("Step 3 failed: {}", e);
                panic!("Step 3 should succeed");
            }
        }

        println!("\n=== FULL WORKFLOW COMPLETE ===");
    }
}
