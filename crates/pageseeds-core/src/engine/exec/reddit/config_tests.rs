//! Config/workflow tests for the Reddit exec pipeline (YAML ProjectConfig).
//!
//! Persist/gate suites live in [`super::persist_tests`].

use crate::engine::workflows::handlers::default_handlers;
use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
    TaskStatus,
};
use crate::reddit::config::MentionStance;
use chrono::Utc;

fn in_memory_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY, name TEXT NOT NULL,
            path TEXT NOT NULL,
            content_dir TEXT,
            site_url TEXT,
            site_id TEXT,
            active INTEGER NOT NULL DEFAULT 1,
            agent_provider TEXT
         );
         CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            priority TEXT NOT NULL DEFAULT 'medium',
            run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
            review_surface TEXT NOT NULL DEFAULT 'none',
            follow_up_policy TEXT NOT NULL DEFAULT 'none',
            agent_policy TEXT NOT NULL DEFAULT 'none',
            title TEXT, description TEXT,
            project_id TEXT NOT NULL,
            depends_on TEXT NOT NULL DEFAULT '[]',
            artifacts TEXT NOT NULL DEFAULT '[]',
            run_attempts INTEGER NOT NULL DEFAULT 0,
            run_last_error TEXT, run_provider TEXT,
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS reddit_opportunities (
            post_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT,
            selftext TEXT,
            url TEXT,
            subreddit TEXT,
            author TEXT,
            posted_date TEXT,
            upvotes INTEGER,
            comment_count INTEGER,
            relevance_score REAL,
            engagement_score REAL,
            accessibility_score REAL,
            final_score REAL,
            severity TEXT,
            why_relevant TEXT,
            key_pain_points TEXT NOT NULL DEFAULT '[]',
            website_fit TEXT,
            mention_stance TEXT,
            product_name TEXT,
            reply_status TEXT NOT NULL DEFAULT 'pending',
            reply_text TEXT,
            reply_url TEXT,
            reply_upvotes INTEGER,
            reply_replies INTEGER,
            posted_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
         );",
    )
    .unwrap();
    conn
}

fn create_test_project(conn: &rusqlite::Connection, path: &str) -> String {
    let id = format!("proj-{}", Utc::now().timestamp_millis());
    conn.execute(
        "INSERT INTO projects (id, name, path, active) VALUES (?1, 'Test', ?2, 1)",
        [&id, path],
    )
    .unwrap();
    id
}

fn create_reddit_search_task(project_id: &str) -> Task {
    Task {
        id: format!("task-{}", Utc::now().timestamp_millis()),
        project_id: project_id.to_string(),
        task_type: "reddit_opportunity_search".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some("Reddit Opportunity Search".to_string()),
        description: Some("Search for Reddit posting opportunities".to_string()),
        depends_on: vec![],
        artifacts: vec![],
        run: TaskRun {
            attempts: 0,
            last_error: None,
            provider: None,
            ..Default::default()
        },
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
        not_before: None,
    }
}

/// YAML-first fixture: project.yaml + project.md prose + guardrails.
/// No reddit_config.md required for happy path (#293).
fn setup_reddit_project(dir: &std::path::Path) {
    let automation = dir.join(".github").join("automation");
    let reddit_dir = automation.join("reddit");
    std::fs::create_dir_all(&reddit_dir).unwrap();

    std::fs::write(
        automation.join("project.yaml"),
        r#"schema_version: 1
product_name: Test Product
search_keywords:
  primary: []
  problem: []
  audience: []
  do_not_expand: []
clusters: []
reddit:
  mention_stance: recommended
  seed_subreddits:
    - testing
    - developers
  excluded_subreddits: []
  trigger_topics:
    - Test automation
    - Developer tools
    - Productivity software
  query_keywords:
    - test automation
    - developer tools
"#,
    )
    .unwrap();

    // Prose for enrich/draft prompts
    std::fs::write(
        automation.join("project.md"),
        r#"# Test Product

## Identity

- **URL:** https://example.com
- **Description:** A test product for Reddit automation testing.

### Key Differentiators
- Fast and reliable automation
- Developer-friendly API

### Search Keywords
- "test automation"
- "developer tools"

## Brand Voice

Helpful, technical, and concise.

## Content Clusters & Status

- [ ] 🎯 Test Automation Basics (PLANNED)
- [ ] 🎯 Developer Productivity (PLANNED)
"#,
    )
    .unwrap();

    // Create _reply_guardrails.md
    std::fs::write(
        reddit_dir.join("_reply_guardrails.md"),
        "# Reply Guardrails\n\nBe helpful and authentic.",
    )
    .unwrap();
}


