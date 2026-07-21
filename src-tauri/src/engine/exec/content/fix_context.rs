/// Deterministic context builder for the content fix pipeline.
///
/// 1. Reads recommendations.json from the automation dir.
/// 2. Finds the article's recommendations by article_id (from task artifact).
/// 3. Reads the current MDX file.
/// 4. Builds a structured context JSON consumed by the generate step.
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;
use super::fix_generate::normalize_target_keyword;

pub(crate) fn exec_fix_content_article_context(
    task: &Task,
    project_path: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Resolve article_id from task artifacts
    let article_id = match super::fix_content_article_id(task) {
        Some(id) => id,
        None => {
            return StepResult::fail("No article_id found in task artifacts".to_string());
        }
    };

    // Try to load recommendations from task artifact first (self-contained),
    // fall back to recommendations.json on disk.
    let article_rec = task
        .artifacts
        .iter()
        .find(|a| a.key.starts_with("recommendations_"))
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .or_else(|| {
            let rec_path = paths.automation_dir.join("recommendations.json");
            std::fs::read_to_string(&rec_path).ok().and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s).ok().and_then(|rec| {
                    rec["articles"].as_array().and_then(|articles| {
                        articles
                            .iter()
                            .find(|a| {
                                a["article_id"]
                                    .as_i64()
                                    .or_else(|| a["article_id"].as_str().and_then(|s| s.parse().ok()))
                                    == Some(article_id)
                            })
                            .cloned()
                    })
                })
            })
        });

    let article_rec = match article_rec {
        Some(a) => a,
        None => {
            return StepResult::fail(format!(
                    "Article {} not found in task artifacts or recommendations.json",
                    article_id
                ));
        }
    };

    // Deserialize into the shared payload contract. Historical artifacts may
    // carry extra/missing fields or an unexpected shape, so anything that
    // does not fit degrades to defaults — same tolerance as the previous
    // loose `serde_json::Value` indexing.
    let payload = serde_json::from_value::<super::ArticleRecommendationPayload>(article_rec)
        .unwrap_or_default();

    // The article's own slug, excluded from the link-target list below: an
    // article must never link to itself. Prefer the stored url_slug; fall back
    // to the file stem for historical artifacts that lack it.
    let own_slug = if !payload.url_slug.is_empty() {
        crate::content::slug::normalize_url_slug(&payload.url_slug)
    } else {
        crate::content::slug::normalize_url_slug(&crate::content::ops::slug_from_filename(
            &payload.article_file,
        ))
    };

    let file = payload.article_file;
    let article_title = payload.article_title;
    let target_keyword = payload
        .target_keyword
        .as_deref()
        .map(|s| normalize_target_keyword(s, article_id));
    let suggestions = serde_json::Value::Array(payload.suggestions);

    // Read current file content
    let file_path = match crate::engine::exec::audit_health::resolve_content_file(repo_root, &file) {
        Some(p) => p,
        None => {
            return StepResult::fail(format!(
                    "File not found: {}. Run sanitize_content to repair paths.",
                    file
                ));
        }
    };

    let file_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to read file {}: {}", file_path.display(), e));
        }
    };

    // Deterministic link-target enrichment: the SAME set that
    // `validate_patch_before_write` enforces downstream
    // (`task_store::load_valid_link_targets`), supplied upstream so the model
    // never has to discover project data itself. DB open/lookup failures
    // degrade to an empty list — the prompt then falls back to the old
    // "do not link when unsure" behavior.
    let link_slugs: Vec<String> = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(db) => available_link_slugs(&db, &task.project_id, project_path, &own_slug),
        Err(_) => Vec::new(),
    };

    let link_target_count = link_slugs.len();

    // Build structured context (lightweight — generate step reads the file itself)
    let context = serde_json::json!({
        "article_id": article_id,
        "article_title": article_title,
        "article_file": file,
        "target_keyword": target_keyword,
        "suggestions": suggestions,
        "available_link_slugs": link_slugs,
    });

    let context_json = match serde_json::to_string_pretty(&context) {
        Ok(s) => s,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize context: {}", e));
        }
    };

    StepResult {
        success: true,
        message: format!(
            "Built fix context for article {} ({} suggestions, {} link targets, {} chars content)",
            article_id,
            suggestions.as_array().map(|a| a.len()).unwrap_or(0),
            link_target_count,
            file_content.len()
        ),
        output: Some(context_json),
    }
}

/// Maximum number of link-target slugs rendered into the fix prompt. 200
/// covers typical blogs comfortably; beyond that the list would dominate the
/// prompt and inflate token cost, so the tail is dropped deterministically
/// (sorted order). `validate_patch_before_write` still enforces the full set.
const MAX_LINK_TARGET_SLUGS: usize = 200;

