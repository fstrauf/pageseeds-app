use super::*;
use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskArtifact, TaskReviewSurface, TaskRun,
    TaskRunPolicy, TaskStatus,
};

fn make_task(artifacts: Vec<TaskArtifact>) -> Task {
    Task {
        id: "test-kw".to_string(),
        task_type: "research_keywords".to_string(),
        phase: "research".to_string(),
        status: TaskStatus::Review,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
        title: Some("Keyword test".to_string()),
        description: None,
        project_id: "proj1".to_string(),
        depends_on: vec![],
        artifacts,
        run: TaskRun::default(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        not_before: None,
    }
}

fn artifact(key: &str, content: serde_json::Value) -> TaskArtifact {
    TaskArtifact {
        key: key.to_string(),
        path: None,
        artifact_type: None,
        source: None,
        content: Some(content.to_string()),
    }
}

// ── parse_range_midpoint ──────────────────────────────────────────────────

#[test]
fn range_midpoint_range_string() {
    assert_eq!(parse_range_midpoint("1,000-10,000"), Some(5500));
}

#[test]
fn range_midpoint_single_number_with_comma() {
    assert_eq!(parse_range_midpoint("1,200"), Some(1200));
}

#[test]
fn range_midpoint_single_plain_number() {
    assert_eq!(parse_range_midpoint("1200"), Some(1200));
}

#[test]
fn range_midpoint_empty_string() {
    assert_eq!(parse_range_midpoint(""), None);
}

#[test]
fn range_midpoint_em_dash_placeholder() {
    assert_eq!(parse_range_midpoint("—"), None);
}

#[test]
fn range_midpoint_second_boundary_of_range() {
    assert_eq!(parse_range_midpoint("5,000-10,000"), Some(7500));
}

// ── extract_keywords_from_markdown_table ──────────────────────────────────

#[test]
fn markdown_table_extracts_keyword_column() {
    let raw = "| Priority | Keyword | Volume | KD |\n\
               |---|---|---|---|\n\
               | High | seo tools | 5,000-10,000 | 30 |\n\
               | Medium | content marketing | 1,000-5,000 | 45 |\n";
    let kws = extract_keywords_from_markdown_table(raw);
    assert!(kws.contains(&"seo tools".to_string()), "got: {kws:?}");
    assert!(
        kws.contains(&"content marketing".to_string()),
        "got: {kws:?}"
    );
}

#[test]
fn markdown_table_skips_header_row() {
    let raw = "| Priority | Keyword | Volume | KD |\n|---|---|---|---|\n";
    assert!(extract_keywords_from_markdown_table(raw).is_empty());
}

#[test]
fn markdown_table_deduplicates_keywords() {
    let raw = "| Priority | Keyword | Volume | KD |\n\
               |---|---|---|---|\n\
               | High | seo tools | 1,000 | 30 |\n\
               | High | seo tools | 2,000 | 35 |\n";
    let kws = extract_keywords_from_markdown_table(raw);
    assert_eq!(kws.iter().filter(|k| k.as_str() == "seo tools").count(), 1);
}

// ── extract_selectable_keywords ───────────────────────────────────────────

#[test]
fn selectable_reads_difficulty_results_array() {
    let json = serde_json::json!({
        "difficulty": {
            "results": [
                {"keyword": "seo tools", "difficulty": 30, "volume": "5,000-10,000"},
                {"keyword": "content strategy", "difficulty": 45, "volume": "1,000-5,000"},
            ]
        }
    });
    let task = make_task(vec![artifact("research_keywords_cli", json)]);
    let kws = extract_selectable_keywords(&task);
    assert!(kws.contains(&"seo tools".to_string()), "got: {kws:?}");
    assert!(
        kws.contains(&"content strategy".to_string()),
        "got: {kws:?}"
    );
}

#[test]
fn selectable_falls_back_to_new_keywords() {
    let json = serde_json::json!({
        "new_keywords": ["keyword a", "keyword b", "keyword c"]
    });
    let task = make_task(vec![artifact("research_keywords_cli", json)]);
    let kws = extract_selectable_keywords(&task);
    assert!(kws.contains(&"keyword a".to_string()), "got: {kws:?}");
    assert_eq!(kws.len(), 3);
}

#[test]
fn selectable_prefers_normalize_stage_over_cli_artifact() {
    let cli_json = serde_json::json!({
        "difficulty": {"results": [{"keyword": "from_cli", "difficulty": 20, "volume": "500"}]}
    });
    let norm_json = serde_json::json!({
        "difficulty": {"results": [{"keyword": "from_normalizer", "difficulty": 15, "volume": "1000"}]}
    });
    let task = make_task(vec![
        artifact("research_keywords_cli", cli_json),
        artifact("research_normalize_stage", norm_json),
    ]);
    let kws = extract_selectable_keywords(&task);
    assert!(kws.contains(&"from_normalizer".to_string()), "got: {kws:?}");
    assert!(!kws.contains(&"from_cli".to_string()), "got: {kws:?}");
}

#[test]
fn selectable_empty_for_no_artifacts() {
    let task = make_task(vec![]);
    assert!(extract_selectable_keywords(&task).is_empty());
}

// ── extract_keyword_metrics ───────────────────────────────────────────────

#[test]
fn metrics_reads_difficulty_and_volume_midpoint() {
    let json = serde_json::json!({
        "difficulty": {
            "results": [
                {"keyword": "seo tools", "difficulty": 28, "volume": "5,000-10,000"},
            ]
        }
    });
    let task = make_task(vec![artifact("research_keywords_cli", json)]);
    let metrics = extract_keyword_metrics(&task);
    let m = metrics.get("seo tools").expect("metric not found");
    assert_eq!(m.difficulty, Some(28));
    assert_eq!(m.volume, Some(7500)); // midpoint of 5000–10000
}

#[test]
fn metrics_handles_null_difficulty() {
    let json = serde_json::json!({
        "difficulty": {
            "results": [
                {"keyword": "hard keyword", "difficulty": null, "volume": "1,000-5,000"},
            ]
        }
    });
    let task = make_task(vec![artifact("research_keywords_cli", json)]);
    let metrics = extract_keyword_metrics(&task);
    let m = metrics.get("hard keyword").expect("metric not found");
    assert_eq!(m.difficulty, None);
    assert_eq!(m.volume, Some(3000)); // midpoint of 1000–5000
}

#[test]
fn metrics_empty_for_no_artifacts() {
    let task = make_task(vec![]);
    assert!(extract_keyword_metrics(&task).is_empty());
}

// ── normalize_keyword ─────────────────────────────────────────────────────

#[test]
fn normalize_trims_and_lowercases() {
    assert_eq!(normalize_keyword("  SEO Tools  "), "seo tools");
    assert_eq!(normalize_keyword("Content Marketing"), "content marketing");
}

// ── to_title_case ─────────────────────────────────────────────────────────

#[test]
fn debug_research_output_format() {
    // Sample output from research_ahrefs_pipeline to verify format
    let sample = r#"{
        "keywords": [
            {"keyword": "test", "volume": 100, "kd": 25.0, "traffic": 500.0, "has_data": true}
        ],
        "themes": ["test"],
        "total_candidates": 1,
        "with_data_count": 1
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(sample).unwrap();

    // Verify the format
    if let Some(keywords) = parsed.get("keywords").and_then(|k| k.as_array()) {
        if let Some(first) = keywords.first() {
            println!("Keyword: {:?}", first.get("keyword"));
            println!("Volume: {:?}", first.get("volume"));
            println!("KD: {:?}", first.get("kd"));
            println!("Traffic: {:?}", first.get("traffic"));
            println!("Has data: {:?}", first.get("has_data"));
        }
    }
}

#[test]
fn title_case_capitalizes_each_word() {
    assert_eq!(to_title_case("seo tools guide"), "Seo Tools Guide");
    assert_eq!(to_title_case("content"), "Content");
}

// ── spawner-based creation (idempotency) ───────────────────────────────

fn in_memory_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    // Minimal schema matching engine/spawner.rs test helper
    conn.execute_batch(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            active INTEGER DEFAULT 1
        );
        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            phase TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            priority TEXT NOT NULL DEFAULT 'medium',
            run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
            review_surface TEXT NOT NULL DEFAULT 'none',
            follow_up_policy TEXT NOT NULL DEFAULT 'none',
            agent_policy TEXT NOT NULL DEFAULT 'none',
            title TEXT,
            description TEXT,
            project_id TEXT NOT NULL,
            depends_on TEXT NOT NULL DEFAULT '[]',
            artifacts TEXT NOT NULL DEFAULT '[]',
            run_attempts INTEGER DEFAULT 0,
            run_last_error TEXT,
            run_provider TEXT,
            not_before TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE task_idempotency_keys (
            key TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT
        );
        CREATE TABLE task_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            provider TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            success INTEGER,
            error TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp/test', 1)",
        [],
    )
    .unwrap();
    conn
}

