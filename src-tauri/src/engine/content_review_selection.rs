//! Content review follow-up selection: validate proposed tasks, store a
//! selectable artifact, and spawn only after the user chooses.
//!
//! Lifecycle lane: **UserSelection** via `ContentReviewPicker`.
//! `after_task_success` builds the proposals artifact; it does **not** spawn
//! child tasks. Spawning happens only in `spawn_from_selection`.

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::engine::exec::content::{
    mark_articles_in_review, recommendation_artifact, ArticleRecommendationPayload,
};
use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::content_review::{
    ContentReviewProposal, ContentReviewRecommendations, ContentReviewSelectableArtifact,
    DroppedProposal, ReviewArticleRecommendation, CONTENT_REVIEW_PROPOSALS_KEY,
};
use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskStatus};

/// Hard cap on selectable proposals shown to the user.
pub const MAX_PROPOSALS: usize = 5;

// ─── Raw proposal (pre-validation) ───────────────────────────────────────────

/// Intermediate shape before deterministic validation.
#[derive(Debug, Clone)]
pub struct RawProposal {
    pub id: String,
    pub task_type: String,
    pub title: String,
    pub description: Option<String>,
    pub params: serde_json::Value,
    pub idempotency_key: String,
    pub priority: Option<String>,
}

// ─── Normalize recommendations → raw proposals ───────────────────────────────

/// Convert recommendations into raw `fix_content_article` proposals.
///
/// One proposal per article. Params match what
/// `create_fix_content_article_tasks` previously used for TaskSpec/artifacts.
pub fn normalize_recommendations_to_proposals(
    recommendations: &ContentReviewRecommendations,
    project_id: &str,
) -> Vec<RawProposal> {
    let mut out = Vec::new();
    for article in &recommendations.articles {
        out.push(raw_from_article_recommendation(article, project_id));
    }
    out
}

fn raw_from_article_recommendation(
    article: &ReviewArticleRecommendation,
    project_id: &str,
) -> RawProposal {
    let article_id = article.article_id;
    let issue_count = article.suggestions.len();
    let priority = if issue_count >= 5 {
        "high"
    } else if issue_count >= 2 {
        "medium"
    } else {
        "low"
    };

    let title = format!("Fix: {}", article.article_title);
    let description = format!(
        "Apply SEO recommendations to '{}' ({} issue{}). File: {}",
        article.article_title,
        issue_count,
        if issue_count == 1 { "" } else { "s" },
        article.article_file
    );

    let params = serde_json::json!({
        "article_id": article_id,
        "article_title": article.article_title,
        "article_file": article.article_file,
        "url_slug": article.url_slug,
        "target_keyword": article.target_keyword,
        "suggestions": article.suggestions,
        "priority": priority,
    });

    RawProposal {
        id: format!("fix_content_article:{}", article_id),
        task_type: "fix_content_article".to_string(),
        title,
        description: Some(description),
        params,
        idempotency_key: format!("fix_content_article:{}:{}", project_id, article_id),
        priority: Some(priority.to_string()),
    }
}

/// Normalize a loose investigation-style `proposed_tasks` array (future #80)
/// into raw proposals. Unknown shapes are skipped with logging; validation
/// still applies the full drop rules.
fn normalize_proposed_tasks_json(
    proposed: &[serde_json::Value],
    project_id: &str,
) -> Vec<RawProposal> {
    let mut out = Vec::new();
    for (idx, item) in proposed.iter().enumerate() {
        let task_type = item
            .get("task_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled proposal")
            .to_string();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let params = item
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("proposed:{}:{}", task_type, idx));
        let idempotency_key = item
            .get("idempotency_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // Prefer article-based key when present so it matches fix spawn keys.
                if let Some(aid) = param_article_id(&params) {
                    format!("fix_content_article:{}:{}", project_id, aid)
                } else {
                    format!("{}:{}:{}", task_type, project_id, id)
                }
            });
        let priority = item
            .get("priority")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        out.push(RawProposal {
            id,
            task_type,
            title,
            description,
            params,
            idempotency_key,
            priority,
        });
    }
    out
}

// ─── Validate ────────────────────────────────────────────────────────────────

