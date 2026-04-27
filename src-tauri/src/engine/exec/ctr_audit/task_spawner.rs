use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::ctr::CtrRecommendation;
use crate::models::task::{ExecutionMode, Task, TaskArtifact};

/// Spawn per-article `fix_ctr_article` tasks based on the recommendations artifact.
///
/// Looks for a `ctr_recommendations` artifact on the parent task; falls back
/// to reading `ctr_recommendations.json` from the automation directory.
/// Creates exactly one task per article with at least one fix recommendation.
pub(crate) fn create_ctr_fix_tasks(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Try to find the artifact on the parent task first
    let recommendation_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_recommendations")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: read from automation dir
            let fallback_path = paths.automation_dir.join("ctr_recommendations.json");
            std::fs::read_to_string(&fallback_path).ok()
        })
        .unwrap_or_default();

    if recommendation_json.is_empty() {
        log::warn!(
            "[ctr_audit] No ctr_recommendations artifact found for task {}",
            parent_task.id
        );
        return Vec::new();
    }

    let agent_output: crate::models::ctr::CtrAgentOutput = match serde_json::from_str(&recommendation_json) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[ctr_audit] Failed to parse recommendations JSON: {}", e);
            return Vec::new();
        }
    };

    let mut created_ids = Vec::new();

    for rec in agent_output.recommendations {
        if rec.fixes.is_empty() {
            continue;
        }

        let article_id = rec.article_id;
        let url_slug = rec.url_slug.clone();
        let file = rec.file.clone().unwrap_or_default();

        let single_rec = CtrRecommendation {
            article_id: rec.article_id,
            url_slug: rec.url_slug,
            file: rec.file,
            priority: rec.priority,
            expected_ctr_improvement: rec.expected_ctr_improvement,
            target_keyword: rec.target_keyword,
            fixes: rec.fixes,
        };

        let single_json = match serde_json::to_string(&single_rec) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ctr_audit] Failed to serialize single recommendation: {}", e);
                continue;
            }
        };

        let artifact = TaskArtifact {
            key: "ctr_recommendations".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("ctr_audit".to_string()),
            content: Some(single_json),
        };

        let idempotency_key = format!(
            "ctr_fix:article:{}:{}:{}",
            parent_task.project_id, article_id, parent_task.id
        );

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_ctr_article".to_string(),
            title: Some(format!("CTR fix: {}", url_slug)),
            description: Some(format!(
                "Apply CTR fixes to article {} ({})",
                article_id, url_slug
            )),
            priority: crate::models::task::Priority::Medium,
            execution_mode: Some(ExecutionMode::Automatic),
            agent_policy: crate::models::task::AgentPolicy::Optional,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![artifact],
            idempotency_key: Some(idempotency_key),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[ctr_audit] Created fix task {} (type: fix_ctr_article, article: {}, file: {})",
                    task.id, article_id, file
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!("[ctr_audit] Failed to create fix task for article {}: {}", article_id, e);
            }
        }
    }

    created_ids
}
