//! Shared slug → `fix_content_article` spawn path.
//!
//! CLI `create-task` and the investigate `CreateTaskTool` both route here so
//! every bare recovery create attaches a full `recommendations_{article_id}`
//! artifact with SERP categories (title / description / h1 / intro). Never
//! spawn `fix_content_article` without that artifact.

use rusqlite::Connection;

use crate::content::slug::normalize_url_slug;
use crate::engine::exec::content::{recommendation_artifact, ArticleRecommendationPayload};
use crate::engine::spawner::{DeduplicationPolicy, TaskSpec, TaskSpawner};
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::content_review::ReviewSuggestion;
use crate::models::task::{AgentPolicy, Priority, Task, TaskRunPolicy};

/// Options for [`spawn_fix_content_article_for_slug`].
#[derive(Debug, Clone)]
pub struct SpawnFixForSlugOpts {
    /// Override task title. Default: `Fix content: {article.title}`.
    pub title: Option<String>,
    pub priority: Priority,
    /// When true, set `run_policy = AutoEnqueue`.
    pub auto_enqueue: bool,
    /// Artifact `source` field (e.g. `"pageseeds-cli"`, `"create_task_tool"`).
    pub source: String,
}

impl Default for SpawnFixForSlugOpts {
    fn default() -> Self {
        Self {
            title: None,
            priority: Priority::Medium,
            auto_enqueue: false,
            source: String::new(),
        }
    }
}

/// Resolve `slug` to a project article and spawn a `fix_content_article` task
/// with a full `recommendations_{article_id}` artifact.
///
/// Idempotency key is always `fix_content_article:{project_id}:{article_id}`.
/// Dedup is `SkipIfActive` so re-runs after completion can create a new task.
pub fn spawn_fix_content_article_for_slug(
    conn: &Connection,
    project_id: &str,
    slug: &str,
    reason: &str,
    opts: SpawnFixForSlugOpts,
) -> Result<Task> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(Error::Validation(
            "fix_content_article requires a non-empty slug (article url_slug to fix)".to_string(),
        ));
    }

    let article = resolve_article_by_slug(conn, project_id, slug)?;
    let suggestions = build_serp_recovery_suggestions(reason);
    let suggestion_values: Vec<serde_json::Value> = suggestions
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect();

    let payload = ArticleRecommendationPayload {
        article_id: article.id,
        article_title: article.title.clone(),
        article_file: article.file.clone(),
        url_slug: article.url_slug.clone(),
        target_keyword: article.target_keyword.clone(),
        suggestions: suggestion_values,
    };

    let source = if opts.source.is_empty() {
        "slug_recovery"
    } else {
        opts.source.as_str()
    };
    let artifact = recommendation_artifact(&payload, source);

    let title = opts.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        format!("Fix content: {}", article.title)
    });
    let description = if reason.trim().is_empty() {
        format!(
            "SERP recovery for '{}' (title, meta description, H1, intro).",
            article.url_slug
        )
    } else {
        reason.to_string()
    };

    let spec = TaskSpec {
        project_id: project_id.to_string(),
        task_type: "fix_content_article".to_string(),
        title: Some(title),
        description: Some(description),
        priority: opts.priority,
        run_policy: if opts.auto_enqueue {
            Some(TaskRunPolicy::AutoEnqueue)
        } else {
            None
        },
        agent_policy: AgentPolicy::Required,
        artifacts: vec![artifact],
        idempotency_key: Some(format!(
            "fix_content_article:{}:{}",
            project_id, article.id
        )),
        dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
        ..Default::default()
    };

    TaskSpawner::spawn(conn, spec)
}

/// Match article by exact `url_slug` or normalized slug (same rule as CLI).
fn resolve_article_by_slug(
    conn: &Connection,
    project_id: &str,
    slug: &str,
) -> Result<crate::models::article::Article> {
    let slug_norm = normalize_url_slug(slug);
    task_store::list_articles(conn, project_id)?
        .into_iter()
        .find(|a| a.url_slug == slug || normalize_url_slug(&a.url_slug) == slug_norm)
        .ok_or_else(|| Error::Validation(format!("No article found for slug '{slug}'")))
}