/// Test that the Reddit workflow plans all 4 steps correctly.
#[test]
fn reddit_workflow_plans_four_steps() {
    let conn = in_memory_db();
    let temp_dir =
        std::env::temp_dir().join(format!("ps_reddit_test_{}", Utc::now().timestamp_millis()));
    setup_reddit_project(&temp_dir);

    let project_id = create_test_project(&conn, &temp_dir.to_string_lossy());
    let task = create_reddit_search_task(&project_id);

    let handlers = default_handlers();
    let handler = handlers
        .iter()
        .find(|h| h.supports(&task))
        .expect("Should find handler");
    let steps = handler.plan(&task);

    // Should have 4 steps: config_parse, search, enrich, results
    assert_eq!(steps.len(), 4, "Reddit workflow should have 4 steps");
    assert_eq!(steps[0].name, "reddit_config_parse_stage");
    assert_eq!(steps[1].name, "reddit_search_stage");
    assert_eq!(steps[2].name, "reddit_enrich_stage");
    assert_eq!(steps[3].name, "reddit_results_stage");

    std::fs::remove_dir_all(&temp_dir).ok();
}

/// Deterministic config parse from project.yaml — no agent/rig (#293).
#[test]
fn reddit_config_parsing_extracts_search_params() {
    let temp_dir =
        std::env::temp_dir().join(format!("ps_reddit_test_{}", Utc::now().timestamp_millis()));
    setup_reddit_project(&temp_dir);

    let task = create_reddit_search_task("proj-yaml");
    let result = crate::engine::exec::reddit::exec_reddit_config_parse(
        &task,
        &temp_dir.to_string_lossy(),
        "unused-provider",
    );

    assert!(
        result.success,
        "config parse should succeed without agent: {}",
        result.message
    );
    let output = result.output.expect("should emit RedditSearchParams JSON");
    let params: crate::engine::exec::reddit::RedditSearchParams =
        serde_json::from_str(&output).expect("output must be RedditSearchParams");

    assert!(
        !params.query_keywords.is_empty(),
        "Should load query keywords from YAML"
    );
    assert!(
        !params.trigger_topics.is_empty(),
        "Should load trigger topics from YAML"
    );
    assert!(
        !params.seed_subreddits.is_empty(),
        "Should load seed subreddits from YAML"
    );

    assert_eq!(params.product_name.as_deref(), Some("Test Product"));
    assert_eq!(params.mention_stance, MentionStance::Recommended);
    assert!(params
        .query_keywords
        .contains(&"test automation".to_string()));
    assert!(params
        .trigger_topics
        .contains(&"Test automation".to_string()));
    assert!(params.seed_subreddits.contains(&"testing".to_string()));
    assert!(params.user_context.is_none());

    std::fs::remove_dir_all(&temp_dir).ok();
}

