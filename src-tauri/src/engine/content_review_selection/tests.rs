use super::*;
use crate::models::content_review::ReviewSuggestion;
use crate::models::task::{
    FollowUpPolicy, Priority, TaskReviewSurface, TaskRun, TaskRunPolicy,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TempProjectDir {
    path: PathBuf,
}

impl TempProjectDir {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .join(format!("pageseeds-cr-selection-{}", Uuid::new_v4()));
        fs::create_dir_all(path.join(".github").join("automation")).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempProjectDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            content_dir TEXT,
            site_url TEXT,
            site_id TEXT,
            sitemap_url TEXT,
            project_mode TEXT NOT NULL DEFAULT 'workspace',
            active INTEGER DEFAULT 1,
            agent_provider TEXT,
            seo_provider TEXT,
            clarity_project_id TEXT
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
        );
        CREATE TABLE articles (
            id INTEGER NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            url_slug TEXT NOT NULL DEFAULT '',
            file TEXT NOT NULL DEFAULT '',
            target_keyword TEXT,
            keyword_difficulty TEXT,
            target_volume INTEGER DEFAULT 0,
            published_date TEXT,
            word_count INTEGER DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'draft',
            review_status TEXT,
            review_started_at TEXT,
            last_reviewed_at TEXT,
            review_count INTEGER NOT NULL DEFAULT 0,
            content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
            estimated_traffic_monthly TEXT,
            page_type TEXT,
            content_hash TEXT,
            last_edited_at TEXT,
            project_id TEXT NOT NULL,
            PRIMARY KEY (id, project_id)
        );
        CREATE TABLE articles_meta (
            project_id TEXT PRIMARY KEY,
            next_article_id INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE article_metadata (
            project_id TEXT NOT NULL,
            article_id INTEGER NOT NULL,
            namespace TEXT NOT NULL,
            payload TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL,
            PRIMARY KEY (project_id, article_id, namespace)
        );",
    )
    .unwrap();
    conn
}

fn create_test_project(conn: &Connection, id: &str, path: &str) {
    conn.execute(
        "INSERT INTO projects (id, name, path, active) VALUES (?1, ?2, ?3, 1)",
        rusqlite::params![id, "Test Project", path],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO articles_meta (project_id, next_article_id) VALUES (?1, 200)",
        [id],
    )
    .unwrap();
}

fn insert_article(conn: &Connection, project_id: &str, id: i64) {
    conn.execute(
        "INSERT INTO articles (
            id, title, url_slug, file, status, project_id, content_gaps_addressed
         ) VALUES (?1, ?2, ?3, ?4, 'published', ?5, '[]')",
        rusqlite::params![
            id,
            format!("Article {id}"),
            format!("article-{id}"),
            format!("./content/{id}_article.mdx"),
            project_id,
        ],
    )
    .unwrap();
}

fn make_parent(project_id: &str, artifacts: Vec<TaskArtifact>) -> Task {
    let now = chrono::Utc::now().to_rfc3339();
    Task {
        id: format!("task-{}", Uuid::new_v4()),
        project_id: project_id.to_string(),
        task_type: "content_review".to_string(),
        phase: "investigation".to_string(),
        status: TaskStatus::Review,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ContentReviewPicker,
        follow_up_policy: FollowUpPolicy::UserSelection,
        agent_policy: AgentPolicy::Required,
        title: Some("Content Review".to_string()),
        description: None,
        depends_on: vec![],
        artifacts,
        run: TaskRun::default(),
        created_at: now.clone(),
        updated_at: now,
        not_before: None,
    }
}

fn sample_suggestion() -> ReviewSuggestion {
    ReviewSuggestion {
        category: "title".to_string(),
        current: "Old".to_string(),
        proposed: "New".to_string(),
        reason: "better".to_string(),
        priority: Some("high".to_string()),
    }
}

fn sample_recs(ids: &[i64]) -> ContentReviewRecommendations {
    ContentReviewRecommendations {
        generated_at: chrono::Utc::now().to_rfc3339(),
        total_articles: ids.len(),
        articles: ids
            .iter()
            .map(|id| ReviewArticleRecommendation {
                article_id: *id,
                article_title: format!("Article {id}"),
                article_file: format!("./content/{id}_article.mdx"),
                url_slug: format!("article-{id}"),
                target_keyword: Some(format!("kw {id}")),
                suggestions: vec![sample_suggestion()],
            })
            .collect(),
    }
}

fn persist_parent(conn: &Connection, parent: &Task) {
    task_store::create_task(conn, parent).unwrap();
}

