use rusqlite::Connection;

use crate::engine::task_store;
use crate::models::task::Task;

use super::topic_health::find_written_article_file;
use super::PostTaskContext;

// ─── Content Outcome Review Spawning (issue #23) ─────────────────────────────

/// Days after a content change before its outcome is reviewed. Uniform 30d
/// across write_article / fix_content_article / consolidate_cluster (decision
/// recorded in issue #23).
const CONTENT_OUTCOME_REVIEW_DELAY_DAYS: i64 = 30;

/// Spawn a +30d `content_outcome_review` follow-up after a successful
/// write_article / fix_content_article / consolidate_cluster task.
///
/// Carries the article slug and a baseline snapshot of clicks/impressions/
/// position (from the article's stored GSC metadata; empty baseline is fine
/// for brand-new articles) as the `content_outcome_target` artifact consumed
/// by `exec::outcome_review::exec_content_outcome_compare`.
/// Returns the spawned task ID, or None when no slug could be resolved.
pub(crate) fn spawn_content_outcome_review(ctx: &PostTaskContext<'_>) -> Option<String> {
    let slug = outcome_review_slug(ctx)?;
    if slug.is_empty() {
        return None;
    }

    let baseline = outcome_baseline_metrics(ctx.conn, &ctx.task.project_id, &slug);
    let anchor_date = chrono::Utc::now().to_rfc3339();

    let target_artifact = crate::models::task::TaskArtifact {
        key: "content_outcome_target".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("post_actions".to_string()),
        content: Some(
            serde_json::json!({
                "slug": slug,
                "parent_task_type": ctx.task.task_type,
                "parent_task_id": ctx.task.id,
                "anchor_date": anchor_date,
                "baseline": {
                    "clicks": baseline.0,
                    "impressions": baseline.1,
                    "position": baseline.2,
                    "source": baseline.3,
                },
            })
            .to_string(),
        ),
    };

    let idempotency_key = format!(
        "content_outcome_review:{}:{}:{}",
        ctx.task.project_id, ctx.task.id, slug
    );
    let not_before = (chrono::Utc::now()
        + chrono::Duration::days(CONTENT_OUTCOME_REVIEW_DELAY_DAYS))
    .to_rfc3339();
    let spec = crate::engine::spawner::TaskSpec {
        project_id: ctx.task.project_id.clone(),
        task_type: "content_outcome_review".to_string(),
        title: Some(format!("Content outcome review: {}", slug)),
        description: Some(format!(
            "Compare GSC snapshot windows for '{}' {} days after {} (parent: {}).",
            slug, CONTENT_OUTCOME_REVIEW_DELAY_DAYS, ctx.task.task_type, ctx.task.id
        )),
        priority: crate::models::task::Priority::Medium,
        run_policy: Some(crate::models::task::TaskRunPolicy::UserEnqueue),
        agent_policy: crate::models::task::AgentPolicy::None,
        depends_on: vec![ctx.task.id.clone()],
        artifacts: vec![target_artifact],
        idempotency_key: Some(idempotency_key),
        not_before: Some(not_before),
        ..Default::default()
    };

    match crate::engine::spawner::TaskSpawner::spawn(ctx.conn, spec) {
        Ok(task) => {
            log::info!(
                "[post_actions] Spawned content_outcome_review {} for slug '{}' (parent {})",
                task.id,
                slug,
                ctx.task.id
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!(
                "[post_actions] Failed to spawn content_outcome_review for '{}': {}",
                slug,
                e
            );
            None
        }
    }
}

/// Resolve the article slug whose outcome should be reviewed.
///
/// - consolidate_cluster: the keeper slug from the merge plan artifacts.
/// - write_article / fix_content_article: slug of the written/modified file.
fn outcome_review_slug(ctx: &PostTaskContext<'_>) -> Option<String> {
    if ctx.task.task_type == "consolidate_cluster" {
        return merge_keeper_slug(ctx.task);
    }

    let file = find_written_article_file(ctx)?;
    // slug_from_filename already strips numeric prefixes, so normalize_url_slug
    // composes cleanly on top of it.
    let slug = crate::content::slug::normalize_url_slug(&crate::content::ops::slug_from_filename(
        &file,
    ));
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

/// Extract the keeper slug from a consolidate_cluster task's artifacts.
/// Primary source: the `merge_load_plan` step artifact (the recommendation
/// JSON with `keep_url`). Fallback: `cannibalization_strategy` matched by the
/// cluster id in the task title ("Merge cluster: <id>").
fn merge_keeper_slug(task: &Task) -> Option<String> {
    let slug_from_keep_url = |keep_url: &str| -> Option<String> {
        let slug = crate::content::slug::extract_slug_from_url(keep_url);
        if slug.is_empty() {
            None
        } else {
            Some(slug)
        }
    };

    if let Some(plan) = task
        .artifacts
        .iter()
        .find(|a| a.key == "merge_load_plan")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
    {
        if let Some(keep_url) = plan["keep_url"].as_str() {
            if let Some(slug) = slug_from_keep_url(keep_url) {
                return Some(slug);
            }
        }
    }

    let cluster_id = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .unwrap_or("")
        .trim();
    if cluster_id.is_empty() {
        return None;
    }
    let strategy = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())?;
    let rec = strategy["merge_recommendations"].as_array()?.iter().find(|r| {
        r["cluster_id"].as_str().unwrap_or("") == cluster_id
    })?;
    slug_from_keep_url(rec["keep_url"].as_str().unwrap_or(""))
}

/// Snapshot the article's current GSC metrics as the outcome baseline.
///
/// Reads the `gsc` namespace from `article_metadata` (the 90-day aggregate
/// written by the GSC sync). Returns (clicks, impressions, position, source).
/// A zeroed baseline with source "none" is fine for brand-new articles.
fn outcome_baseline_metrics(
    conn: &Connection,
    project_id: &str,
    slug: &str,
) -> (f64, f64, f64, &'static str) {
    let normalized = crate::content::slug::normalize_url_slug(slug);
    let articles = match task_store::list_articles(conn, project_id) {
        Ok(a) => a,
        Err(_) => return (0.0, 0.0, 0.0, "none"),
    };
    let article = articles.iter().find(|a| {
        crate::content::slug::normalize_url_slug(&a.url_slug) == normalized
    });
    let article_id = match article {
        Some(a) => a.id,
        None => return (0.0, 0.0, 0.0, "none"),
    };

    let payload: Option<String> = conn
        .query_row(
            "SELECT payload FROM article_metadata
             WHERE project_id = ?1 AND article_id = ?2 AND namespace = 'gsc'",
            rusqlite::params![project_id, article_id],
            |row| row.get(0),
        )
        .ok();
    payload
        .and_then(|p| serde_json::from_str::<serde_json::Value>(&p).ok())
        .map(|v| {
            (
                v["clicks"].as_f64().unwrap_or(0.0),
                v["impressions"].as_f64().unwrap_or(0.0),
                v["avg_position"].as_f64().unwrap_or(0.0),
                "article_metadata_gsc",
            )
        })
        .unwrap_or((0.0, 0.0, 0.0, "none"))
}
