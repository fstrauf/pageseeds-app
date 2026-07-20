use crate::engine::project_paths::ProjectPaths;
use crate::models::task::{Task, TaskArtifact};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Leniently coerce a JSON value to an article id: accepts numbers and
/// numeric strings (historical artifacts were written both ways).
fn value_as_i64(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::String(id) => {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse::<i64>().ok()
            }
        }
        serde_json::Value::Number(id) => id.as_i64(),
        _ => None,
    }
}

pub(crate) fn recommendation_article_id(article: &serde_json::Value) -> Option<i64> {
    article.get("article_id").and_then(value_as_i64)
}

fn deserialize_lenient_i64<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value_as_i64(&value).unwrap_or(0))
}

/// Typed payload of the `recommendations_{article_id}` task artifact — the
/// single writer/reader contract for per-article fix recommendations.
///
/// Consumers also read historical artifacts stored in the DB, so every field
/// tolerates absence (`#[serde(default)]`) and `article_id` accepts both
/// numbers and numeric strings, mirroring the loose `serde_json::Value`
/// indexing previously used in `fix_context`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ArticleRecommendationPayload {
    #[serde(default, deserialize_with = "deserialize_lenient_i64")]
    pub article_id: i64,
    #[serde(default)]
    pub article_title: String,
    #[serde(default)]
    pub article_file: String,
    #[serde(default)]
    pub url_slug: String,
    #[serde(default)]
    pub target_keyword: Option<String>,
    /// Kept as raw JSON values: historical suggestions vary in shape and are
    /// passed through verbatim to the fix pipeline's generate step, which
    /// parses them tolerantly into `ReviewSuggestion`.
    #[serde(default)]
    pub suggestions: Vec<serde_json::Value>,
}

/// Build the single `recommendations_{article_id}` artifact for a fix task.
/// Owns the artifact key format so all writers agree on it.
pub(crate) fn recommendation_artifact(
    payload: &ArticleRecommendationPayload,
    source: &str,
) -> TaskArtifact {
    TaskArtifact {
        key: format!("recommendations_{}", payload.article_id),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(source.to_string()),
        content: Some(serde_json::to_string(payload).unwrap_or_default()),
    }
}

pub(crate) fn fix_content_article_id(task: &Task) -> Option<i64> {
    task.artifacts.iter().find_map(|artifact| {
        artifact
            .content
            .as_deref()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok())
            .and_then(|article| recommendation_article_id(&article))
            .or_else(|| {
                artifact
                    .key
                    .strip_prefix("recommendations_")
                    .and_then(|suffix| suffix.parse::<i64>().ok())
            })
    })
}

fn sync_article_review_state_to_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> crate::error::Result<()> {
    crate::content::article_index::export_projection(
        conn,
        project_id,
        std::path::Path::new(project_path),
    )?;
    Ok(())
}

fn mark_articles_in_review(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    article_ids: &[i64],
) -> crate::error::Result<usize> {
    if article_ids.is_empty() {
        return Ok(0);
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut updated = 0usize;
    for article_id in article_ids {
        updated += conn.execute(
            "UPDATE articles
             SET review_status = 'in_review', review_started_at = ?1
             WHERE id = ?2 AND project_id = ?3",
            rusqlite::params![&now, article_id, project_id],
        )?;
    }

    if updated > 0 {
        sync_article_review_state_to_repo(conn, project_id, project_path)?;
    }

    Ok(updated)
}

pub(crate) fn mark_fix_content_article_reviewed(
    conn: &Connection,
    task: &Task,
    project_path: &str,
) -> crate::error::Result<Option<i64>> {
    let Some(article_id) = fix_content_article_id(task) else {
        return Ok(None);
    };

    let now = chrono::Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE articles
         SET review_status = 'reviewed',
             review_started_at = NULL,
             last_reviewed_at = ?1,
             review_count = COALESCE(review_count, 0) + 1
         WHERE id = ?2 AND project_id = ?3",
        rusqlite::params![&now, article_id, &task.project_id],
    )?;

    if rows > 0 {
        sync_article_review_state_to_repo(conn, &task.project_id, project_path)?;
        Ok(Some(article_id))
    } else {
        Ok(None)
    }
}

