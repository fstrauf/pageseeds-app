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

/// Mark articles as `in_review` and sync the repo projection.
/// Used by content-review selection after the user picks fix tasks.
pub(crate) fn mark_articles_in_review(
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

/// Release an article's `in_review` flag when its fix task did not complete
/// successfully (soft-failed verification or cancellation). Resets the review
/// state to the pre-review default (NULL, matching how articles ship before any
/// review) so the article becomes selectable by `select_priority_articles`
/// again. `last_reviewed_at` and `review_count` are deliberately untouched —
/// no review happened.
///
/// Only releases when the article is still `in_review`, so a `reviewed` state
/// written by another successful fix is never clobbered.
pub(crate) fn release_fix_content_article_in_review(
    conn: &Connection,
    task: &Task,
    project_path: &str,
) -> crate::error::Result<Option<i64>> {
    let Some(article_id) = fix_content_article_id(task) else {
        return Ok(None);
    };

    let rows = conn.execute(
        "UPDATE articles
         SET review_status = NULL, review_started_at = NULL
         WHERE id = ?1 AND project_id = ?2 AND review_status = 'in_review'",
        rusqlite::params![article_id, &task.project_id],
    )?;

    if rows > 0 {
        sync_article_review_state_to_repo(conn, &task.project_id, project_path)?;
        Ok(Some(article_id))
    } else {
        Ok(None)
    }
}