fn write_recommendations(project_dir: &Path, recommendations: serde_json::Value) {
    let path = project_dir
        .join(".github")
        .join("automation")
        .join("recommendations.json");
    fs::write(
        path,
        serde_json::to_string_pretty(&recommendations).unwrap(),
    )
    .unwrap();
}

fn idempotency_keys(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT key FROM task_idempotency_keys ORDER BY key")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

/// Build proposals from disk recommendations and spawn all selectable ones.
/// Canonical selection path used by migrated fix-spawn tests.
fn build_and_spawn_all(
    conn: &Connection,
    parent: &Task,
    project_path: &str,
) -> Vec<Task> {
    persist_parent(conn, parent);
    let art = build_and_store_proposals_artifact(conn, parent, project_path).unwrap();
    let ids: Vec<String> = art.proposals.iter().map(|p| p.id.clone()).collect();
    if ids.is_empty() {
        return Vec::new();
    }
    spawn_from_selection(conn, &parent.id, &ids).unwrap()
}

#[test]
fn normalize_recommendations_emits_fix_content_article() {
    let recs = sample_recs(&[10, 20]);
    let raw = normalize_recommendations_to_proposals(&recs, "proj1");
    assert_eq!(raw.len(), 2);
    assert_eq!(raw[0].task_type, "fix_content_article");
    assert_eq!(raw[0].id, "fix_content_article:10");
    assert_eq!(
        raw[0].idempotency_key,
        "fix_content_article:proj1:10"
    );
    assert!(raw[0].title.starts_with("Fix: "));
    assert_eq!(param_article_id(&raw[0].params), Some(10));
}

#[test]
fn validate_drops_unsupported_task_type() {
    let conn = in_memory_db();
    create_test_project(&conn, "proj1", "/tmp");
    let raw = vec![
        RawProposal {
            id: "x:1".to_string(),
            task_type: "not_a_real_task".to_string(),
            title: "Bad".to_string(),
            description: None,
            params: json!({"article_id": 1}),
            idempotency_key: "k1".to_string(),
            priority: None,
        },
        // Known task type that is still not spawnable from content_review.
        RawProposal {
            id: "write:1".to_string(),
            task_type: "write_article".to_string(),
            title: "Write".to_string(),
            description: None,
            params: json!({"keyword": "foo"}),
            idempotency_key: "k2".to_string(),
            priority: None,
        },
    ];
    let art = validate_proposals(&conn, "proj1", raw, "test", None);
    assert!(art.proposals.is_empty());
    assert_eq!(art.dropped.len(), 2);
    assert!(art
        .dropped
        .iter()
        .all(|d| d.reason == "unsupported for content_review selection"));
}

#[test]
fn validate_drops_missing_article_id() {
    let conn = in_memory_db();
    create_test_project(&conn, "proj1", "/tmp");
    let raw = vec![RawProposal {
        id: "fix_content_article:1".to_string(),
        task_type: "fix_content_article".to_string(),
        title: "Fix: missing".to_string(),
        description: None,
        params: json!({"article_title": "No id"}),
        idempotency_key: "fix_content_article:proj1:1".to_string(),
        priority: None,
    }];
    let art = validate_proposals(&conn, "proj1", raw, "test", None);
    assert!(art.proposals.is_empty());
    assert_eq!(art.dropped[0].reason, "missing_article_id");
}

#[test]
fn validate_caps_at_five() {
    let conn = in_memory_db();
    create_test_project(&conn, "proj1", "/tmp");
    let recs = sample_recs(&[1, 2, 3, 4, 5, 6, 7]);
    let raw = normalize_recommendations_to_proposals(&recs, "proj1");
    let art = validate_proposals(&conn, "proj1", raw, "recommendations", None);
    assert_eq!(art.proposals.len(), 5);
    assert!(art.dropped.iter().any(|d| d.reason == "cap_exceeded"));
    assert_eq!(
        art.dropped
            .iter()
            .filter(|d| d.reason == "cap_exceeded")
            .count(),
        2
    );
}

#[test]
fn validate_drops_duplicate_idempotency_keys_in_batch() {
    let conn = in_memory_db();
    create_test_project(&conn, "proj1", "/tmp");
    let raw = vec![
        RawProposal {
            id: "fix_content_article:1".to_string(),
            task_type: "fix_content_article".to_string(),
            title: "A".to_string(),
            description: None,
            params: json!({"article_id": 1}),
            idempotency_key: "fix_content_article:proj1:1".to_string(),
            priority: None,
        },
        RawProposal {
            id: "fix_content_article:1b".to_string(),
            task_type: "fix_content_article".to_string(),
            title: "B".to_string(),
            description: None,
            params: json!({"article_id": 1}),
            idempotency_key: "fix_content_article:proj1:1".to_string(),
            priority: None,
        },
    ];
    let art = validate_proposals(&conn, "proj1", raw, "test", None);
    assert_eq!(art.proposals.len(), 1);
    assert_eq!(art.dropped[0].reason, "duplicate_idempotency_key");
}

#[test]
fn validate_drops_when_active_task_exists() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);
    insert_article(&conn, "proj1", 1);

    // Seed an active fix task + idempotency key.
    let existing = TaskSpawner::spawn(
        &conn,
        TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "fix_content_article".to_string(),
            title: Some("existing".to_string()),
            idempotency_key: Some("fix_content_article:proj1:1".to_string()),
            dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 30 }),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(existing.status, TaskStatus::Todo);

    let recs = sample_recs(&[1]);
    let raw = normalize_recommendations_to_proposals(&recs, "proj1");
    let art = validate_proposals(&conn, "proj1", raw, "recommendations", None);
    assert!(art.proposals.is_empty());
    assert_eq!(art.dropped[0].reason, "active_task_exists");
}