/// After a successful content review, create individual `fix_content_article` tasks
/// for each article in recommendations.json.
///
/// Skips if recommendations.json is absent (review found nothing).
pub(crate) fn create_fix_content_article_tasks(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy, TaskStatus};
    use std::collections::HashSet;

    let paths = ProjectPaths::from_path(project_path);
    let rec_path = paths.automation_dir.join("recommendations.json");

    let rec_str = match std::fs::read_to_string(&rec_path) {
        Ok(s) => s,
        Err(_) => {
            log::info!(
                "[create_apply_task] recommendations.json not found — no apply tasks created"
            );
            return Vec::new();
        }
    };
    let rec: serde_json::Value = match serde_json::from_str(&rec_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[create_apply_task] failed to parse recommendations.json: {}",
                e
            );
            return Vec::new();
        }
    };

    let articles = match rec["articles"].as_array() {
        Some(a) if !a.is_empty() => a,
        _ => {
            log::info!("[create_apply_task] no articles in recommendations — skipping");
            return Vec::new();
        }
    };

    let mut created_task_ids = Vec::new();
    let mut seen_article_ids = HashSet::new();
    let mut in_review_article_ids = Vec::new();

    for article in articles {
        let Some(article_id) = recommendation_article_id(article) else {
            let article_title = article["article_title"].as_str().unwrap_or("article");
            log::warn!(
                "[create_apply_task] skipping article '{}' with missing/invalid article_id",
                article_title
            );
            continue;
        };

        let article_title = article["article_title"].as_str().unwrap_or("article");
        let article_file = article["article_file"].as_str().unwrap_or("");

        if !seen_article_ids.insert(article_id.clone()) {
            log::warn!(
                "[create_apply_task] skipping duplicate recommendation for article '{}' ({})",
                article_title,
                article_id
            );
            continue;
        }

        // Store the full single-article recommendations so the fix task is self-contained.
        // This matches the CTR pattern where follow-up tasks carry their full context.
        let payload = ArticleRecommendationPayload {
            article_id,
            article_title: article_title.to_string(),
            article_file: article_file.to_string(),
            url_slug: article["url_slug"].as_str().unwrap_or("").to_string(),
            target_keyword: Some(article["target_keyword"].as_str().unwrap_or("").to_string()),
            suggestions: article["suggestions"].as_array().cloned().unwrap_or_default(),
        };
        let article_id_str = article_id.to_string();

        let title = format!("Fix: {}", article_title);

        // Create individual artifact for this article
        let artifact = recommendation_artifact(&payload, "content_review");

        // Idempotency key per article: fix_content_article:{project_id}:{article_id}
        let idempotency_key = format!(
            "fix_content_article:{}:{}",
            parent_task.project_id, article_id_str
        );

        // Calculate priority based on issue count
        let issue_count = article["suggestions"]
            .as_array()
            .map(|s| s.len())
            .unwrap_or(0);
        let priority = if issue_count >= 5 {
            Priority::High
        } else if issue_count >= 2 {
            Priority::Medium
        } else {
            Priority::Low
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_content_article".to_string(),
            title: Some(title),
            description: Some(format!(
                "Apply SEO recommendations to '{}' ({} issue{}). \
                 File: {}",
                article_title,
                issue_count,
                if issue_count == 1 { "" } else { "s" },
                article_file
            )),
            phase: Some("implementation".to_string()),
            run_policy: Some(TaskRunPolicy::AutoEnqueue),
            priority,
            agent_policy: AgentPolicy::Required,
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::Cooldown { days: 30 }),
            artifacts: vec![artifact],
            depends_on: vec![],
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[create_apply_task] created {} for article '{}' ({} issues)",
                    task.id,
                    article_title,
                    issue_count
                );
                created_task_ids.push(task.id);
                in_review_article_ids.push(article_id);
            }
            Err(e) => {
                log::warn!(
                    "[create_apply_task] failed to create task for article '{}': {}",
                    article_title,
                    e
                );
            }
        }
    }

    if let Err(e) = mark_articles_in_review(
        conn,
        &parent_task.project_id,
        project_path,
        &in_review_article_ids,
    ) {
        log::warn!(
            "[create_apply_task] failed to mark articles in_review: {}",
            e
        );
    }

    log::info!(
        "[create_apply_task] created {} individual fix task(s) from content review",
        created_task_ids.len()
    );

    created_task_ids
}