/// Deterministically validate raw proposals into a selectable artifact.
///
/// Drops unknown task types, missing required params, duplicate keys, and
/// proposals whose idempotency key already maps to an active task. Caps kept
/// proposals at [`MAX_PROPOSALS`].
pub fn validate_proposals(
    conn: &Connection,
    project_id: &str,
    raw: Vec<RawProposal>,
    source: &str,
    findings_summary: Option<String>,
) -> ContentReviewSelectableArtifact {
    let mut proposals = Vec::new();
    let mut dropped = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_keys = std::collections::HashSet::new();

    for proposal in raw {
        // Cap after validation of earlier items so dropped reasons are accurate.
        if proposals.len() >= MAX_PROPOSALS {
            dropped.push(DroppedProposal {
                reason: "cap_exceeded".to_string(),
                task_type: Some(proposal.task_type.clone()),
                detail: Some(format!(
                    "Proposal '{}' exceeded max of {} selectable proposals",
                    proposal.id, MAX_PROPOSALS
                )),
            });
            continue;
        }

        if proposal.task_type.is_empty()
            || crate::config::task_definitions::find(&proposal.task_type).is_none()
        {
            dropped.push(DroppedProposal {
                reason: "unknown_task_type".to_string(),
                task_type: Some(proposal.task_type.clone()),
                detail: Some(format!("Unknown task_type for proposal '{}'", proposal.id)),
            });
            continue;
        }

        if !seen_ids.insert(proposal.id.clone()) {
            dropped.push(DroppedProposal {
                reason: "duplicate_id".to_string(),
                task_type: Some(proposal.task_type.clone()),
                detail: Some(format!("Duplicate proposal id '{}'", proposal.id)),
            });
            continue;
        }

        if !seen_keys.insert(proposal.idempotency_key.clone()) {
            dropped.push(DroppedProposal {
                reason: "duplicate_idempotency_key".to_string(),
                task_type: Some(proposal.task_type.clone()),
                detail: Some(format!(
                    "Duplicate idempotency key '{}'",
                    proposal.idempotency_key
                )),
            });
            continue;
        }

        // Minimal param checks per known task type.
        if proposal.task_type == "fix_content_article" {
            if param_article_id(&proposal.params).is_none() {
                dropped.push(DroppedProposal {
                    reason: "missing_article_id".to_string(),
                    task_type: Some(proposal.task_type.clone()),
                    detail: Some(format!(
                        "fix_content_article proposal '{}' is missing article_id",
                        proposal.id
                    )),
                });
                continue;
            }
        }

        if has_active_task_for_key(conn, &proposal.idempotency_key) {
            dropped.push(DroppedProposal {
                reason: "active_task_exists".to_string(),
                task_type: Some(proposal.task_type.clone()),
                detail: Some(format!(
                    "Active task already exists for key '{}'",
                    proposal.idempotency_key
                )),
            });
            continue;
        }

        // project_id is part of the public validate API for future project-scoped checks.
        let _ = project_id;

        proposals.push(ContentReviewProposal {
            id: proposal.id,
            task_type: proposal.task_type,
            title: proposal.title,
            description: proposal.description,
            params: proposal.params,
            idempotency_key: proposal.idempotency_key,
            priority: proposal.priority,
        });
    }

    ContentReviewSelectableArtifact {
        findings_summary,
        proposals,
        dropped,
        source: source.to_string(),
    }
}

fn param_article_id(params: &serde_json::Value) -> Option<i64> {
    match params.get("article_id") {
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse().ok()
            }
        }
        _ => None,
    }
}

/// True when the idempotency key maps to an active (todo/queued/in_progress/review) task.
fn has_active_task_for_key(conn: &Connection, key: &str) -> bool {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT task_id, expires_at FROM task_idempotency_keys WHERE key = ?1",
            [key],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .ok()
        .flatten();

    let (task_id, expires_at) = match row {
        Some(v) => v,
        None => return false,
    };

    // Expired cooldown keys do not block.
    if let Some(exp) = expires_at {
        if let Ok(exp_dt) = chrono::DateTime::parse_from_rfc3339(&exp) {
            if chrono::Utc::now() > exp_dt.with_timezone(&chrono::Utc) {
                return false;
            }
        }
    }

    match task_store::get_task(conn, &task_id) {
        Ok(task) => matches!(
            task.status,
            TaskStatus::Todo | TaskStatus::Queued | TaskStatus::InProgress | TaskStatus::Review
        ),
        Err(_) => false,
    }
}

// ─── Build + store artifact (no spawn) ───────────────────────────────────────

