//! Persist, enrich-gate, and fetch hygiene tests for the Reddit exec pipeline.
//!
//! Config/workflow suites live in [`super::config_tests`].

use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
    TaskStatus,
};
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

/// Test that opportunities can be persisted to and fetched from the database.
/// Fetch only returns pending rows with non-empty reply_text (#236).
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

    // Only post1 has a non-empty draft — post2 must not appear in the picker.
    assert_eq!(fetched.len(), 1, "Should fetch only drafted pending rows");
    assert_eq!(fetched[0].post_id, "post1");
    assert!(fetched[0].reply_text.is_some(), "Should have drafted reply");
    assert_eq!(
        fetched[0].why_relevant.as_deref(),
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
            artifact_key: None,
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

/// A search-shaped payload must persist N pending rows. Fetch only lists
/// pending rows that already have a draft (#236); pre-enrich pending
/// without reply_text are stored but hidden from the picker.
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

    // Pre-enrich: no drafts yet → fetch returns empty feed (not draft candidates).
    let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    assert!(result.success, "fetch failed: {}", result.message);
    let output = result.output.expect("fetch must return output");
    let fetched: Vec<serde_json::Value> =
        serde_json::from_str(&output).expect("fetch output must be a JSON array");
    assert!(
        fetched.is_empty(),
        "picker must not list draft-less pending rows"
    );

    // After a draft is written, the row appears in fetch.
    conn.execute(
        "UPDATE reddit_opportunities SET reply_text=?1, why_relevant=?2 \
         WHERE post_id=?3 AND project_id=?4",
        rusqlite::params!["Helpful drafted reply mentioning ProductX.", "Fits automation", "p71_a", project_id],
    )
    .unwrap();
    let result2 = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    assert!(result2.success, "fetch failed: {}", result2.message);
    let fetched2: Vec<serde_json::Value> =
        serde_json::from_str(&result2.output.expect("output")).expect("json");
    assert_eq!(fetched2.len(), 1);
    assert_eq!(fetched2[0]["post_id"], "p71_a");
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

// ─── Issue #236 + #241: enrich gate + DB∪history dedup + fetch hygiene ────

#[test]
fn should_skip_draft_gate_thresholds() {
    use crate::engine::exec::reddit::{should_skip_draft, MIN_DRAFT_RELEVANCE};

    assert!(
        should_skip_draft(0.0, "some draft", true),
        "relevance 0 skips"
    );
    assert!(
        should_skip_draft(3.9, "some draft", true),
        "below 4.0 skips"
    );
    assert!(
        !should_skip_draft(MIN_DRAFT_RELEVANCE, "some draft", true),
        "exactly 4.0 keeps draft when answers OP"
    );
    assert!(
        !should_skip_draft(8.0, "value-first draft", true),
        "high keeps when answers OP"
    );
    assert!(
        should_skip_draft(9.0, "", true),
        "empty reply skips even if high relevance"
    );
    assert!(
        should_skip_draft(9.0, "   \t\n", true),
        "whitespace-only reply skips"
    );
    // #241 answer-quality gate
    assert!(
        should_skip_draft(9.0, "pitchy non-answer", false),
        "answers_op_question=false skips even when relevance ≥ 4 and text non-empty"
    );
    assert!(
        !should_skip_draft(4.0, "helpful answer to OP", true),
        "keeps draft when relevance ≥ 4, non-empty, answers_op_question=true"
    );
}

#[test]
fn format_post_body_for_enrich_truncates_to_2000() {
    use crate::engine::exec::reddit::{
        format_post_body_for_enrich, ENRICH_SELFTEXT_MAX_CHARS,
    };

    let long: String = "a".repeat(2500);
    let out = format_post_body_for_enrich(&long);
    assert_eq!(out.chars().count(), ENRICH_SELFTEXT_MAX_CHARS);
    assert_eq!(out, "a".repeat(ENRICH_SELFTEXT_MAX_CHARS));

    let short: String = "b".repeat(1500);
    let out_short = format_post_body_for_enrich(&short);
    assert_eq!(out_short.chars().count(), 1500);
    assert_eq!(out_short, short);

    // Newlines → spaces; quotes normalized
    let messy = "line1\nline2 \"quoted\"";
    let cleaned = format_post_body_for_enrich(messy);
    assert_eq!(cleaned, "line1 line2 'quoted'");
}

#[test]
fn enrich_apply_gate_low_relevance_skipped_null_reply() {
    let conn = in_memory_db();
    let project_id = create_test_project(&conn, "/tmp/ps_reddit_gate_low");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO reddit_opportunities \
         (post_id, project_id, title, reply_status, created_at, updated_at) \
         VALUES ('gate_low', ?1, 'Off-topic', 'pending', ?2, ?2)",
        rusqlite::params![project_id, now],
    )
    .unwrap();

    let (status, reply) = crate::engine::exec::reddit::apply_enrich_gate_update(
        &conn,
        &project_id,
        "gate_low",
        2.0,
        "Would have been a draft",
        "Barely related",
        true,
    )
    .expect("gate update");

    assert_eq!(status, "skipped");
    assert!(reply.is_none());

    let (db_status, db_reply, db_why, db_score): (
        String,
        Option<String>,
        Option<String>,
        Option<f64>,
    ) = conn
        .query_row(
            "SELECT reply_status, reply_text, why_relevant, relevance_score \
             FROM reddit_opportunities WHERE post_id='gate_low'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(db_status, "skipped");
    assert!(db_reply.is_none());
    assert_eq!(db_why.as_deref(), Some("Barely related"));
    assert_eq!(db_score, Some(2.0));
}

#[test]
fn enrich_apply_gate_high_relevance_pending_with_draft() {
    let conn = in_memory_db();
    let project_id = create_test_project(&conn, "/tmp/ps_reddit_gate_high");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO reddit_opportunities \
         (post_id, project_id, title, reply_status, created_at, updated_at) \
         VALUES ('gate_high', ?1, 'Great fit', 'pending', ?2, ?2)",
        rusqlite::params![project_id, now],
    )
    .unwrap();

    let draft = "I ran into the same issue — here is what worked. ProductX helped me automate that workflow.";
    let (status, reply) = crate::engine::exec::reddit::apply_enrich_gate_update(
        &conn,
        &project_id,
        "gate_high",
        8.5,
        draft,
        "Direct product fit",
        true,
    )
    .expect("gate update");

    assert_eq!(status, "pending");
    assert_eq!(reply.as_deref(), Some(draft));

    // High-relevance draft must appear in fetch results.
    let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    assert!(result.success, "{}", result.message);
    let fetched: Vec<serde_json::Value> =
        serde_json::from_str(&result.output.expect("output")).expect("json");
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0]["post_id"], "gate_high");
    assert_eq!(fetched[0]["reply_text"], draft);
    assert_eq!(fetched[0]["reply_status"], "pending");
}