/// Sorted, own-slug-excluded list of valid internal link targets for a
/// project, built from the single source of truth
/// (`task_store::load_valid_link_targets`).
fn available_link_slugs(
    conn: &rusqlite::Connection,
    project_id: &str,
    project_path: &str,
    own_slug: &str,
) -> Vec<String> {
    let mut slugs: Vec<String> =
        crate::engine::task_store::load_valid_link_targets(conn, project_id, project_path)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s != own_slug)
            .collect();
    // Sorted so prompt rendering and the cap above are deterministic.
    slugs.sort();
    slugs.truncate(MAX_LINK_TARGET_SLUGS);
    slugs
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskReviewSurface, TaskRun,
        TaskRunPolicy, TaskStatus,
    };
    use rusqlite::Connection;
    use uuid::Uuid;

    struct TempProjectDir {
        path: std::path::PathBuf,
    }

    impl TempProjectDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("pageseeds-fix-context-{}", Uuid::new_v4()));
            std::fs::create_dir_all(path.join(".github").join("automation")).unwrap();
            std::fs::create_dir_all(path.join("content").join("blog")).unwrap();
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempProjectDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', ?2, 1, 'workspace')",
            rusqlite::params![id, path],
        )
        .unwrap();
    }

    fn insert_article(conn: &Connection, project_id: &str, id: i64, slug: &str) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status,
                content_gaps_addressed, target_volume, word_count, review_count
             ) VALUES (?1, ?2, ?3, ?4, 'article.mdx', 'draft', '[]', 0, 0, 0)",
            rusqlite::params![id, project_id, format!("Article {}", id), slug],
        )
        .unwrap();
    }

    #[test]
    fn available_link_slugs_excludes_own_slug_and_sorts() {
        let conn = in_memory_db();
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        insert_project(&conn, "p1", &project_path);
        insert_article(&conn, "p1", 1, "zebra-post");
        insert_article(&conn, "p1", 2, "own-article");
        insert_article(&conn, "p1", 3, "alpha-post");
        insert_article(&conn, "p1", 4, "mid-post");

        let slugs = available_link_slugs(&conn, "p1", &project_path, "own-article");
        assert_eq!(slugs, vec!["alpha-post", "mid-post", "zebra-post"]);

        // Deterministic: a second call yields the identical list.
        let again = available_link_slugs(&conn, "p1", &project_path, "own-article");
        assert_eq!(slugs, again);
    }

    #[test]
    fn available_link_slugs_caps_at_max() {
        let conn = in_memory_db();
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        insert_project(&conn, "p1", &project_path);
        for id in 0..(MAX_LINK_TARGET_SLUGS + 50) {
            insert_article(&conn, "p1", id as i64, &format!("post-{:04}", id));
        }

        let slugs = available_link_slugs(&conn, "p1", &project_path, "no-such-slug");
        assert_eq!(slugs.len(), MAX_LINK_TARGET_SLUGS);
        // Sorted cap keeps the lexicographically first slugs.
        assert_eq!(slugs[0], "post-0000");
        assert_eq!(slugs[MAX_LINK_TARGET_SLUGS - 1], "post-0199");
    }

    #[test]
    fn context_step_enriches_available_link_slugs() {
        // Mutates process-global env — serialize against other env-mutating tests.
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();
        let db_path = dir.path().join("test.db");
        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        {
            let conn = Connection::open(&db_path).unwrap();
            crate::db::init_with_conn(&conn).unwrap();
            insert_project(&conn, "p1", &project_path);
            insert_article(&conn, "p1", 1, "own-article");
            insert_article(&conn, "p1", 2, "other-post");
            insert_article(&conn, "p1", 3, "another-post");
        }
        std::env::set_var("PAGESEEDS_DB_PATH", &db_path);

        // The article being fixed, on disk so the context step can read it.
        std::fs::write(
            dir.path().join("content").join("blog").join("own-article.mdx"),
            "---\ntitle: \"Own Article\"\ndate: \"2026-01-01\"\n---\n\n# Own Article\n\nBody.\n",
        )
        .unwrap();

        let recommendations = serde_json::json!({
            "article_id": 1,
            "article_title": "Own Article",
            "article_file": "content/blog/own-article.mdx",
            "url_slug": "own-article",
            "target_keyword": "own article",
            "suggestions": []
        });

        let now = chrono::Utc::now().to_rfc3339();
        let task = Task {
            id: "task-fix-ctx".to_string(),
            project_id: "p1".to_string(),
            task_type: "fix_content_article".to_string(),
            phase: "fix".to_string(),
            status: TaskStatus::InProgress,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: Some("Fix article".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![TaskArtifact {
                key: "recommendations_1".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("content_review".to_string()),
                content: Some(recommendations.to_string()),
            }],
            run: TaskRun::default(),
            created_at: now.clone(),
            not_before: None,
            updated_at: now,
        };

        let result = exec_fix_content_article_context(&task, &project_path);
        match old_db {
            Some(v) => std::env::set_var("PAGESEEDS_DB_PATH", v),
            None => std::env::remove_var("PAGESEEDS_DB_PATH"),
        }

        assert!(result.success, "context step failed: {}", result.message);
        let context: serde_json::Value =
            serde_json::from_str(&result.output.unwrap()).unwrap();
        let slugs: Vec<&str> = context["available_link_slugs"]
            .as_array()
            .expect("available_link_slugs must be an array")
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(slugs, vec!["another-post", "other-post"]);
    }
}