#[test]
fn build_and_store_from_disk_recommendations() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);
    insert_article(&conn, "proj1", 42);

    let recs = sample_recs(&[42]);
    fs::write(
        dir.path()
            .join(".github")
            .join("automation")
            .join("recommendations.json"),
        serde_json::to_string_pretty(&recs).unwrap(),
    )
    .unwrap();

    let parent = make_parent("proj1", vec![]);
    persist_parent(&conn, &parent);

    let art = build_and_store_proposals_artifact(&conn, &parent, &path).unwrap();
    assert_eq!(art.proposals.len(), 1);
    assert_eq!(art.source, "recommendations");
    assert_eq!(art.proposals[0].id, "fix_content_article:42");

    let reloaded = task_store::get_task(&conn, &parent.id).unwrap();
    assert!(reloaded
        .artifacts
        .iter()
        .any(|a| a.key == CONTENT_REVIEW_PROPOSALS_KEY));
}

#[test]
fn build_and_store_prefers_step_artifact_over_disk() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);

    // Disk has article 1, step artifact has article 99 — artifact wins.
    let disk = sample_recs(&[1]);
    fs::write(
        dir.path()
            .join(".github")
            .join("automation")
            .join("recommendations.json"),
        serde_json::to_string_pretty(&disk).unwrap(),
    )
    .unwrap();

    let step_recs = sample_recs(&[99]);
    let parent = make_parent(
        "proj1",
        vec![TaskArtifact {
            key: "content_review_recommend".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("content_review_recommend".to_string()),
            content: Some(serde_json::to_string(&step_recs).unwrap()),
        }],
    );
    persist_parent(&conn, &parent);

    let art = build_and_store_proposals_artifact(&conn, &parent, &path).unwrap();
    assert_eq!(art.proposals.len(), 1);
    assert_eq!(art.proposals[0].id, "fix_content_article:99");
}

#[test]
fn selection_rejects_unknown_proposal_id() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);

    let recs = sample_recs(&[1]);
    let raw = normalize_recommendations_to_proposals(&recs, "proj1");
    let selectable = validate_proposals(&conn, "proj1", raw, "recommendations", None);
    let parent = make_parent(
        "proj1",
        vec![TaskArtifact {
            key: CONTENT_REVIEW_PROPOSALS_KEY.to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("recommendations".to_string()),
            content: Some(serde_json::to_string(&selectable).unwrap()),
        }],
    );
    persist_parent(&conn, &parent);

    let err = spawn_from_selection(&conn, &parent.id, &["not-a-real-id".to_string()])
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn selection_spawns_and_marks_parent_done() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);
    insert_article(&conn, "proj1", 7);

    let recs = sample_recs(&[7]);
    let raw = normalize_recommendations_to_proposals(&recs, "proj1");
    let selectable = validate_proposals(&conn, "proj1", raw, "recommendations", None);
    assert_eq!(selectable.proposals.len(), 1);
    let proposal_id = selectable.proposals[0].id.clone();

    let parent = make_parent(
        "proj1",
        vec![TaskArtifact {
            key: CONTENT_REVIEW_PROPOSALS_KEY.to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("recommendations".to_string()),
            content: Some(serde_json::to_string(&selectable).unwrap()),
        }],
    );
    persist_parent(&conn, &parent);

    let created = spawn_from_selection(&conn, &parent.id, &[proposal_id.clone()]).unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].task_type, "fix_content_article");
    assert_eq!(created[0].status, TaskStatus::Todo);
    assert_eq!(
        created[0].run_policy,
        crate::models::task::TaskRunPolicy::UserEnqueue
    );

    let parent_after = task_store::get_task(&conn, &parent.id).unwrap();
    assert_eq!(parent_after.status, TaskStatus::Done);

    let articles = task_store::list_articles(&conn, "proj1").unwrap();
    let article = articles.iter().find(|a| a.id == 7).unwrap();
    assert_eq!(article.review_status.as_deref(), Some("in_review"));
}