/// Read recommendations (artifact or disk), validate, and upsert the
/// `content_review_proposals` artifact on the parent. Does **not** spawn tasks.
pub fn build_and_store_proposals_artifact(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Result<ContentReviewSelectableArtifact> {
    if !matches!(
        parent_task.task_type.as_str(),
        "content_review" | "content_audit"
    ) {
        return Err(Error::Validation(format!(
            "Parent task is not a content review (got '{}')",
            parent_task.task_type
        )));
    }

    let (raw, source, summary) = load_raw_proposals(conn, parent_task, project_path);

    let artifact = validate_proposals(
        conn,
        &parent_task.project_id,
        raw,
        &source,
        summary,
    );

    let content = serde_json::to_string(&artifact).map_err(|e| {
        Error::InvalidJson(format!("serialize content_review_proposals: {}", e))
    })?;

    let task_artifact = TaskArtifact {
        key: CONTENT_REVIEW_PROPOSALS_KEY.to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(source),
        content: Some(content),
    };
    task_store::upsert_task_artifact(conn, &parent_task.id, &task_artifact)?;

    log::info!(
        "[content_review_selection] stored {} proposal(s) ({} dropped) on task {}",
        artifact.proposals.len(),
        artifact.dropped.len(),
        parent_task.id
    );

    Ok(artifact)
}

/// Load raw proposals preferring investigation `proposed_tasks`, then the
/// `content_review_recommend` step artifact, then disk `recommendations.json`.
fn load_raw_proposals(
    _conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> (Vec<RawProposal>, String, Option<String>) {
    // 1) Future #80 path: any artifact carrying proposed_tasks.
    for art in &parent_task.artifacts {
        if let Some(content) = art.content.as_deref() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(arr) = val.get("proposed_tasks").and_then(|v| v.as_array()) {
                    if !arr.is_empty() {
                        let summary = val
                            .get("findings_summary")
                            .or_else(|| val.get("summary"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let raw =
                            normalize_proposed_tasks_json(arr, &parent_task.project_id);
                        return (raw, "investigation".to_string(), summary);
                    }
                }
            }
        }
    }

    // 2) Step artifact from content_review_recommend.
    if let Some(content) = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "content_review_recommend")
        .and_then(|a| a.content.as_deref())
    {
        if let Some((recs, summary)) = parse_recommendations_content(content) {
            if !recs.articles.is_empty() {
                let raw =
                    normalize_recommendations_to_proposals(&recs, &parent_task.project_id);
                return (raw, "recommendations".to_string(), summary);
            }
        }
    }

    // 3) Disk fallback: {project}/.github/automation/recommendations.json
    let paths = ProjectPaths::from_path(project_path);
    let rec_path = paths.automation_dir.join("recommendations.json");
    if let Ok(rec_str) = std::fs::read_to_string(&rec_path) {
        if let Some((recs, summary)) = parse_recommendations_content(&rec_str) {
            let raw = normalize_recommendations_to_proposals(&recs, &parent_task.project_id);
            return (raw, "recommendations".to_string(), summary);
        }
    }

    log::info!(
        "[content_review_selection] no recommendations found for task {}",
        parent_task.id
    );
    (Vec::new(), "recommendations".to_string(), None)
}

fn parse_recommendations_content(
    content: &str,
) -> Option<(ContentReviewRecommendations, Option<String>)> {
    // Prefer typed deserialization; fall back to lenient Value for partial shapes.
    if let Ok(recs) = serde_json::from_str::<ContentReviewRecommendations>(content) {
        let summary = if recs.articles.is_empty() {
            Some("No article recommendations were generated.".to_string())
        } else {
            Some(format!(
                "{} article(s) with fix recommendations",
                recs.articles.len()
            ))
        };
        return Some((recs, summary));
    }

    let val: serde_json::Value = serde_json::from_str(content).ok()?;
    let articles = val.get("articles").or_else(|| val.get("recommendations"))?;
    let mut recs = ContentReviewRecommendations {
        generated_at: val
            .get("generated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        total_articles: 0,
        articles: Vec::new(),
    };
    if let Some(arr) = articles.as_array() {
        for item in arr {
            if let Ok(a) = serde_json::from_value::<ReviewArticleRecommendation>(item.clone()) {
                recs.articles.push(a);
            }
        }
    }
    recs.total_articles = recs.articles.len();
    let summary = Some(format!(
        "{} article(s) with fix recommendations",
        recs.articles.len()
    ));
    Some((recs, summary))
}

// ─── Spawn from user selection ───────────────────────────────────────────────

/// Spawn follow-up tasks for the selected proposal ids.
///
/// - Loads `content_review_proposals` from the parent
/// - Rejects ids not present in the stored artifact
/// - Spawns via `TaskSpawner` with the same idempotency keys as the legacy path
/// - Marks articles in_review and parent Done
pub fn spawn_from_selection(
    conn: &Connection,
    parent_task_id: &str,
    proposal_ids: &[String],
) -> Result<Vec<Task>> {
    if proposal_ids.is_empty() {
        return Err(Error::Validation("No proposals selected".to_string()));
    }

    let parent_task = task_store::get_task(conn, parent_task_id)?;
    if !matches!(
        parent_task.task_type.as_str(),
        "content_review" | "content_audit"
    ) {
        return Err(Error::Validation(format!(
            "Parent task is not a content review (got '{}')",
            parent_task.task_type
        )));
    }

    let artifact = load_proposals_artifact(&parent_task)?;
    let by_id: std::collections::HashMap<&str, &ContentReviewProposal> = artifact
        .proposals
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();

    for id in proposal_ids {
        if !by_id.contains_key(id.as_str()) {
            return Err(Error::Validation(format!(
                "Proposal '{}' not found in content_review_proposals artifact",
                id
            )));
        }
    }

    let project = task_store::get_project(conn, &parent_task.project_id)?;
    let mut created = Vec::new();
    let mut in_review_ids = Vec::new();

    for id in proposal_ids {
        let proposal = by_id[id.as_str()];
        match spawn_one(conn, &parent_task, proposal) {
            Ok(task) => {
                if let Some(aid) = param_article_id(&proposal.params) {
                    in_review_ids.push(aid);
                }
                created.push(task);
            }
            Err(e) => {
                log::warn!(
                    "[content_review_selection] failed to spawn proposal '{}': {}",
                    id,
                    e
                );
                return Err(e);
            }
        }
    }

    if let Err(e) = mark_articles_in_review(
        conn,
        &parent_task.project_id,
        &project.path,
        &in_review_ids,
    ) {
        log::warn!(
            "[content_review_selection] failed to mark articles in_review: {}",
            e
        );
    }

    task_store::update_task_status(conn, parent_task_id, TaskStatus::Done)?;

    log::info!(
        "[content_review_selection] spawned {} task(s) from selection on {}",
        created.len(),
        parent_task_id
    );

    Ok(created)
}

fn load_proposals_artifact(parent: &Task) -> Result<ContentReviewSelectableArtifact> {
    let content = parent
        .artifacts
        .iter()
        .find(|a| a.key == CONTENT_REVIEW_PROPOSALS_KEY)
        .and_then(|a| a.content.as_deref())
        .ok_or_else(|| {
            Error::Validation(
                "content_review_proposals artifact missing on parent task".to_string(),
            )
        })?;
    serde_json::from_str(content).map_err(|e| {
        Error::InvalidJson(format!("content_review_proposals: {}", e))
    })
}

fn spawn_one(
    conn: &Connection,
    parent: &Task,
    proposal: &ContentReviewProposal,
) -> Result<Task> {
    // Currently only fix_content_article is emitted from the recommendations path.
    if proposal.task_type != "fix_content_article" {
        return Err(Error::Validation(format!(
            "Unsupported proposal task_type '{}' (only fix_content_article is spawnable)",
            proposal.task_type
        )));
    }

    let article_id = param_article_id(&proposal.params).ok_or_else(|| {
        Error::Validation(format!(
            "Proposal '{}' missing article_id in params",
            proposal.id
        ))
    })?;

    let article_title = proposal
        .params
        .get("article_title")
        .and_then(|v| v.as_str())
        .unwrap_or("article")
        .to_string();
    let article_file = proposal
        .params
        .get("article_file")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let url_slug = proposal
        .params
        .get("url_slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let target_keyword = proposal
        .params
        .get("target_keyword")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let suggestions = proposal
        .params
        .get("suggestions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let payload = ArticleRecommendationPayload {
        article_id,
        article_title: article_title.clone(),
        article_file: article_file.clone(),
        url_slug,
        target_keyword,
        suggestions,
    };
    let artifact = recommendation_artifact(&payload, "content_review");

    let priority = match proposal.priority.as_deref() {
        Some("high") => Priority::High,
        Some("low") => Priority::Low,
        _ => Priority::Medium,
    };

    let issue_count = payload.suggestions.len();
    let description = proposal.description.clone().or_else(|| {
        Some(format!(
            "Apply SEO recommendations to '{}' ({} issue{}). File: {}",
            article_title,
            issue_count,
            if issue_count == 1 { "" } else { "s" },
            article_file
        ))
    });

    let spec = TaskSpec {
        project_id: parent.project_id.clone(),
        task_type: "fix_content_article".to_string(),
        title: Some(proposal.title.clone()),
        description,
        phase: Some("implementation".to_string()),
        // Inherit UserEnqueue from task definition (human gate before repo writes).
        priority,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(proposal.idempotency_key.clone()),
        dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 30 }),
        artifacts: vec![artifact],
        // Match cannibalization/clarity: child depends on parent.
        depends_on: vec![parent.id.clone()],
        ..Default::default()
    };

    TaskSpawner::spawn(conn, spec)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
    fn validate_drops_unknown_task_type() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1", "/tmp");
        let raw = vec![RawProposal {
            id: "x:1".to_string(),
            task_type: "not_a_real_task".to_string(),
            title: "Bad".to_string(),
            description: None,
            params: json!({"article_id": 1}),
            idempotency_key: "k1".to_string(),
            priority: None,
        }];
        let art = validate_proposals(&conn, "proj1", raw, "test", None);
        assert!(art.proposals.is_empty());
        assert_eq!(art.dropped.len(), 1);
        assert_eq!(art.dropped[0].reason, "unknown_task_type");
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
}
