//! Shared slug → `fix_indexing_internal_links` spawn path + artifact contract.
//!
//! CLI `create-task -t fix_indexing_internal_links -S <slug>` and the investigate
//! `CreateTaskTool` both route here so every bare create attaches a valid
//! `indexing_link_target` artifact (IHC child shape). Never spawn this task type
//! without that artifact — execute fails at context/plan/apply/verify.
//!
//! Also owns:
//! - typed `indexing_link_target` artifact builder (IHC + operator + GSC recovery)
//! - shared-keyword source shortlist (exact match, skip self / already-linking, cap 8)

use std::collections::HashSet;

use rusqlite::Connection;

use crate::content::slug::normalize_url_slug;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{DeduplicationPolicy, TaskSpec, TaskSpawner};
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::indexing_health::{
    IndexingLinkTarget, IndexingLinkTargetArtifact, LinkSourceCandidate,
};
use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskRunPolicy};

/// Article-scoped idempotency key for all `fix_indexing_internal_links` spawn paths.
pub fn fix_indexing_internal_links_idempotency_key(project_id: &str, article_id: i64) -> String {
    format!("fix_indexing_internal_links:{project_id}:{article_id}")
}

/// Build the durable `indexing_link_target` task artifact (IHC child shape).
///
/// Used by operator slug spawn, IHC `build_add_links_spec`, and GSC recovery.
pub fn indexing_link_target_artifact(
    campaign_task_id: Option<String>,
    target: IndexingLinkTarget,
    source: &str,
) -> TaskArtifact {
    let doc = IndexingLinkTargetArtifact {
        campaign_task_id,
        target,
    };
    TaskArtifact {
        key: "indexing_link_target".to_string(),
        path: None,
        artifact_type: Some("indexing_link_target".to_string()),
        source: Some(source.to_string()),
        content: Some(serde_json::to_string(&doc).unwrap_or_else(|_| {
            r#"{"target":{"url":"","slug":"","article_id":0,"file":"","reason_code":"","incoming_link_count_before":0,"target_keyword":"","source_candidates":[]}}"#.to_string()
        })),
    }
}

/// Input row for [`shortlist_shared_keyword_source_candidates`].
///
/// Owned fields so both `Article` slices and IHC JSON maps can build rows without
/// temporary-borrow lifetime issues.
#[derive(Debug, Clone)]
pub struct SharedKeywordArticleRef {
    pub article_id: i64,
    pub slug: String,
    pub title: String,
    pub file: String,
    pub target_keyword: Option<String>,
}

/// Shared-keyword source shortlist for indexing internal-link fixes.
///
/// Rules (single ranking algorithm for IHC build_context and operator spawn):
/// - exact case-insensitive `target_keyword` match
/// - skip self (`article_id == target_article_id` or id 0)
/// - skip sources already linking to the target
/// - cap 8
///
/// Empty shortlist is OK (target has no keyword, or no matching sources).
pub fn shortlist_shared_keyword_source_candidates(
    target_article_id: i64,
    target_keyword: &str,
    articles: impl IntoIterator<Item = SharedKeywordArticleRef>,
    already_linking_source_ids: &HashSet<i64>,
) -> Vec<LinkSourceCandidate> {
    let target_keyword = target_keyword.trim();
    if target_keyword.is_empty() {
        return Vec::new();
    }
    let target_kw_lower = target_keyword.to_lowercase();
    let mut candidates: Vec<LinkSourceCandidate> = Vec::new();

    for src in articles {
        if src.article_id == 0 || src.article_id == target_article_id {
            continue;
        }
        if already_linking_source_ids.contains(&src.article_id) {
            continue;
        }
        let src_kw = src.target_keyword.as_deref().unwrap_or("").trim();
        if src_kw.is_empty() {
            continue;
        }
        if src_kw.to_lowercase() != target_kw_lower {
            continue;
        }
        candidates.push(LinkSourceCandidate {
            article_id: src.article_id,
            slug: src.slug,
            title: src.title,
            file: src.file,
            reason: "shared target keyword".to_string(),
        });
        if candidates.len() >= 8 {
            break;
        }
    }

    candidates
}

