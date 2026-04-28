use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::{ExecutionMode, Task, TaskArtifact};

/// Spawn per-article `fix_ctr_article` tasks based on the ctr_build_context artifact.
///
/// Reads the `ctr_build_context` artifact from the parent task, iterates over
/// articles with detected issues, and creates one `fix_ctr_article` task per
/// article. Each task receives a `ctr_context` artifact containing the single
/// article's data so the task can perform its own analysis + fix + verification.
pub(crate) fn create_ctr_fix_tasks(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Try to find the context artifact on the parent task first
    let context_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_build_context")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: read from automation dir (matches context.rs out_path)
            let fallback_path = paths.automation_dir.join("ctr_audit_context.json");
            std::fs::read_to_string(&fallback_path).ok()
        })
        .unwrap_or_default();

    if context_json.is_empty() {
        log::warn!(
            "[ctr_audit] No ctr_build_context artifact found for task {}",
            parent_task.id
        );
        return Vec::new();
    }

    let context_doc: serde_json::Value = match serde_json::from_str(&context_json) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[ctr_audit] Failed to parse ctr_build_context JSON: {}", e);
            return Vec::new();
        }
    };

    let articles = match context_doc["articles"].as_array() {
        Some(arr) => arr,
        None => {
            log::warn!("[ctr_audit] ctr_build_context has no articles array");
            return Vec::new();
        }
    };

    let mut created_ids = Vec::new();
    let mut skipped_healthy = 0usize;

    for article in articles {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("");
        let file_ref = article["file"].as_str().unwrap_or("");

        // Skip articles with no detected issues
        let issues = &article["issues_detected"];
        let has_issues = issues["file_not_found"].as_bool().unwrap_or(false)
            || issues["title_too_long"].as_bool().unwrap_or(false)
            || issues["meta_too_short"].as_bool().unwrap_or(false)
            || issues["snippet_suboptimal"].as_bool().unwrap_or(false)
            || issues["missing_faq_schema"].as_bool().unwrap_or(false);

        if !has_issues {
            skipped_healthy += 1;
            continue;
        }

        // Build single-article context
        let single_context = serde_json::json!({
            "total_articles": 1,
            "articles": [article],
        });

        let context_str = match serde_json::to_string(&single_context) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ctr_audit] Failed to serialize context for article {}: {}", id, e);
                continue;
            }
        };

        let artifact = TaskArtifact {
            key: "ctr_context".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("ctr_audit".to_string()),
            content: Some(context_str),
        };

        let idempotency_key = format!(
            "ctr_fix:article:{}:{}:{}",
            parent_task.project_id, id, parent_task.id
        );

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_ctr_article".to_string(),
            title: Some(format!("CTR fix: {}", url_slug)),
            description: Some(format!(
                "Apply CTR fixes to article {} ({})",
                id, url_slug
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
                    task.id, id, file_ref
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!("[ctr_audit] Failed to create fix task for article {}: {}", id, e);
            }
        }
    }

    // Create a schema renderer task if any articles need it
    if let Some(task_id) = create_ctr_schema_renderer_task(conn, parent_task, project_path) {
        created_ids.push(task_id);
    }

    let total_scanned = articles.len();
    let spawned = created_ids.len();
    log::info!(
        "[ctr_audit] Spawner result: {} scanned, {} healthy skipped, {} fix task(s) created",
        total_scanned,
        skipped_healthy,
        spawned
    );

    if spawned == 0 && total_scanned > 0 {
        log::warn!(
            "[ctr_audit] CTR audit found {} article(s) but created 0 fix tasks. \
             All may be healthy, or the handoff may be broken.",
            total_scanned
        );
    }

    created_ids
}

/// Spawn a `fix_ctr_schema_renderer` task if rendered audits show missing FAQPage JSON-LD.
fn create_ctr_schema_renderer_task(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
) -> Option<String> {
    let result = crate::engine::exec::ctr_audit::exec_ctr_schema_detect(
        parent_task, project_path, conn,
    );

    if !result.success {
        log::warn!("[ctr_schema] Schema detection failed: {}", result.message);
        return None;
    }

    let affected: Vec<serde_json::Value> = match result.output.as_deref() {
        Some(json) => serde_json::from_str(json).unwrap_or_default(),
        None => return None,
    };

    if affected.is_empty() {
        return None;
    }

    let detection_json = match serde_json::to_string_pretty(&affected) {
        Ok(j) => j,
        Err(e) => {
            log::warn!("[ctr_schema] Failed to serialize detection: {}", e);
            return None;
        }
    };

    let artifact = TaskArtifact {
        key: "ctr_schema_detection".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("ctr_schema_detect".to_string()),
        content: Some(detection_json),
    };

    let idempotency_key = format!(
        "ctr_fix:schema_renderer:{}:{}",
        parent_task.project_id, parent_task.id
    );

    let spec = TaskSpec {
        project_id: parent_task.project_id.clone(),
        task_type: "fix_ctr_schema_renderer".to_string(),
        title: Some(format!(
            "Fix schema renderer: {} article(s) missing FAQPage JSON-LD",
            affected.len()
        )),
        description: Some(format!(
            "Detected {} article(s) with source FAQ content but no rendered FAQPage JSON-LD. \
             The target repo's schema rendering code needs to be fixed.",
            affected.len()
        )),
        priority: crate::models::task::Priority::High,
        execution_mode: Some(ExecutionMode::Manual),
        agent_policy: crate::models::task::AgentPolicy::Optional,
        depends_on: vec![parent_task.id.clone()],
        artifacts: vec![artifact],
        idempotency_key: Some(idempotency_key),
        ..Default::default()
    };

    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[ctr_schema] Created schema renderer fix task {} ({} articles)",
                task.id, affected.len()
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!("[ctr_schema] Failed to create schema renderer fix task: {}", e);
            None
        }
    }
}
