//! Follow-up task creation from user-selected Clarity findings.
//!
//! Selection-command flow (task lifecycle contract): the task-drawer picker
//! sends the findings the user selected; this function resolves each finding's
//! URL to a project article, spawns a self-contained `fix_content_article`
//! task per resolvable finding, skips the rest with an explanation, and marks
//! the parent task done.
//!
//! Every spawned task carries a `recommendations_{article_id}` artifact in the
//! exact shape the fix pipeline's context step consumes
//! (`engine::exec::content::fix_context`), so no task can fail on a missing
//! `article_id`.

use rusqlite::Connection;

use crate::engine::exec::content::{recommendation_artifact, ArticleRecommendationPayload};
use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::clarity::{
    ClarityFindingPayload, ClaritySkippedFinding, ClarityTaskCreationResult,
};
use crate::models::content_review::ReviewSuggestion;
use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy, TaskStatus};

/// Create follow-up tasks from user selections in the task drawer.
///
/// Validates the parent task, resolves each finding URL against the project's
/// valid link targets, spawns `fix_content_article` tasks via `TaskSpawner`
/// with idempotency keys (`clarity_fix:{project}:{slug}:{issue_type}`), marks
/// the parent done, and returns created tasks plus per-finding skip reasons.
pub fn spawn_tasks_from_selection(
    db: &Connection,
    parent_task_id: &str,
    findings: &[ClarityFindingPayload],
) -> Result<ClarityTaskCreationResult> {
    if findings.is_empty() {
        return Err(Error::Validation("No findings selected".to_string()));
    }

    let parent_task = task_store::get_task(db, parent_task_id)?;
    if parent_task.task_type != "investigate_clarity" && parent_task.task_type != "clarity_analytics"
    {
        return Err(Error::Validation(format!(
            "Parent task is not a Clarity investigation (got '{}')",
            parent_task.task_type
        )));
    }

    let project = task_store::get_project(db, &parent_task.project_id)?;
    let valid_targets =
        task_store::load_valid_link_targets(db, &parent_task.project_id, &project.path)?;
    let articles = task_store::list_articles(db, &parent_task.project_id)?;
    let articles_by_slug: std::collections::HashMap<String, _> = articles
        .into_iter()
        .map(|a| (crate::content::slug::normalize_url_slug(&a.url_slug), a))
        .collect();

    let mut created_tasks = Vec::new();
    let mut skipped = Vec::new();

    for finding in findings {
        let slug = crate::content::slug::extract_slug_from_url(&finding.url);
        let article = crate::content::slug::resolve_slug(&slug, &valid_targets)
            .and_then(|resolved| articles_by_slug.get(&resolved));

        let article = match article {
            Some(a) => a,
            None => {
                skipped.push(ClaritySkippedFinding {
                    issue_type: finding.issue_type.clone(),
                    url: finding.url.clone(),
                    reason: "URL does not resolve to an article in this project \
                             (external, utility, or non-content page)"
                        .to_string(),
                });
                continue;
            }
        };

        // Self-contained single-article recommendation in the shared
        // `recommendations_{article_id}` contract, so the fix pipeline's
        // context step can consume it directly.
        let suggestion = ReviewSuggestion {
            category: "clarity".to_string(),
            current: finding.evidence.clone(),
            proposed: finding.recommendation.clone(),
            reason: format!(
                "Clarity '{}' (severity: {}). Dashboard: {}",
                finding.issue_type, finding.severity, finding.clarity_dashboard_url
            ),
            priority: Some(finding.severity.clone()),
        };
        let payload = ArticleRecommendationPayload {
            article_id: article.id,
            article_title: article.title.clone(),
            article_file: article.file.clone(),
            url_slug: article.url_slug.clone(),
            target_keyword: article.target_keyword.clone(),
            suggestions: vec![serde_json::to_value(&suggestion).unwrap_or_default()],
        };
        let artifact = recommendation_artifact(&payload, &parent_task.task_type);

        let normalized_slug = crate::content::slug::normalize_url_slug(&article.url_slug);
        let idempotency_key = format!(
            "clarity_fix:{}:{}:{}",
            parent_task.project_id, normalized_slug, finding.issue_type
        );

        let priority = match finding.severity.as_str() {
            "high" => Priority::High,
            "medium" => Priority::Medium,
            _ => Priority::Low,
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_content_article".to_string(),
            title: Some(format!("Fix: {} ({})", article.title, finding.issue_type)),
            description: Some(format!(
                "Clarity finding on {}: {}\n\nRecommendation: {}\n\nDashboard: {}",
                finding.url,
                finding.evidence,
                finding.recommendation,
                finding.clarity_dashboard_url
            )),
            run_policy: Some(TaskRunPolicy::UserEnqueue),
            priority,
            agent_policy: AgentPolicy::Required,
            depends_on: vec![parent_task_id.to_string()],
            artifacts: vec![artifact],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
            ..Default::default()
        };

        match TaskSpawner::spawn(db, spec) {
            Ok(task) => created_tasks.push(task),
            Err(e) => {
                log::warn!(
                    "[clarity_follow_up] failed to create task for '{}': {}",
                    finding.url,
                    e
                );
                skipped.push(ClaritySkippedFinding {
                    issue_type: finding.issue_type.clone(),
                    url: finding.url.clone(),
                    reason: format!("Failed to create task: {}", e),
                });
            }
        }
    }

    task_store::update_task_status(db, parent_task_id, TaskStatus::Done)?;

    Ok(ClarityTaskCreationResult {
        created_tasks,
        skipped,
    })
}