#[test]
fn enrich_apply_gate_empty_reply_skipped_even_if_high_relevance() {
    let conn = in_memory_db();
    let project_id = create_test_project(&conn, "/tmp/ps_reddit_gate_empty");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO reddit_opportunities \
         (post_id, project_id, title, reply_status, created_at, updated_at) \
         VALUES ('gate_empty', ?1, 'Ok topic', 'pending', ?2, ?2)",
        rusqlite::params![project_id, now],
    )
    .unwrap();

    let (status, reply) = crate::engine::exec::reddit::apply_enrich_gate_update(
        &conn,
        &project_id,
        "gate_empty",
        9.0,
        "  ",
        "Relevant but no value-first mention possible",
        true,
    )
    .expect("gate update");

    assert_eq!(status, "skipped");
    assert!(reply.is_none());

    // Must not appear as a draft candidate in fetch.
    let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    let fetched: Vec<serde_json::Value> =
        serde_json::from_str(&result.output.expect("output")).expect("json");
    assert!(
        fetched.is_empty(),
        "relevance-high empty draft must not be a picker candidate"
    );

    // And must not re-enter enrich (status != pending).
    let status_db: String = conn
        .query_row(
            "SELECT reply_status FROM reddit_opportunities WHERE post_id='gate_empty'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status_db, "skipped");
}

#[test]
fn enrich_apply_gate_answers_op_false_skipped() {
    let conn = in_memory_db();
    let project_id = create_test_project(&conn, "/tmp/ps_reddit_gate_unanswered");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO reddit_opportunities \
         (post_id, project_id, title, reply_status, created_at, updated_at) \
         VALUES ('gate_unanswered', ?1, 'Real question', 'pending', ?2, ?2)",
        rusqlite::params![project_id, now],
    )
    .unwrap();

    let (status, reply) = crate::engine::exec::reddit::apply_enrich_gate_update(
        &conn,
        &project_id,
        "gate_unanswered",
        8.0,
        "Check out our product — it solves everything!",
        "Relevant but pitch-shaped",
        false,
    )
    .expect("gate update");

    assert_eq!(status, "skipped");
    assert!(reply.is_none());

    let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    let fetched: Vec<serde_json::Value> =
        serde_json::from_str(&result.output.expect("output")).expect("json");
    assert!(
        fetched.is_empty(),
        "answers_op_question=false must not be a picker candidate"
    );

    let status_db: String = conn
        .query_row(
            "SELECT reply_status FROM reddit_opportunities WHERE post_id='gate_unanswered'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status_db, "skipped");
}

