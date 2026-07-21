//! Safety tests for the Reddit reply pipeline (issue #22).
//!
//! Covers:
//! 1. Posting path validation — exec_reddit_post_reply must run validate_reply
//!    (and the stance check) BEFORE any Reddit API call and fail the task on
//!    violations. These tests only exercise failure paths that return before
//!    credential loading, so no network access or credentials are needed.
//! 2. Selftext plumbing — the post body is persisted and read back.
//! 3. Stale-marking — re-running a search marks pending rows 'stale' instead of
//!    deleting them, preserves posted rows, and revives re-discovered posts.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use pageseeds_lib::db;
use pageseeds_lib::engine::exec::reddit::{exec_reddit_post_reply, persist_reddit_opportunities};
use pageseeds_lib::engine::task_store;
use pageseeds_lib::models::project::Project;
use pageseeds_lib::models::reddit::RedditOpportunity;
use pageseeds_lib::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
    TaskStatus,
};
use pageseeds_lib::reddit::db as reddit_db;

// ─── Harness ──────────────────────────────────────────────────────────────────

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}_{nanos}"))
}

fn create_test_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("Failed to open in-memory DB");
    db::init_with_conn(&conn).expect("Failed to initialize DB schema");
    conn
}

fn create_test_project(conn: &rusqlite::Connection, path: &str) -> String {
    let project = Project {
        id: format!("test-proj-{}", chrono::Utc::now().timestamp_millis()),
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

fn make_task(project_id: &str, task_type: &str, description: Option<String>) -> Task {
    let now = chrono::Utc::now().to_rfc3339();
    Task {
        id: format!("test-task-{}", chrono::Utc::now().timestamp_millis()),
        project_id: project_id.to_string(),
        task_type: task_type.to_string(),
        phase: "implementation".to_string(),
        status: TaskStatus::Todo,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some("Test task".to_string()),
        description,
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

fn make_opportunity(project_id: &str, post_id: &str) -> RedditOpportunity {
    let now = chrono::Utc::now().to_rfc3339();
    RedditOpportunity {
        post_id: post_id.to_string(),
        title: Some(format!("Title for {post_id}")),
        selftext: None,
        url: Some(format!("https://reddit.com/r/testing/{post_id}")),
        subreddit: Some("testing".to_string()),
        author: Some("testuser".to_string()),
        posted_date: None,
        upvotes: Some(10),
        comment_count: Some(5),
        relevance_score: Some(7.0),
        engagement_score: Some(6.0),
        accessibility_score: Some(8.0),
        final_score: Some(7.0),
        severity: Some("HIGH".to_string()),
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
        project_id: project_id.to_string(),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn reply_task_description(post_id: &str, draft: &str) -> String {
    format!(
        "**Subreddit:** r/testing\n\n**Post URL:** https://reddit.com/r/testing/{post_id}\n\n**Why Relevant:** test\n\n**Draft Reply:**\n{draft}\n\n**Post ID:** {post_id}"
    )
}

fn setup_stance_required_project(dir: &Path) {
    let automation_dir = dir.join(".github").join("automation");
    std::fs::create_dir_all(&automation_dir).expect("Failed to create automation dir");
    std::fs::write(
        automation_dir.join("reddit_config.md"),
        "## Product Name\n- TestProduct\n\n## Mention Stance\n- REQUIRED\n",
    )
    .expect("Failed to write reddit_config.md");
}

// ─── 1. Posting-path validation ───────────────────────────────────────────────

#[test]
fn post_reply_fails_validation_when_draft_contains_url() {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("reddit_safety_url");
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());

    let draft = "This draft has enough sentences to pass length checks easily. \
                 It also has plenty of words to clear the minimum word count rule. \
                 But it links out to https://spam.example.com which is forbidden.";
    let task = make_task(
        &project_id,
        "reddit_reply",
        Some(reply_task_description("post_url_fail", draft)),
    );

    let result = exec_reddit_post_reply(&task, &project_dir.to_string_lossy(), &conn);

    assert!(
        !result.success,
        "task must fail when the draft contains a URL"
    );
    assert!(
        result.message.contains("Reply validation failed"),
        "failure message must name validation, got: {}",
        result.message
    );
    assert!(
        result.message.contains("URLs"),
        "failure message should mention the URL rule, got: {}",
        result.message
    );

    // The opportunity must NOT have been marked posted.
    let status: String = conn
        .query_row(
            "SELECT COUNT(*) FROM reddit_opportunities WHERE post_id='post_url_fail' AND reply_status='posted'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n.to_string())
        .unwrap_or_default();
    assert_eq!(status, "0", "no row may be marked posted");

    std::fs::remove_dir_all(&project_dir).ok();
}

#[test]
fn post_reply_fails_stance_check_when_product_name_missing() {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("reddit_safety_stance");
    setup_stance_required_project(&project_dir);
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());

    // Passes base validation (3+ sentences, 30+ words, no URLs) but never
    // mentions "TestProduct" while the stance is REQUIRED.
    let draft = "Tracking expiry dates manually in a spreadsheet gets painful fast. \
                 I used to lose count of what was in the pantry every single week. \
                 A simple reminder habit fixed most of that waste for me.";
    assert!(
        pageseeds_lib::reddit::validation::validate_reply(draft).valid,
        "control: draft must pass base validation"
    );

    let task = make_task(
        &project_id,
        "reddit_reply",
        Some(reply_task_description("post_stance_fail", draft)),
    );

    let result = exec_reddit_post_reply(&task, &project_dir.to_string_lossy(), &conn);

    assert!(
        !result.success,
        "task must fail when REQUIRED product mention is missing"
    );
    assert!(
        result.message.contains("TestProduct"),
        "failure message should name the product, got: {}",
        result.message
    );

    std::fs::remove_dir_all(&project_dir).ok();
}

// ─── 2. Selftext plumbing ─────────────────────────────────────────────────────

#[test]
fn selftext_round_trips_through_db() {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("reddit_safety_selftext");
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());

    let mut opp = make_opportunity(&project_id, "post_selftext");
    opp.selftext = Some("This is the full post body that the LLM should see.".to_string());
    reddit_db::upsert_opportunity(&conn, &opp).expect("upsert failed");

    let fetched = reddit_db::get_opportunity(&conn, "post_selftext").expect("fetch failed");
    assert_eq!(
        fetched.selftext.as_deref(),
        Some("This is the full post body that the LLM should see."),
        "selftext must persist and read back"
    );

    // Update path: upserting again with a new body overwrites it.
    let mut updated = make_opportunity(&project_id, "post_selftext");
    updated.selftext = Some("Edited body.".to_string());
    reddit_db::upsert_opportunity(&conn, &updated).expect("re-upsert failed");
    let fetched = reddit_db::get_opportunity(&conn, "post_selftext").expect("fetch failed");
    assert_eq!(fetched.selftext.as_deref(), Some("Edited body."));

    std::fs::remove_dir_all(&project_dir).ok();
}

#[test]
fn persist_stores_selftext_from_search_json() {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("reddit_safety_persist_selftext");
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());

    let json = serde_json::json!({
        "posts": [
            {
                "post_id": "post_body_1",
                "title": "How do you track expiry dates?",
                "selftext": "I keep throwing away food because I forget what is in the fridge.",
                "subreddit": "mealprep"
            }
        ]
    })
    .to_string();

    persist_reddit_opportunities(&conn, &project_id, &json).expect("persist failed");

    let fetched = reddit_db::get_opportunity(&conn, "post_body_1").expect("fetch failed");
    assert_eq!(
        fetched.selftext.as_deref(),
        Some("I keep throwing away food because I forget what is in the fridge.")
    );
    assert_eq!(fetched.reply_status, "pending");

    std::fs::remove_dir_all(&project_dir).ok();
}

// ─── 3. Stale-marking instead of delete ───────────────────────────────────────

#[test]
fn persist_marks_pending_stale_and_preserves_posted() {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("reddit_safety_stale");
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());

    // Seed one pending and one posted row (a "previous run").
    let pending = make_opportunity(&project_id, "old_pending");
    reddit_db::upsert_opportunity(&conn, &pending).expect("seed pending failed");
    let posted = make_opportunity(&project_id, "old_posted");
    reddit_db::upsert_opportunity(&conn, &posted).expect("seed posted failed");
    reddit_db::mark_posted(&conn, "old_posted", "reply", "https://reddit.com/x")
        .expect("mark_posted failed");

    // New run brings one fresh post.
    let json = serde_json::json!({
        "posts": [
            { "post_id": "fresh_1", "title": "Fresh post", "subreddit": "testing" }
        ]
    })
    .to_string();
    persist_reddit_opportunities(&conn, &project_id, &json).expect("persist failed");

    let old = reddit_db::get_opportunity(&conn, "old_pending").expect("old row must still exist");
    assert_eq!(
        old.reply_status, "stale",
        "pending rows must be stale-marked, not deleted"
    );

    let posted = reddit_db::get_opportunity(&conn, "old_posted").expect("posted row must exist");
    assert_eq!(posted.reply_status, "posted", "posted history must be preserved");

    let fresh = reddit_db::get_opportunity(&conn, "fresh_1").expect("fresh row must exist");
    assert_eq!(fresh.reply_status, "pending");

    // Rediscovery: the stale post appears in the next run and is revived.
    let json = serde_json::json!({
        "posts": [
            { "post_id": "old_pending", "title": "Still relevant", "subreddit": "testing" }
        ]
    })
    .to_string();
    persist_reddit_opportunities(&conn, &project_id, &json).expect("persist failed");
    let revived = reddit_db::get_opportunity(&conn, "old_pending").expect("row must exist");
    assert_eq!(
        revived.reply_status, "pending",
        "re-discovered stale posts must flip back to pending"
    );

    std::fs::remove_dir_all(&project_dir).ok();
}

#[test]
fn update_reply_text_persists_user_edits() {
    let conn = create_test_db();
    let project_dir = unique_temp_dir("reddit_safety_edit");
    let project_id = create_test_project(&conn, &project_dir.to_string_lossy());

    let opp = make_opportunity(&project_id, "post_edit");
    reddit_db::upsert_opportunity(&conn, &opp).expect("upsert failed");

    reddit_db::update_reply_text(&conn, "post_edit", "User-edited draft text.")
        .expect("update_reply_text failed");

    let fetched = reddit_db::get_opportunity(&conn, "post_edit").expect("fetch failed");
    assert_eq!(fetched.reply_text.as_deref(), Some("User-edited draft text."));

    std::fs::remove_dir_all(&project_dir).ok();
}
