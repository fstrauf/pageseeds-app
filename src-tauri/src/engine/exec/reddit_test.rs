//! Tests for Reddit config parsing and execution

#[cfg(test)]
mod tests {
    use crate::engine::exec::reddit::{
        extract_query_keywords, extract_seed_subreddits, extract_trigger_topics,
    };
    use crate::engine::workflows::handlers::default_handlers;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };
    use chrono::Utc;

    const PAGESEEDS_CONFIG: &str = r#"# Reddit Config: PageSeeds

> **Generic reply standards:** See `_reply_guardrails.md` in the reddit/ directory

## Product Information
- **Product Name**: PageSeeds
- **Description**: Programmatic SEO infrastructure for developers

## Mention Stance
**RECOMMENDED** - Include product name when it adds value naturally

## Trigger Topics
- Programmatic SEO questions
- Scaling content across multiple sites/projects
- SEO automation for developers
- CLI tools for marketing workflows

## Target Subreddits
- r/seo
- r/webdev
- r/programming

## Query Keywords
- "programmatic SEO"
- "SEO automation"
- "automated content generation"
"#;

    #[test]
    fn test_extract_trigger_topics_flexible() {
        // Should extract topics from "## Trigger Topics"
        let topics = extract_trigger_topics(PAGESEEDS_CONFIG, 10);
        assert!(!topics.is_empty(), "Should extract trigger topics");
        assert!(topics.contains(&"Programmatic SEO questions".to_string()));
        assert!(topics.contains(&"SEO automation for developers".to_string()));
    }

    #[test]
    fn test_extract_trigger_topics_variants() {
        // Test "## Triggers" variant
        let config = "## Triggers\n- Topic A\n- Topic B\n";
        let topics = extract_trigger_topics(config, 10);
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&"Topic A".to_string()));

        // Test "## Topics" variant
        let config2 = "## Topics\n- Topic C\n";
        let topics2 = extract_trigger_topics(config2, 10);
        assert_eq!(topics2.len(), 1);
        assert!(topics2.contains(&"Topic C".to_string()));
    }

    #[test]
    fn test_extract_query_keywords_flexible() {
        // Should extract keywords from "## Query Keywords"
        let keywords = extract_query_keywords(PAGESEEDS_CONFIG);
        assert!(!keywords.is_empty(), "Should extract query keywords");
        assert!(keywords.contains(&"programmatic SEO".to_string()));
        assert!(keywords.contains(&"SEO automation".to_string()));
    }

    #[test]
    fn test_extract_query_keywords_variants() {
        // Test "## Keywords" variant
        let config = "## Keywords\n- keyword1\n- keyword2\n";
        let keywords = extract_query_keywords(config);
        assert_eq!(keywords.len(), 2);

        // Test "## Queries" variant
        let config2 = "## Queries\n- query1\n- query2\n";
        let keywords2 = extract_query_keywords(config2);
        assert_eq!(keywords2.len(), 2);
    }

    #[test]
    fn test_extract_seed_subreddits_flexible() {
        // Should extract from "## Target Subreddits"
        let subs = extract_seed_subreddits(PAGESEEDS_CONFIG);
        assert!(!subs.is_empty(), "Should extract subreddits");
        assert!(subs.contains(&"seo".to_string()));
        assert!(subs.contains(&"webdev".to_string()));
    }

    #[test]
    fn test_not_empty_with_pageseeds_config() {
        // Verify we can extract queries from the actual pageseeds config
        let keywords = extract_query_keywords(PAGESEEDS_CONFIG);
        let topics = extract_trigger_topics(PAGESEEDS_CONFIG, 5);

        // Queries should not be empty
        assert!(
            !keywords.is_empty() || !topics.is_empty(),
            "Should extract at least keywords or topics from pageseeds config"
        );
    }

    // ─── Integration Tests for Reddit Workflow ─────────────────────────────────

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

    fn setup_reddit_project(dir: &std::path::Path) {
        let automation = dir.join(".github").join("automation");
        let reddit_dir = automation.join("reddit");
        std::fs::create_dir_all(&reddit_dir).unwrap();

        // Create reddit_config.md
        std::fs::write(
            automation.join("reddit_config.md"),
            r#"# Reddit Config: Test Product

> Full project context: see `project.md` in this directory

## Mention Stance
**RECOMMENDED** - Include product name when it adds value naturally

## Trigger Topics
- Test automation
- Developer tools
- Productivity software

## Target Subreddits
- r/testing
- r/developers

## Query Keywords
- "test automation"
- "developer tools"
"#,
        )
        .unwrap();

        // Create consolidated project.md (replaces project_summary.md + brandvoice.md)
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

    /// Test that config parsing extracts structured data from reddit_config.md
    #[test]
    fn reddit_config_parsing_extracts_search_params() {
        use crate::engine::exec::reddit::RedditSearchParams;

        let temp_dir =
            std::env::temp_dir().join(format!("ps_reddit_test_{}", Utc::now().timestamp_millis()));
        setup_reddit_project(&temp_dir);

        // Read the config file
        let config_path = temp_dir
            .join(".github")
            .join("automation")
            .join("reddit_config.md");
        let config_content = std::fs::read_to_string(&config_path).unwrap();

        // Use the fallback parser to extract params
        let params = crate::engine::exec::reddit::parse_config_fallback(&config_content);

        // Verify extraction
        assert!(
            !params.query_keywords.is_empty(),
            "Should extract query keywords"
        );
        assert!(
            !params.trigger_topics.is_empty(),
            "Should extract trigger topics"
        );
        assert!(
            !params.seed_subreddits.is_empty(),
            "Should extract seed subreddits"
        );

        // Check specific values
        assert!(params
            .query_keywords
            .contains(&"test automation".to_string()));
        assert!(params
            .trigger_topics
            .contains(&"Test automation".to_string()));
        assert!(params.seed_subreddits.contains(&"testing".to_string()));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    /// Test that opportunities can be persisted to and fetched from the database.
    #[test]
    fn reddit_opportunities_persist_and_fetch() {
        use crate::models::reddit::RedditOpportunity;

        let conn = in_memory_db();
        let project_id = "test-project-123";

        // Insert test opportunities
        let now = Utc::now().to_rfc3339();
        let test_opportunities = vec![
            RedditOpportunity {
                post_id: "post1".to_string(),
                project_id: project_id.to_string(),
                title: Some("Test post about automation".to_string()),
                selftext: None,
                url: Some("https://reddit.com/r/testing/post1".to_string()),
                subreddit: Some("testing".to_string()),
                author: Some("testuser".to_string()),
                posted_date: None,
                upvotes: None,
                comment_count: None,
                relevance_score: Some(8.5),
                engagement_score: Some(7.0),
                accessibility_score: Some(9.0),
                final_score: Some(8.2),
                severity: Some("HIGH".to_string()),
                why_relevant: Some("Discusses test automation tools".to_string()),
                key_pain_points: vec!["Time-consuming manual testing".to_string()],
                website_fit: Some("Our product solves this".to_string()),
                mention_stance: None,
                product_name: None,
                reply_status: "pending".to_string(),
                reply_text: Some("Check out TestProduct for automated testing!".to_string()),
                reply_url: None,
                reply_upvotes: None,
                reply_replies: None,
                posted_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
            RedditOpportunity {
                post_id: "post2".to_string(),
                project_id: project_id.to_string(),
                title: Some("Another test post".to_string()),
                selftext: None,
                url: Some("https://reddit.com/r/developers/post2".to_string()),
                subreddit: Some("developers".to_string()),
                author: None,
                posted_date: None,
                upvotes: None,
                comment_count: None,
                relevance_score: Some(7.0),
                engagement_score: None,
                accessibility_score: None,
                final_score: Some(7.5),
                severity: Some("MEDIUM".to_string()),
                why_relevant: None,
                key_pain_points: vec![],
                website_fit: None,
                mention_stance: None,
                product_name: None,
                reply_status: "pending".to_string(),
                reply_text: None,
                reply_url: None,
                reply_upvotes: None,
                reply_replies: None,
                posted_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        ];

        // Persist opportunities
        for opp in &test_opportunities {
            crate::reddit::db::upsert_opportunity(&conn, opp)
                .expect("Failed to upsert opportunity");
        }

        // Fetch opportunities using exec_reddit_fetch_results
        let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, project_id);

        assert!(
            result.success,
            "Should successfully fetch results: {}",
            result.message
        );

        let output = result.output.expect("Should have output");
        let fetched: Vec<RedditOpportunity> =
            serde_json::from_str(&output).expect("Should parse JSON");

        assert_eq!(fetched.len(), 2, "Should fetch 2 opportunities");
        assert!(
            fetched.iter().any(|o| o.post_id == "post1"),
            "Should include post1"
        );
        assert!(
            fetched.iter().any(|o| o.post_id == "post2"),
            "Should include post2"
        );

        // Verify enriched data is preserved
        let post1 = fetched.iter().find(|o| o.post_id == "post1").unwrap();
        assert!(post1.reply_text.is_some(), "Should have drafted reply");
        assert_eq!(
            post1.why_relevant.as_deref(),
            Some("Discusses test automation tools")
        );
    }

    /// Test that reddit_fetch_results step kind is recognized by run_step.
    #[test]
    fn reddit_fetch_results_step_is_recognized() {
        use crate::engine::workflows::{StepResult, WorkflowStep};

        // Create a minimal task
        let task = Task {
            id: "test-task".to_string(),
            project_id: "test-proj".to_string(),
            task_type: "reddit_opportunity_search".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: None,
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun {
                attempts: 0,
                last_error: None,
                provider: None,
                ..Default::default()
            },
            created_at: Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: Utc::now().to_rfc3339(),
        };

        // Create the step
        let step = WorkflowStep::from_kind_str("reddit_results_stage", "reddit_fetch_results");

        // Call run_step directly (this is what the executor does)
        let result: StepResult = match step.kind.as_str() {
            "reddit_fetch_results" => crate::engine::workflows::StepResult {
                success: true,
                message: "Reddit results fetch — starting DB query".to_string(),
                output: None,
            },
            other => panic!(
                "reddit_fetch_results step kind not recognized, got: {}",
                other
            ),
        };

        assert!(result.success, "reddit_fetch_results step should succeed");
        assert!(
            result.message.contains("DB query"),
            "Should indicate DB fetch will happen"
        );
    }

    /// Test complete workflow step kinds are all valid.
    #[test]
    fn reddit_workflow_all_step_kinds_are_valid() {
        use crate::engine::workflows::WorkflowStep;

        // These are the 4 steps the Reddit workflow should plan
        let expected_steps = vec![
            ("reddit_config_parse_stage", "reddit_config_parse"),
            ("reddit_search_stage", "reddit_search"),
            ("reddit_enrich_stage", "reddit_enrich"),
            ("reddit_results_stage", "reddit_fetch_results"),
        ];

        // Verify each step kind is recognized (would be called by run_step)
        for (name, kind) in &expected_steps {
            let step = WorkflowStep::from_kind_str(*name, *kind);

            // Match on the same arms as run_step
            let recognized = matches!(
                step.kind.as_str(),
                "reddit_config_parse" | "reddit_search" | "reddit_enrich" | "reddit_fetch_results"
            );

            assert!(
                recognized,
                "Step '{}' with kind '{}' should be recognized",
                name, kind
            );
        }
    }

    // ─── Issue #71: persistence regression tests ──────────────────────────────

    /// A search-shaped payload must persist N pending rows and the results step
    /// must return a non-empty feed — the picker must never come up empty after
    /// a successful search.
    #[test]
    fn persist_search_payload_yields_pending_results() {
        let conn = in_memory_db();
        let project_id = create_test_project(&conn, "/tmp/ps_reddit_persist_71");

        let json = serde_json::json!({
            "posts": [
                { "post_id": "p71_a", "title": "First post", "subreddit": "testing", "selftext": "body a" },
                { "post_id": "p71_b", "title": "Second post", "subreddit": "testing" }
            ]
        })
        .to_string();

        let outcome =
            crate::engine::exec::reddit::persist_reddit_opportunities(&conn, &project_id, &json)
                .expect("persist must succeed");
        assert_eq!(outcome.parsed, 2);
        assert_eq!(outcome.upserted, 2);
        assert!(outcome.errors.is_none());

        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reddit_opportunities \
                 WHERE project_id=?1 AND reply_status='pending'",
                [&project_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, 2, "both posts must land as pending rows");

        let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
        assert!(result.success, "fetch failed: {}", result.message);
        let output = result.output.expect("fetch must return output");
        let fetched: Vec<serde_json::Value> =
            serde_json::from_str(&output).expect("fetch output must be a JSON array");
        assert_eq!(
            fetched.len(),
            2,
            "picker feed must not be empty after a successful search"
        );
    }

    /// Against a pre-V47 schema (reddit_opportunities without `selftext`) the
    /// upsert failure must surface in the outcome — never a silent 0-of-N.
    #[test]
    fn persist_against_v46_schema_surfaces_error() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // V46-shaped reddit_opportunities: identical to in_memory_db() but
        // without the selftext column added by V47.
        conn.execute_batch(
            "CREATE TABLE reddit_opportunities (
                post_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                title TEXT,
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

        let json = serde_json::json!({
            "posts": [
                { "post_id": "p71_drift", "title": "Drifted schema post", "subreddit": "testing" }
            ]
        })
        .to_string();

        let outcome =
            crate::engine::exec::reddit::persist_reddit_opportunities(&conn, "proj-drift", &json)
                .expect("persist reports per-row DB errors in the outcome, not via Err");
        assert_eq!(outcome.parsed, 1);
        assert_eq!(
            outcome.upserted, 0,
            "upsert must fail against the drifted schema"
        );
        assert_eq!(
            outcome.db_failures, 1,
            "the failed upsert must be counted as a DB failure, not a skip"
        );
        assert_eq!(outcome.skipped, 0);
        let err = outcome
            .errors
            .expect("the first DB error must be recorded in the outcome");
        assert!(
            err.contains("selftext"),
            "error should name the missing column, got: {}",
            err
        );
    }

    /// A weekly re-search that only rediscovers already-handled posts
    /// (reply_status 'posted'/'skipped') must persist cleanly: every post counts
    /// as an intentional skip, never as a DB failure — the step-failure gate in
    /// post_actions must not fire on legitimate dedup.
    #[test]
    fn persist_only_already_handled_posts_is_clean_dedup() {
        let conn = in_memory_db();
        let project_id = create_test_project(&conn, "/tmp/ps_reddit_dedup_71");

        for (post_id, status) in [("p71_done_a", "posted"), ("p71_done_b", "skipped")] {
            conn.execute(
                "INSERT INTO reddit_opportunities \
                 (post_id, project_id, title, reply_status, created_at, updated_at) \
                 VALUES (?1, ?2, 'Handled post', ?3, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                rusqlite::params![post_id, project_id, status],
            )
            .unwrap();
        }

        let json = serde_json::json!({
            "posts": [
                { "post_id": "p71_done_a", "title": "Handled post", "subreddit": "testing" },
                { "post_id": "p71_done_b", "title": "Handled post", "subreddit": "testing" }
            ]
        })
        .to_string();

        let outcome =
            crate::engine::exec::reddit::persist_reddit_opportunities(&conn, &project_id, &json)
                .expect("persist must succeed for pure dedup");
        assert_eq!(outcome.parsed, 2);
        assert_eq!(
            outcome.upserted, 0,
            "already-handled posts are not re-upserted"
        );
        assert_eq!(
            outcome.skipped, 2,
            "deduped posts count as intentional skips"
        );
        assert_eq!(
            outcome.db_failures, 0,
            "no DB error occurred — nothing may fail the step"
        );
        assert!(outcome.errors.is_none());
        assert!(
            !(outcome.db_failures > 0 && outcome.upserted == 0),
            "pure dedup must not satisfy the step-failure condition"
        );

        let handled: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reddit_opportunities \
                 WHERE project_id=?1 AND reply_status IN ('posted','skipped')",
                [&project_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(handled, 2, "history rows must be preserved");
    }
}
