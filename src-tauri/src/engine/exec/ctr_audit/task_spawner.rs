use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::{ExecutionMode, Task, TaskArtifact};

/// Spawn up to 3 CTR fix tasks based on the recommendations artifact.
///
/// Looks for a `ctr_recommendations` artifact on the parent task; falls back
/// to reading `ctr_recommendations.json` from the automation directory.
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

    // Parse recommendations so we can filter per fix task type.
    // Each fix task only receives recommendations relevant to its specialty,
    // preventing the agent from hitting step limits by trying to fix everything.
    let full_recommendations: serde_json::Value = match serde_json::from_str(&recommendation_json) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[ctr_audit] Failed to parse recommendations JSON: {}", e);
            return Vec::new();
        }
    };

    let fix_task_configs = [
        (
            "fix_title_meta",
            format!("ctr_fix:title_meta:{}:{}", parent_task.project_id, parent_task.id),
            vec!["title_rewrite", "meta_description"],
        ),
        (
            "fix_faq_schema",
            format!("ctr_fix:faq:{}:{}", parent_task.project_id, parent_task.id),
            vec!["faq_schema"],
        ),
        (
            "fix_snippet_bait",
            format!("ctr_fix:snippet:{}:{}", parent_task.project_id, parent_task.id),
            vec!["snippet_bait"],
        ),
    ];

    let mut created_ids = Vec::new();

    for (task_type, idempotency_key, allowed_fix_types) in &fix_task_configs {
        let filtered = filter_recommendations_by_fix_type(&full_recommendations, allowed_fix_types);
        let filtered_json = match serde_json::to_string(&filtered) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ctr_audit] Failed to serialize filtered recommendations: {}", e);
                continue;
            }
        };

        // Skip creating this fix task if there are no relevant recommendations
        let rec_count = filtered["recommendations"].as_array().map(|r| r.len()).unwrap_or(0);
        if rec_count == 0 {
            log::info!(
                "[ctr_audit] No {} recommendations found — skipping fix task",
                task_type
            );
            continue;
        }

        let artifact = TaskArtifact {
            key: "ctr_recommendations".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("ctr_audit".to_string()),
            content: Some(filtered_json),
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: task_type.to_string(),
            title: Some(format!("CTR fix: {} ({} articles)", task_type, rec_count)),
            description: Some(format!(
                "Follow-up CTR fix task from {} (parent: {}) — {} articles to fix",
                task_type, parent_task.id, rec_count
            )),
            priority: crate::models::task::Priority::Medium,
            execution_mode: Some(ExecutionMode::Automatic),
            agent_policy: crate::models::task::AgentPolicy::Optional,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![artifact],
            idempotency_key: Some(idempotency_key.clone()),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!("[ctr_audit] Created fix task {} (type: {}, {} articles)", task.id, task_type, rec_count);
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!("[ctr_audit] Failed to create fix task {}: {}", task_type, e);
            }
        }
    }

    created_ids
}

/// Filter recommendations to only include fixes of the specified types.
/// Returns a new JSON object with only matching recommendations.
fn filter_recommendations_by_fix_type(
    full: &serde_json::Value,
    allowed_types: &[&str],
) -> serde_json::Value {
    let empty_arr = vec![];
    let recommendations = full["recommendations"].as_array().unwrap_or(&empty_arr);

    let filtered: Vec<serde_json::Value> = recommendations
        .iter()
        .filter_map(|rec| {
            let fixes = rec["fixes"].as_array()?;
            let matching_fixes: Vec<serde_json::Value> = fixes
                .iter()
                .filter(|f| {
                    f["type"]
                        .as_str()
                        .map(|t| allowed_types.contains(&t))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            if matching_fixes.is_empty() {
                return None;
            }

            let mut new_rec = rec.clone();
            new_rec["fixes"] = serde_json::Value::Array(matching_fixes);
            Some(new_rec)
        })
        .collect();

    serde_json::json!({
        "recommendations": filtered,
        "summary": format!("Filtered for fix types: {:?}", allowed_types),
    })
}