/// Source article IDs whose `outgoing_ids` already include the target.
pub fn already_linking_source_ids_from_scan(
    link_scan: Option<&serde_json::Value>,
    target_article_id: i64,
) -> HashSet<i64> {
    link_scan
        .and_then(|v| v["profiles"].as_array())
        .map(|profiles| {
            profiles
                .iter()
                .filter_map(|p| {
                    let source_id = p["id"].as_i64()?;
                    let outgoing = p["outgoing_ids"].as_array()?;
                    let links = outgoing
                        .iter()
                        .any(|o| o.as_i64() == Some(target_article_id));
                    if links {
                        Some(source_id)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Options for [`spawn_fix_indexing_internal_links_for_slug`].
#[derive(Debug, Clone)]
pub struct SpawnFixIndexingLinksForSlugOpts {
    /// Override task title. Default: `Add links: {url_slug}`.
    pub title: Option<String>,
    pub priority: Priority,
    /// When true, set `run_policy = AutoEnqueue`.
    pub auto_enqueue: bool,
    /// Artifact `source` field (e.g. `"pageseeds-cli"`, `"create_task_tool"`).
    pub source: String,
    /// Optional task description / operator reason.
    pub reason: Option<String>,
}

impl Default for SpawnFixIndexingLinksForSlugOpts {
    fn default() -> Self {
        Self {
            title: None,
            priority: Priority::Medium,
            auto_enqueue: false,
            source: String::new(),
            reason: None,
        }
    }
}

/// Resolve `slug` to a project article and spawn a `fix_indexing_internal_links`
/// task with a full `indexing_link_target` artifact.
///
/// Idempotency key is `fix_indexing_internal_links:{project_id}:{article_id}`.
/// Dedup is `SkipIfActive` so re-runs after completion can create a new task.
pub fn spawn_fix_indexing_internal_links_for_slug(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    slug: &str,
    opts: SpawnFixIndexingLinksForSlugOpts,
) -> Result<Task> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(Error::Validation(
            "fix_indexing_internal_links requires a non-empty slug (article url_slug to add inbound links for)"
                .to_string(),
        ));
    }

    let article = resolve_article_by_slug(conn, project_id, slug)?;
    if article.id == 0 {
        return Err(Error::Validation(format!(
            "Article for slug '{slug}' has id 0 — cannot build indexing_link_target"
        )));
    }

    let articles = task_store::list_articles(conn, project_id)?;
    let paths = ProjectPaths::from_path(project_path);
    let link_scan = load_link_scan(&paths);

    let incoming_before = incoming_count_from_scan(link_scan.as_ref(), article.id);
    let already_linked = already_linking_source_ids_from_scan(link_scan.as_ref(), article.id);
    let target_keyword = article.target_keyword.clone().unwrap_or_default();
    let source_candidates = shortlist_shared_keyword_source_candidates(
        article.id,
        &target_keyword,
        articles.iter().map(|a| SharedKeywordArticleRef {
            article_id: a.id,
            slug: a.url_slug.clone(),
            title: a.title.clone(),
            file: a.file.clone(),
            target_keyword: a.target_keyword.clone(),
        }),
        &already_linked,
    );

    let url = build_article_url(conn, project_id, &article.url_slug);
    let source = if opts.source.is_empty() {
        "slug_recovery"
    } else {
        opts.source.as_str()
    };

    let artifact = indexing_link_target_artifact(
        None,
        IndexingLinkTarget {
            url,
            slug: article.url_slug.clone(),
            article_id: article.id,
            file: article.file.clone(),
            reason_code: "operator_scoped".to_string(),
            incoming_link_count_before: incoming_before,
            target_keyword,
            source_candidates,
        },
        source,
    );

    let title = opts
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| format!("Add links: {}", article.url_slug));

    let description = opts
        .reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "Operator-scoped internal-link fix for slug '{}' (article_id {}). Builds indexing_link_target without IHC/recovery parent.",
                article.url_slug, article.id
            )
        });

    let spec = TaskSpec {
        project_id: project_id.to_string(),
        task_type: "fix_indexing_internal_links".to_string(),
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
        idempotency_key: Some(fix_indexing_internal_links_idempotency_key(
            project_id, article.id,
        )),
        dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
        ..Default::default()
    };

    TaskSpawner::spawn(conn, spec)
}

