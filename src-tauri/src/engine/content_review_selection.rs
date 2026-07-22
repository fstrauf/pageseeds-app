//! Content review follow-up selection: validate proposed tasks, store a
//! selectable artifact, and spawn only after the user chooses.
//!
//! Lifecycle lane: **UserSelection** via `ContentReviewPicker`.
//! `after_task_success` builds the proposals artifact; it does **not** spawn
//! child tasks. Spawning happens only in `spawn_from_selection`.
//!
//! This PR scopes proposals to recommendations → `fix_content_article` only.
//! Generic multi-type proposed_tasks handling belongs in a future issue.

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

/// Only task type this selection surface can spawn.
const SUPPORTED_TASK_TYPE: &str = "fix_content_article";

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
/// One proposal per article. Params match what `spawn_one` needs for TaskSpec
/// and the `recommendations_{id}` artifact.
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
        task_type: SUPPORTED_TASK_TYPE.to_string(),
        title,
        description: Some(description),
        params,
        idempotency_key: format!("fix_content_article:{}:{}", project_id, article_id),
        priority: Some(priority.to_string()),
    }
}

// ─── Validate ────────────────────────────────────────────────────────────────

/// Deterministically validate raw proposals into a selectable artifact.
///
/// Only `fix_content_article` is accepted. Other task types are dropped with
/// reason `"unsupported for content_review selection"` so the picker never
/// shows a proposal that `spawn_one` cannot spawn. Also drops missing required
/// params, duplicate keys, and proposals whose idempotency key already maps to
/// an active task. Caps kept proposals at [`MAX_PROPOSALS`].
pub fn validate_proposals(
    conn: &Connection,
    _project_id: &str,
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

        if proposal.task_type != SUPPORTED_TASK_TYPE {
            dropped.push(DroppedProposal {
                reason: "unsupported for content_review selection".to_string(),
                task_type: Some(proposal.task_type.clone()),
                detail: Some(format!(
                    "Only {SUPPORTED_TASK_TYPE} is selectable for content_review (got '{}' on proposal '{}')",
                    proposal.task_type, proposal.id
                )),
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

    let (raw, source, summary) = load_raw_proposals(parent_task, project_path);

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

/// Load raw proposals from the `content_review_recommend` step artifact, then
/// disk `recommendations.json`. Recommendations → fix_content_article only.
fn load_raw_proposals(
    parent_task: &Task,
    project_path: &str,
) -> (Vec<RawProposal>, String, Option<String>) {
    // 1) Step artifact from content_review_recommend.
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

    // 2) Disk fallback: {project}/.github/automation/recommendations.json
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
/// - Spawns via `TaskSpawner` (canonical path for fix_content_article children)
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

/// Canonical construction path for `fix_content_article` children from a
/// content_review selection. Anything that reaches here was validated as
/// `fix_content_article` with a present `article_id`.
fn spawn_one(
    conn: &Connection,
    parent: &Task,
    proposal: &ContentReviewProposal,
) -> Result<Task> {
    // Aligned with validate_proposals: only fix_content_article is spawnable.
    if proposal.task_type != SUPPORTED_TASK_TYPE {
        return Err(Error::Validation(format!(
            "Unsupported proposal task_type '{}' (only {SUPPORTED_TASK_TYPE} is spawnable)",
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
        task_type: SUPPORTED_TASK_TYPE.to_string(),
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
mod tests;