#[test]
fn selection_is_idempotent_on_rerun() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);
    insert_article(&conn, "proj1", 7);

    let recs = sample_recs(&[7]);
    let raw = normalize_recommendations_to_proposals(&recs, "proj1");
    let selectable = validate_proposals(&conn, "proj1", raw, "recommendations", None);
    let proposal_id = selectable.proposals[0].id.clone();

    let parent = make_parent(
        "proj1",
        vec![TaskArtifact {
            key: CONTENT_REVIEW_PROPOSALS_KEY.to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("recommendations".to_string()),
            content: Some(serde_json::to_string(&selectable).unwrap()),
        }],
    );
    persist_parent(&conn, &parent);

    let first = spawn_from_selection(&conn, &parent.id, &[proposal_id.clone()]).unwrap();
    assert_eq!(first.len(), 1);
    let first_id = first[0].id.clone();

    // Parent is Done; re-selection still loads artifact and returns existing task.
    // Reset parent to review so re-selection is allowed by our function (we only
    // require content_review type, not review status).
    let second = spawn_from_selection(&conn, &parent.id, &[proposal_id]).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].id, first_id);

    let all = task_store::list_tasks(&conn, "proj1").unwrap();
    let fixes: Vec<_> = all
        .iter()
        .filter(|t| t.task_type == "fix_content_article")
        .collect();
    assert_eq!(fixes.len(), 1);
}

#[test]
fn after_task_success_stores_proposals_without_spawning() {
    let conn = in_memory_db();
    let dir = TempProjectDir::new();
    let path = dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &path);
    insert_article(&conn, "proj1", 5);

    let recs = sample_recs(&[5]);
    fs::write(
        dir.path()
            .join(".github")
            .join("automation")
            .join("recommendations.json"),
        serde_json::to_string_pretty(&recs).unwrap(),
    )
    .unwrap();

    let parent = make_parent("proj1", vec![]);
    persist_parent(&conn, &parent);

    let _follow_ups = crate::engine::post_actions::after_task_success(
        &crate::engine::post_actions::PostTaskContext {
            conn: &conn,
            task: &parent,
            project_path: &path,
            progress: &[],
        },
    );
    // UserSelection: must not auto-spawn fix_content_article children.
    // (generate_feature_spec may still be spawned as an unrelated monthly audit follow-up.)
    let tasks = task_store::list_tasks(&conn, "proj1").unwrap();
    assert!(
        !tasks.iter().any(|t| t.task_type == "fix_content_article"),
        "content_review must not auto-spawn fix_content_article"
    );

    let reloaded = task_store::get_task(&conn, &parent.id).unwrap();
    let proposals_art = reloaded
        .artifacts
        .iter()
        .find(|a| a.key == CONTENT_REVIEW_PROPOSALS_KEY)
        .expect("proposals artifact should be stored");
    let selectable: ContentReviewSelectableArtifact =
        serde_json::from_str(proposals_art.content.as_deref().unwrap()).unwrap();
    assert_eq!(selectable.proposals.len(), 1);
}

// ─── Migrated from create_fix_content_article_tasks (legacy auto-spawn) ─────