/// Build SERP-category recovery suggestions (title / description / h1 / intro).
///
/// Always emits the default high-priority SERP template so recovery never invents
/// rewrite instructions from free-text. When `reason` is non-empty, it is appended
/// as `Context: …` on each suggestion’s `reason` field only — `proposed` stays the
/// stable default. Operator context already lives on the task description.
pub(crate) fn build_serp_recovery_suggestions(reason: &str) -> Vec<ReviewSuggestion> {
    let mut suggestions = default_serp_template();
    let context = reason.trim();
    if !context.is_empty() {
        for s in &mut suggestions {
            s.reason = format!("{} Context: {}", s.reason, context);
        }
    }
    suggestions
}

fn default_serp_template() -> Vec<ReviewSuggestion> {
    vec![
        ReviewSuggestion {
            category: "title".to_string(),
            current: String::new(),
            proposed: "Refresh the title for CTR and rank defense while keeping primary intent clear."
                .to_string(),
            reason: "SERP recovery: title needs CTR/rank defense refresh.".to_string(),
            priority: Some("high".to_string()),
        },
        ReviewSuggestion {
            category: "description".to_string(),
            current: String::new(),
            proposed: "Refresh the meta description to improve SERP snippet CTR and query coverage."
                .to_string(),
            reason: "SERP recovery: meta description should reinforce the primary query.".to_string(),
            priority: Some("high".to_string()),
        },
        ReviewSuggestion {
            category: "h1".to_string(),
            current: String::new(),
            proposed: "Align the H1 with the primary search intent and target keyword.".to_string(),
            reason: "SERP recovery: H1 should match primary intent.".to_string(),
            priority: Some("high".to_string()),
        },
        ReviewSuggestion {
            category: "intro".to_string(),
            current: String::new(),
            proposed: "Strengthen the introduction and value proposition for scanners and rankers."
                .to_string(),
            reason: "SERP recovery: intro should lead with clear value and intent match.".to_string(),
            priority: Some("high".to_string()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::task::TaskStatus;
    use uuid::Uuid;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', '/tmp/test', 1, 'workspace')",
            rusqlite::params![id],
        )
        .unwrap();
    }

    fn insert_article(conn: &Connection, project_id: &str, id: i64, slug: &str, title: &str) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status,
                content_gaps_addressed, target_volume, word_count, review_count
             ) VALUES (?1, ?2, ?3, ?4, 'content/blog/article.mdx', 'published', '[]', 0, 500, 0)",
            rusqlite::params![id, project_id, title, slug],
        )
        .unwrap();
    }

    #[test]
    fn build_serp_default_has_four_categories() {
        let suggestions = build_serp_recovery_suggestions("");
        let cats: Vec<&str> = suggestions.iter().map(|s| s.category.as_str()).collect();
        assert_eq!(cats, vec!["title", "description", "h1", "intro"]);
        assert!(suggestions.iter().all(|s| s.priority.as_deref() == Some("high")));
        assert!(!suggestions.iter().any(|s| s.category == "content"));
        // Stable default proposed text (no free-text protocol).
        let defaults = default_serp_template();
        for (got, def) in suggestions.iter().zip(defaults.iter()) {
            assert_eq!(got.proposed, def.proposed);
            assert_eq!(got.reason, def.reason);
        }
    }

    #[test]
    fn build_serp_with_reason_keeps_default_proposed_and_appends_context() {
        let reason = "GSC CTR drop 3%→1% | Improve title for CTR";
        let suggestions = build_serp_recovery_suggestions(reason);
        let cats: Vec<&str> = suggestions.iter().map(|s| s.category.as_str()).collect();
        assert_eq!(cats, vec!["title", "description", "h1", "intro"]);
        assert!(!cats.iter().any(|c| *c == "content"));

        let defaults = default_serp_template();
        for (got, def) in suggestions.iter().zip(defaults.iter()) {
            // proposed must never be overwritten by free-text reason fragments
            assert_eq!(got.proposed, def.proposed);
            assert_eq!(
                got.reason,
                format!("{} Context: {}", def.reason, reason)
            );
            // Operator metric text must not become a rewrite instruction
            assert!(!got.proposed.contains("3%"));
            assert!(!got.proposed.contains("GSC CTR drop"));
        }
    }

    #[test]
    fn spawn_attaches_recommendations_artifact_with_article_id_and_serp_cats() {
        let conn = in_memory_db();
        let project_id = "proj1";
        insert_project(&conn, project_id);
        insert_article(&conn, project_id, 42, "my-article", "My Article");

        let task = spawn_fix_content_article_for_slug(
            &conn,
            project_id,
            "my-article",
            "",
            SpawnFixForSlugOpts {
                source: "test".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(task.task_type, "fix_content_article");
        assert_eq!(
            task.title.as_deref(),
            Some("Fix content: My Article")
        );

        let art = task
            .artifacts
            .iter()
            .find(|a| a.key == "recommendations_42")
            .expect("recommendations_42 artifact");
        assert_eq!(art.source.as_deref(), Some("test"));

        let payload: ArticleRecommendationPayload =
            serde_json::from_str(art.content.as_deref().unwrap()).unwrap();
        assert_eq!(payload.article_id, 42);
        assert_eq!(payload.url_slug, "my-article");
        assert_eq!(payload.article_title, "My Article");
        assert_eq!(payload.article_file, "content/blog/article.mdx");

        let cats: Vec<String> = payload
            .suggestions
            .iter()
            .filter_map(|s| s.get("category").and_then(|c| c.as_str()).map(|s| s.to_string()))
            .collect();
        assert_eq!(cats, vec!["title", "description", "h1", "intro"]);
        assert!(!cats.iter().any(|c| c == "content"));
    }

    #[test]
    fn spawn_rejects_unknown_slug() {
        let conn = in_memory_db();
        insert_project(&conn, "proj1");
        insert_article(&conn, "proj1", 1, "exists", "Exists");

        let err = spawn_fix_content_article_for_slug(
            &conn,
            "proj1",
            "missing-slug",
            "reason",
            SpawnFixForSlugOpts {
                source: "test".to_string(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("No article found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn spawn_rejects_empty_slug() {
        let conn = in_memory_db();
        insert_project(&conn, "proj1");
        let err = spawn_fix_content_article_for_slug(
            &conn,
            "proj1",
            "  ",
            "",
            SpawnFixForSlugOpts::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-empty slug"));
    }

    #[test]
    fn spawn_idempotency_returns_existing_active_task() {
        let conn = in_memory_db();
        let project_id = "proj1";
        insert_project(&conn, project_id);
        insert_article(&conn, project_id, 7, "slug-a", "Article A");

        let opts = SpawnFixForSlugOpts {
            source: "test".to_string(),
            ..Default::default()
        };
        let first = spawn_fix_content_article_for_slug(
            &conn,
            project_id,
            "slug-a",
            "refresh title",
            opts.clone(),
        )
        .unwrap();
        let second = spawn_fix_content_article_for_slug(
            &conn,
            project_id,
            "slug-a",
            "refresh title again",
            opts,
        )
        .unwrap();

        assert_eq!(first.id, second.id, "SkipIfActive must return existing active task");

        // Idempotency key uses article_id, not slug.
        let key: String = conn
            .query_row(
                "SELECT key FROM task_idempotency_keys WHERE task_id = ?1",
                rusqlite::params![first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(key, format!("fix_content_article:{project_id}:7"));

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE type = 'fix_content_article' AND project_id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(first.status, TaskStatus::Todo);
    }

    #[test]
    fn spawn_matches_normalized_slug() {
        let conn = in_memory_db();
        insert_project(&conn, "proj1");
        // Store a slug that normalizes the same as the input form with trailing slash path.
        insert_article(&conn, "proj1", 3, "my-post", "My Post");

        let task = spawn_fix_content_article_for_slug(
            &conn,
            "proj1",
            "my-post/",
            "",
            SpawnFixForSlugOpts {
                source: "pageseeds-cli".to_string(),
                title: Some(format!("custom-{}", Uuid::new_v4())),
                priority: Priority::High,
                auto_enqueue: true,
            },
        )
        .unwrap();

        assert_eq!(task.task_type, "fix_content_article");
        assert_eq!(task.priority, Priority::High);
        assert_eq!(task.run_policy, TaskRunPolicy::AutoEnqueue);
        assert!(task.artifacts.iter().any(|a| a.key == "recommendations_3"));
    }
}