/// Artifact wire form for mention_stance is UPPERCASE; deserialize accepts both.
#[test]
fn reddit_search_params_stance_serde_uppercase_wire() {
    use crate::engine::exec::reddit::RedditSearchParams;

    let params = RedditSearchParams {
        product_name: Some("X".into()),
        mention_stance: MentionStance::Required,
        trigger_topics: vec!["t".into()],
        query_keywords: vec!["q".into()],
        seed_subreddits: vec!["seo".into()],
        excluded_subreddits: vec![],
        user_context: None,
    };
    let json = serde_json::to_string(&params).expect("serialize");
    assert!(
        json.contains("\"REQUIRED\""),
        "artifact JSON must use UPPERCASE stance, got: {json}"
    );
    assert!(
        !json.contains("\"required\""),
        "must not serialize snake_case stance on artifact wire"
    );

    let from_upper: RedditSearchParams =
        serde_json::from_str(r#"{"product_name":"X","mention_stance":"RECOMMENDED","trigger_topics":[],"query_keywords":[],"seed_subreddits":[],"excluded_subreddits":[]}"#)
            .expect("UPPERCASE deserialize");
    assert_eq!(from_upper.mention_stance, MentionStance::Recommended);

    let from_snake: RedditSearchParams =
        serde_json::from_str(r#"{"product_name":"X","mention_stance":"optional","trigger_topics":[],"query_keywords":[],"seed_subreddits":[],"excluded_subreddits":[]}"#)
            .expect("snake_case deserialize");
    assert_eq!(from_snake.mention_stance, MentionStance::Optional);
}

/// user_context from task description is injected into params.
#[test]
fn reddit_config_parse_injects_user_context() {
    let temp_dir = std::env::temp_dir().join(format!(
        "ps_reddit_uctx_{}",
        Utc::now().timestamp_millis()
    ));
    setup_reddit_project(&temp_dir);

    let mut task = create_reddit_search_task("proj-uctx");
    task.description = Some(r#"{"user_context":"focus on CLI tools"}"#.to_string());

    let result = crate::engine::exec::reddit::exec_reddit_config_parse(
        &task,
        &temp_dir.to_string_lossy(),
        "_",
    );
    assert!(result.success, "{}", result.message);
    let params: crate::engine::exec::reddit::RedditSearchParams =
        serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
    assert_eq!(params.user_context.as_deref(), Some("focus on CLI tools"));

    std::fs::remove_dir_all(&temp_dir).ok();
}

/// Empty keywords+topics fails clearly (operator must fill YAML).
#[test]
fn reddit_config_parse_fails_when_keywords_and_topics_empty() {
    let temp_dir = std::env::temp_dir().join(format!(
        "ps_reddit_empty_{}",
        Utc::now().timestamp_millis()
    ));
    let automation = temp_dir.join(".github").join("automation");
    std::fs::create_dir_all(automation.join("reddit")).unwrap();
    std::fs::write(
        automation.join("project.yaml"),
        r#"schema_version: 1
product_name: EmptyKw
reddit:
  mention_stance: optional
  seed_subreddits: []
  excluded_subreddits: []
  trigger_topics: []
  query_keywords: []
"#,
    )
    .unwrap();
    std::fs::write(automation.join("project.md"), "# EmptyKw\n").unwrap();
    std::fs::write(
        automation.join("reddit").join("_reply_guardrails.md"),
        "guard\n",
    )
    .unwrap();

    let task = create_reddit_search_task("proj-empty");
    let result = crate::engine::exec::reddit::exec_reddit_config_parse(
        &task,
        &temp_dir.to_string_lossy(),
        "_",
    );
    assert!(!result.success);
    assert!(
        result.message.contains("query_keywords") || result.message.contains("trigger_topics"),
        "message should mention empty keywords/topics: {}",
        result.message
    );

    std::fs::remove_dir_all(&temp_dir).ok();
}

/// validate_project_stance REQUIRED with YAML only (no reddit_config.md).
#[test]
fn validate_project_stance_required_yaml_only() {
    use crate::reddit::validation::validate_project_stance;

    let temp_dir = std::env::temp_dir().join(format!(
        "ps_reddit_stance_{}",
        Utc::now().timestamp_millis()
    ));
    let automation = temp_dir.join(".github").join("automation");
    std::fs::create_dir_all(&automation).unwrap();
    std::fs::write(
        automation.join("project.yaml"),
        r#"schema_version: 1
product_name: StanceProduct
reddit:
  mention_stance: required
  seed_subreddits: []
  excluded_subreddits: []
  trigger_topics:
    - something
  query_keywords:
    - something
"#,
    )
    .unwrap();

    let ok = validate_project_stance(
        "This is a long enough reply. StanceProduct helps here. Third sentence ends.",
        &automation,
    );
    assert!(ok.valid, "reply with product name should pass: {:?}", ok.error);

    let bad = validate_project_stance(
        "This is a long enough reply without the brand. Still three sentences here. Yes.",
        &automation,
    );
    assert!(!bad.valid);
    assert!(
        bad.error
            .as_deref()
            .unwrap_or("")
            .contains("StanceProduct"),
        "error should name product: {:?}",
        bad.error
    );

    std::fs::remove_dir_all(&temp_dir).ok();
}

/// YAML-only tree (no reddit_config.md) is not reported missing by draft gates.
#[test]
fn missing_config_files_allows_yaml_without_reddit_config_md() {
    let temp_dir = std::env::temp_dir().join(format!(
        "ps_reddit_missing_{}",
        Utc::now().timestamp_millis()
    ));
    setup_reddit_project(&temp_dir);
    let automation = temp_dir.join(".github").join("automation");
    assert!(
        !automation.join("reddit_config.md").exists(),
        "fixture must not require reddit_config.md"
    );
    let missing = crate::reddit::config::missing_config_files(&automation);
    assert!(
        missing.is_empty(),
        "YAML + prose + guardrails should pass: {:?}",
        missing
    );
    std::fs::remove_dir_all(&temp_dir).ok();
}