/// Match article by exact `url_slug` or normalized slug (same rule as content_fix / ctr_fix).
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

/// Build absolute blog URL when the project has a site base; otherwise a
/// deterministic placeholder host so the artifact still has a full URL.
fn build_article_url(conn: &Connection, project_id: &str, url_slug: &str) -> String {
    let base = task_store::get_project(conn, project_id)
        .ok()
        .and_then(|p| p.site_base_url())
        .filter(|b| !b.is_empty());
    match base {
        Some(b) => format!("{}/blog/{}", b.trim_end_matches('/'), url_slug),
        None => format!("https://example.invalid/blog/{url_slug}"),
    }
}

fn load_link_scan(paths: &ProjectPaths) -> Option<serde_json::Value> {
    let path = paths.automation_dir.join("link_scan.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn incoming_count_from_scan(link_scan: Option<&serde_json::Value>, article_id: i64) -> usize {
    link_scan
        .and_then(|v| v["profiles"].as_array())
        .and_then(|profiles| {
            profiles
                .iter()
                .find(|p| p["id"].as_i64() == Some(article_id))
                .and_then(|p| p["incoming_ids"].as_array())
                .map(|arr| arr.len())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::task::TaskStatus;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::init_with_conn(&conn).unwrap();
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

    fn insert_project_with_site(conn: &Connection, id: &str, path: &str, site_url: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode, site_url)
             VALUES (?1, 'Test', ?2, 1, 'workspace', ?3)",
            rusqlite::params![id, path, site_url],
        )
        .unwrap();
    }

    fn insert_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        slug: &str,
        title: &str,
        file: &str,
        target_keyword: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', ?6, '[]', 0, 500, 0)",
            rusqlite::params![id, project_id, title, slug, file, target_keyword],
        )
        .unwrap();
    }

    fn setup_project() -> (String, Connection) {
        let path = std::env::temp_dir()
            .join(format!(
                "indexing_link_fix_slug_test_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        let auto = std::path::Path::new(&path)
            .join(".github")
            .join("automation");
        std::fs::create_dir_all(&auto).unwrap();

        let conn = in_memory_db();
        insert_project(&conn, "proj1", &path);
        insert_article(
            &conn,
            "proj1",
            42,
            "my-orphan-page",
            "My Orphan Page",
            "content/my_orphan_page.mdx",
            Some("machine learning"),
        );

        (path, conn)
    }

    #[test]
    fn shortlist_exact_keyword_skip_self_and_cap() {
        let articles = [
            SharedKeywordArticleRef {
                article_id: 1,
                slug: "self".into(),
                title: "Self".into(),
                file: "self.mdx".into(),
                target_keyword: Some("kw".into()),
            },
            SharedKeywordArticleRef {
                article_id: 2,
                slug: "a".into(),
                title: "A".into(),
                file: "a.mdx".into(),
                target_keyword: Some("kw".into()),
            },
            SharedKeywordArticleRef {
                article_id: 3,
                slug: "b".into(),
                title: "B".into(),
                file: "b.mdx".into(),
                target_keyword: Some("other".into()),
            },
            SharedKeywordArticleRef {
                article_id: 4,
                slug: "c".into(),
                title: "C".into(),
                file: "c.mdx".into(),
                target_keyword: Some("KW".into()), // case-insensitive match but already linked
            },
        ];
        let already = HashSet::from([4i64]);
        let out = shortlist_shared_keyword_source_candidates(1, "kw", articles, &already);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].article_id, 2);
        assert_eq!(out[0].reason, "shared target keyword");
    }

    #[test]
    fn spawn_attaches_indexing_link_target_with_article_id() {
        let (path, conn) = setup_project();

        let task = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "my-orphan-page",
            SpawnFixIndexingLinksForSlugOpts {
                source: "test".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(task.task_type, "fix_indexing_internal_links");
        assert_eq!(task.title.as_deref(), Some("Add links: my-orphan-page"));

        let art = task
            .artifacts
            .iter()
            .find(|a| a.key == "indexing_link_target")
            .expect("indexing_link_target artifact");
        assert_eq!(art.source.as_deref(), Some("test"));
        assert_eq!(
            art.artifact_type.as_deref(),
            Some("indexing_link_target")
        );

        let parsed = crate::engine::exec::content::parse_target_artifact(&task)
            .expect("parse_target_artifact should succeed");
        assert_eq!(parsed.article_id, 42);
        assert_eq!(parsed.slug, "my-orphan-page");
        assert_eq!(parsed.file, "content/my_orphan_page.mdx");
        assert_eq!(parsed.reason_code, "operator_scoped");
        assert_eq!(parsed.target_keyword, "machine learning");
        assert!(parsed.url.contains("my-orphan-page"));
        // No other articles with shared keyword → empty shortlist is OK
        assert!(parsed.source_candidates.is_empty());

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_includes_shared_keyword_candidates() {
        let (path, conn) = setup_project();
        insert_article(
            &conn,
            "proj1",
            7,
            "source-a",
            "Source A",
            "content/source_a.mdx",
            Some("machine learning"),
        );
        insert_article(
            &conn,
            "proj1",
            8,
            "source-b",
            "Source B",
            "content/source_b.mdx",
            Some("other keyword"),
        );

        let task = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "my-orphan-page",
            SpawnFixIndexingLinksForSlugOpts {
                source: "test".to_string(),
                ..Default::default()
            },
        )
        .unwrap();

        let target = crate::engine::exec::content::parse_target_artifact(&task).unwrap();
        assert_eq!(target.source_candidates.len(), 1);
        assert_eq!(target.source_candidates[0].article_id, 7);
        assert_eq!(target.source_candidates[0].slug, "source-a");
        assert_eq!(
            target.source_candidates[0].reason,
            "shared target keyword"
        );

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_rejects_unknown_slug() {
        let (path, conn) = setup_project();
        let err = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "missing-slug",
            SpawnFixIndexingLinksForSlugOpts::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("No article found"),
            "unexpected: {err}"
        );
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_rejects_empty_slug() {
        let (path, conn) = setup_project();
        let err = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "  ",
            SpawnFixIndexingLinksForSlugOpts::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("non-empty slug"),
            "unexpected: {err}"
        );
        assert!(
            err.to_string().contains("fix_indexing_internal_links"),
            "error should name the task type: {err}"
        );
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_idempotency_returns_existing_active_task() {
        let (path, conn) = setup_project();
        let opts = SpawnFixIndexingLinksForSlugOpts {
            source: "test".to_string(),
            ..Default::default()
        };
        let first = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "my-orphan-page",
            opts.clone(),
        )
        .unwrap();
        let second = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "my-orphan-page",
            opts,
        )
        .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.status, TaskStatus::Todo);

        let key: String = conn
            .query_row(
                "SELECT key FROM task_idempotency_keys WHERE task_id = ?1",
                rusqlite::params![first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(key, "fix_indexing_internal_links:proj1:42");

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn spawn_matches_normalized_slug_and_site_url() {
        let path = std::env::temp_dir()
            .join(format!(
                "indexing_link_fix_norm_{}",
                std::process::id()
            ))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(
            std::path::Path::new(&path)
                .join(".github")
                .join("automation"),
        )
        .unwrap();

        let conn = in_memory_db();
        insert_project_with_site(&conn, "proj1", &path, "sc-domain:example.com");
        insert_article(
            &conn,
            "proj1",
            3,
            "my-post",
            "My Post",
            "content/my_post.mdx",
            None,
        );

        let task = spawn_fix_indexing_internal_links_for_slug(
            &conn,
            "proj1",
            &path,
            "my-post/",
            SpawnFixIndexingLinksForSlugOpts {
                source: "pageseeds-cli".to_string(),
                priority: Priority::High,
                auto_enqueue: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(task.task_type, "fix_indexing_internal_links");
        assert_eq!(task.priority, Priority::High);
        assert_eq!(task.run_policy, TaskRunPolicy::AutoEnqueue);

        let target = crate::engine::exec::content::parse_target_artifact(&task).unwrap();
        assert_eq!(target.url, "https://example.com/blog/my-post");
        // No keyword → empty candidates still valid artifact
        assert!(target.source_candidates.is_empty());

        let _ = std::fs::remove_dir_all(&path);
    }
}