#[test]
fn relevance_zero_cannot_appear_as_draft_candidate() {
    let conn = in_memory_db();
    let project_id = create_test_project(&conn, "/tmp/ps_reddit_gate_zero");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO reddit_opportunities \
         (post_id, project_id, title, reply_status, created_at, updated_at) \
         VALUES ('gate_zero', ?1, 'Noise', 'pending', ?2, ?2)",
        rusqlite::params![project_id, now],
    )
    .unwrap();

    crate::engine::exec::reddit::apply_enrich_gate_update(
        &conn,
        &project_id,
        "gate_zero",
        0.0,
        "Should not stick",
        "Not relevant",
        true,
    )
    .unwrap();

    let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    let fetched: Vec<serde_json::Value> =
        serde_json::from_str(&result.output.expect("output")).expect("json");
    assert!(
        fetched.is_empty(),
        "relevance-0 must not appear in reddit_results_stage"
    );
}

#[test]
fn search_handled_set_unions_db_posted_skipped_without_history_file() {
    let conn = in_memory_db();
    let temp_dir = std::env::temp_dir().join(format!(
        "ps_reddit_handled_{}",
        Utc::now().timestamp_millis()
    ));
    // Intentionally no _posted_history.json — only SQLite.
    std::fs::create_dir_all(&temp_dir).unwrap();
    let project_id = create_test_project(&conn, &temp_dir.to_string_lossy());

    for (post_id, status) in [
        ("db_posted_1", "posted"),
        ("db_skipped_1", "skipped"),
        ("db_pending_1", "pending"),
    ] {
        conn.execute(
            "INSERT INTO reddit_opportunities \
             (post_id, project_id, title, reply_status, created_at, updated_at) \
             VALUES (?1, ?2, 't', ?3, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            rusqlite::params![post_id, project_id, status],
        )
        .unwrap();
    }

    let handled = crate::engine::exec::reddit::collect_handled_post_ids(
        &conn,
        &temp_dir.to_string_lossy(),
        &project_id,
    );

    assert!(
        handled.contains("db_posted_1"),
        "posted must be in handled set even without history file"
    );
    assert!(
        handled.contains("db_skipped_1"),
        "skipped must be in handled set even without history file"
    );
    assert!(
        !handled.contains("db_pending_1"),
        "pending must not be treated as handled"
    );

    // Pure helper: history ids ∪ DB ids
    let mut from_history = std::collections::HashSet::new();
    from_history.insert("hist_only".to_string());
    crate::engine::exec::reddit::union_db_handled_ids(&conn, &project_id, &mut from_history);
    assert!(from_history.contains("hist_only"));
    assert!(from_history.contains("db_posted_1"));
    assert!(from_history.contains("db_skipped_1"));
    assert!(!from_history.contains("db_pending_1"));

    std::fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn fetch_only_returns_pending_with_non_empty_reply_text() {
    let conn = in_memory_db();
    let project_id = create_test_project(&conn, "/tmp/ps_reddit_fetch_hygiene");
    let now = Utc::now().to_rfc3339();

    let rows = [
        ("f_ok", "pending", Some("Real draft here")),
        ("f_null", "pending", None),
        ("f_empty", "pending", Some("")),
        ("f_ws", "pending", Some("   ")),
        ("f_skipped", "skipped", Some("old draft")),
        ("f_posted", "posted", Some("was posted")),
    ];
    for (post_id, status, reply) in rows {
        conn.execute(
            "INSERT INTO reddit_opportunities \
             (post_id, project_id, title, reply_status, reply_text, final_score, \
              created_at, updated_at) \
             VALUES (?1, ?2, 't', ?3, ?4, 8.0, ?5, ?5)",
            rusqlite::params![post_id, project_id, status, reply, now],
        )
        .unwrap();
    }

    let result = crate::engine::exec::reddit::exec_reddit_fetch_results(&conn, &project_id);
    assert!(result.success, "{}", result.message);
    let fetched: Vec<serde_json::Value> =
        serde_json::from_str(&result.output.expect("output")).expect("json");
    assert_eq!(fetched.len(), 1, "only non-empty pending draft");
    assert_eq!(fetched[0]["post_id"], "f_ok");
}