#[test]
fn selection_uses_numeric_article_ids_in_idempotency_keys() {
    let conn = in_memory_db();
    let project_dir = TempProjectDir::new();
    let project_path = project_dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 109);
    insert_article(&conn, "proj1", 111);

    write_recommendations(
        project_dir.path(),
        json!({
            "articles": [
                {
                    "article_id": 109,
                    "article_title": "Alpha",
                    "article_file": "./content/109_alpha.mdx",
                    "suggestions": [{
                        "category": "title",
                        "current": "a",
                        "proposed": "b",
                        "reason": "r"
                    }]
                },
                {
                    "article_id": 111,
                    "article_title": "Beta",
                    "article_file": "./content/111_beta.mdx",
                    "suggestions": [
                        {
                            "category": "meta_description",
                            "current": "a",
                            "proposed": "b",
                            "reason": "r"
                        },
                        {
                            "category": "cta",
                            "current": "a",
                            "proposed": "b",
                            "reason": "r"
                        }
                    ]
                }
            ]
        }),
    );

    let parent = make_parent("proj1", vec![]);
    let created = build_and_spawn_all(&conn, &parent, &project_path);

    assert_eq!(created.len(), 2);
    // Parent also registered an idempotency key if any; filter to fix keys.
    let fix_keys: Vec<String> = idempotency_keys(&conn)
        .into_iter()
        .filter(|k| k.starts_with("fix_content_article:"))
        .collect();
    assert_eq!(
        fix_keys,
        vec![
            "fix_content_article:proj1:109".to_string(),
            "fix_content_article:proj1:111".to_string(),
        ]
    );

    assert!(created.iter().all(|task| task.task_type == "fix_content_article"));
    assert!(created.iter().all(|task| {
        task.artifacts.iter().any(|artifact| {
            artifact.key == "recommendations_109" || artifact.key == "recommendations_111"
        })
    }));

    let articles = task_store::list_articles(&conn, "proj1").unwrap();
    assert!(articles
        .iter()
        .all(|article| article.review_status.as_deref() == Some("in_review")));

    let exported: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            project_dir
                .path()
                .join(".github")
                .join("automation")
                .join("articles.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let exported_articles = exported["articles"].as_array().unwrap();
    assert!(exported_articles
        .iter()
        .all(|article| article["review_status"] == "in_review"));
}

#[test]
fn selection_gates_fixes_behind_user_enqueue() {
    let conn = in_memory_db();
    let project_dir = TempProjectDir::new();
    let project_path = project_dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 109);

    write_recommendations(
        project_dir.path(),
        json!({
            "articles": [
                {
                    "article_id": 109,
                    "article_title": "Alpha",
                    "article_file": "./content/109_alpha.mdx",
                    "suggestions": [{
                        "category": "title",
                        "current": "a",
                        "proposed": "b",
                        "reason": "r"
                    }]
                }
            ]
        }),
    );

    let parent = make_parent("proj1", vec![]);
    let created = build_and_spawn_all(&conn, &parent, &project_path);
    assert_eq!(created.len(), 1);

    // Selection leaves fix tasks pending explicit user enqueue: UserEnqueue + Todo.
    assert!(created
        .iter()
        .all(|task| task.run_policy == TaskRunPolicy::UserEnqueue));
    assert!(created.iter().all(|task| task.status == TaskStatus::Todo));
}

#[test]
fn selection_skips_invalid_and_duplicate_article_ids() {
    let conn = in_memory_db();
    let project_dir = TempProjectDir::new();
    let project_path = project_dir.path().to_string_lossy().to_string();
    create_test_project(&conn, "proj1", &project_path);
    insert_article(&conn, "proj1", 109);

    write_recommendations(
        project_dir.path(),
        json!({
            "articles": [
                {
                    "article_id": 109,
                    "article_title": "Alpha",
                    "article_file": "./content/109_alpha.mdx",
                    "suggestions": [{
                        "category": "title",
                        "current": "a",
                        "proposed": "b",
                        "reason": "r"
                    }]
                },
                {
                    "article_id": 109,
                    "article_title": "Alpha Duplicate",
                    "article_file": "./content/109_alpha_dup.mdx",
                    "suggestions": [{
                        "category": "cta",
                        "current": "a",
                        "proposed": "b",
                        "reason": "r"
                    }]
                },
                {
                    "article_title": "Missing ID",
                    "article_file": "./content/missing_id.mdx",
                    "suggestions": [{
                        "category": "faq",
                        "current": "a",
                        "proposed": "b",
                        "reason": "r"
                    }]
                }
            ]
        }),
    );

    let parent = make_parent("proj1", vec![]);
    persist_parent(&conn, &parent);
    let art = build_and_store_proposals_artifact(&conn, &parent, &project_path).unwrap();
    // Missing article_id is skipped at parse; same article_id yields the same
    // proposal id, so the duplicate is dropped as duplicate_id at validate.
    assert_eq!(art.proposals.len(), 1);
    assert!(art.dropped.iter().any(|d| d.reason == "duplicate_id"));

    let created = spawn_from_selection(
        &conn,
        &parent.id,
        &[art.proposals[0].id.clone()],
    )
    .unwrap();
    assert_eq!(created.len(), 1);
    let fix_keys: Vec<String> = idempotency_keys(&conn)
        .into_iter()
        .filter(|k| k.starts_with("fix_content_article:"))
        .collect();
    assert_eq!(
        fix_keys,
        vec!["fix_content_article:proj1:109".to_string()]
    );
}