fn research_artifact_json() -> serde_json::Value {
    serde_json::json!({
        "difficulty": {
            "results": [
                {"keyword": "seo tools", "difficulty": 30, "volume": "5,000-10,000"},
                {"keyword": "content strategy", "difficulty": 45, "volume": "1,000-5,000"},
            ]
        }
    })
}

fn insert_research_task(conn: &rusqlite::Connection) -> String {
    crate::engine::spawner::TaskSpawner::spawn(
        conn,
        crate::engine::spawner::TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "research_keywords".to_string(),
            artifacts: vec![artifact(
                "research_final_selection",
                research_artifact_json(),
            )],
            ..Default::default()
        },
    )
    .unwrap()
    .id
}

#[test]
fn spec_carries_normalized_idempotency_key() {
    let task = make_task(vec![artifact(
        "research_final_selection",
        research_artifact_json(),
    )]);
    let specs = build_content_tasks_from_keywords(
        vec!["  SEO   Tools ".to_string()],
        &task,
        "test-kw",
        "proj1",
    )
    .unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0].idempotency_key.as_deref(),
        Some("write_article:proj1:seo tools")
    );
    assert_eq!(specs[0].depends_on, vec!["test-kw".to_string()]);
    assert_eq!(specs[0].agent_policy, AgentPolicy::None);
}

