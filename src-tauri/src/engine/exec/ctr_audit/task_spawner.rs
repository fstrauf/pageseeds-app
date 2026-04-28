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

    // Pre-load articles.json for field enrichment (file / target_keyword fallback)
    let articles_lookup: std::collections::HashMap<i64, (String, String, String)> =
        std::fs::read_to_string(&paths.articles_json)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("articles").cloned())
            .and_then(|a| a.as_array().cloned())
            .map(|arr| {
                arr.into_iter()
                    .filter_map(|article| {
                        let id = article.get("id")?.as_i64()?;
                        let file = article.get("file")?.as_str()?.to_string();
                        let url_slug = article.get("url_slug")?.as_str()?.to_string();
                        let target_keyword = article.get("target_keyword")?.as_str()?.to_string();
                        Some((id, (file, url_slug, target_keyword)))
                    })
                    .collect()
            })
            .unwrap_or_default();

    let mut created_ids = Vec::new();
    let mut schema_renderer_needed = false;

    for rec in agent_output.recommendations {
        if rec.fixes.is_empty() {
            continue;
        }

        // Enrich missing fields from trusted article context.
        let mut rec = rec;
        if let Some((article_file, article_slug, article_keyword)) = articles_lookup.get(&rec.article_id) {
            if rec.file.is_empty() {
                rec.file = article_file.clone();
                log::info!("[ctr_audit] Enriched missing 'file' for article {} from articles.json: {}", rec.article_id, rec.file);
            }
            if rec.url_slug.is_empty() {
                rec.url_slug = article_slug.clone();
                log::info!("[ctr_audit] Enriched missing 'url_slug' for article {} from articles.json: {}", rec.article_id, rec.url_slug);
            }
            if rec.target_keyword.is_empty() {
                rec.target_keyword = article_keyword.clone();
                log::info!("[ctr_audit] Enriched missing 'target_keyword' for article {} from articles.json: {}", rec.article_id, rec.target_keyword);
            }
        }

        // Phase 1 contract enforcement: file and target_keyword are required.
        if rec.file.is_empty() {
            log::warn!(
                "[ctr_audit] Skipping recommendation for article {}: missing required 'file' field and no enrichment available",
                rec.article_id
            );
            continue;
        }
        if rec.target_keyword.is_empty() {
            log::warn!(
                "[ctr_audit] Skipping recommendation for article {} ({}): missing required 'target_keyword' field and no enrichment available",
                rec.article_id,
                rec.file
            );
            continue;
        }

        let article_id = rec.article_id;
        let url_slug = rec.url_slug.clone();
        let file = rec.file.clone();

        // Phase 5: Distinguish FAQ content fixes from schema renderer fixes.
        // If the article has a FaqSchema fix but rendered audit shows no FAQPage JSON-LD,
        // remove the FaqSchema fix from the article task and flag for schema renderer task.
        let mut fixes = rec.fixes.clone();
        if fixes.iter().any(|f| matches!(f.fix_type, crate::models::ctr::CtrFixType::FaqSchema)) {
            if let Ok(Some(audit)) = crate::db::get_ctr_rendered_audit(conn, &parent_task.project_id, article_id) {
                if !audit.has_rendered_faq_page && audit.rendered_faq_question_count == 0 {
                    log::info!(
                        "[ctr_audit] Article {} has source FAQ but no rendered FAQPage JSON-LD — routing to schema renderer task",
                        article_id
                    );
                    fixes.retain(|f| !matches!(f.fix_type, crate::models::ctr::CtrFixType::FaqSchema));
                    schema_renderer_needed = true;
                }
            }
        }

        if fixes.is_empty() {
            log::info!("[ctr_audit] All fixes for article {} were routed to site-level tasks — skipping article task", article_id);
            continue;
        }

        let single_rec = CtrRecommendation {
            article_id: rec.article_id,
            url_slug: rec.url_slug,
            file: rec.file,
            priority: rec.priority,
            expected_ctr_improvement: rec.expected_ctr_improvement,
            target_keyword: rec.target_keyword,
            fixes,
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

    // Create a schema renderer task if any articles need it
    if schema_renderer_needed {
        if let Some(task_id) = create_ctr_schema_renderer_task(conn, parent_task, project_path) {
            created_ids.push(task_id);
        }
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