#[test]
fn reselecting_active_keyword_does_not_duplicate_task() {
    let conn = in_memory_db();
    let research_id = insert_research_task(&conn);

    let first =
        create_article_tasks_from_keywords(&conn, "proj1", &research_id, vec!["seo tools".into()])
            .unwrap();
    assert_eq!(first.len(), 1);

    // Re-select while the first write task is still active (todo).
    let second =
        create_article_tasks_from_keywords(&conn, "proj1", &research_id, vec!["seo tools".into()])
            .unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].id, second[0].id);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE type = 'write_article'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "no duplicate write_article task");
}

#[test]
fn casing_and_quote_variants_collapse_to_one_task() {
    let conn = in_memory_db();
    let research_id = insert_research_task(&conn);

    let first = create_article_tasks_from_keywords(
        &conn,
        "proj1",
        &research_id,
        vec!["  SEO   Tools ".into()],
    )
    .unwrap();
    let second = create_article_tasks_from_keywords(
        &conn,
        "proj1",
        &research_id,
        vec!["\"seo tools\"".into()],
    )
    .unwrap();
    assert_eq!(first[0].id, second[0].id);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE type = 'write_article'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn reselecting_after_previous_task_done_creates_new_task() {
    let conn = in_memory_db();
    let research_id = insert_research_task(&conn);

    let first =
        create_article_tasks_from_keywords(&conn, "proj1", &research_id, vec!["seo tools".into()])
            .unwrap();
    crate::engine::task_store::update_task_status(&conn, &first[0].id, TaskStatus::Done)
        .unwrap();

    let second =
        create_article_tasks_from_keywords(&conn, "proj1", &research_id, vec!["seo tools".into()])
            .unwrap();
    assert_ne!(first[0].id, second[0].id, "SkipIfActive allows re-creation once done");
}
