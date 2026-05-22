use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::models::task::{Task, TaskArtifact, TaskRunPolicy, TaskStatus};

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
            // Fallback 1: read from database
            crate::db::content_audit::get_latest_audit_artifact(conn, &parent_task.project_id, "ctr_audit_context")
                .ok()
                .flatten()
                .and_then(|v| serde_json::to_string(&v).ok())
        })
        .or_else(|| {
            // Fallback 2: read from automation dir (matches context.rs out_path)
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
    let mut skipped_existing = 0usize;

    for article in articles {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("");
        let file_ref = article["file"].as_str().unwrap_or("");

        // Skip articles with no detected issues
        let issues = &article["issues_detected"];
        let has_frontmatter_faq = article["has_frontmatter_faq"].as_bool().unwrap_or(false);

        // FAQ issue is only a source-level issue when frontmatter FAQ is missing.
        // If frontmatter FAQ exists but rendered schema is missing, that's a render-level
        // issue handled by the schema renderer task, not a per-article fix.
        let missing_source_faq =
            issues["missing_faq_schema"].as_bool().unwrap_or(false) && !has_frontmatter_faq;

        let has_issues = issues["file_not_found"].as_bool().unwrap_or(false)
            || issues["title_too_long"].as_bool().unwrap_or(false)
            || issues["meta_too_short"].as_bool().unwrap_or(false)
            || issues["snippet_suboptimal"].as_bool().unwrap_or(false)
            || missing_source_faq;

        if !has_issues {
            skipped_healthy += 1;
            continue;
        }

        if let Some(existing) =
            find_active_ctr_fix_task_for_article(conn, &parent_task.project_id, id)
        {
            skipped_existing += 1;
            log::info!(
                "[ctr_audit] Existing active CTR fix task {} for article {} is {:?}; not spawning duplicate",
                existing.id,
                id,
                existing.status
            );

            if matches!(
                existing.status,
                TaskStatus::Todo | TaskStatus::Queued | TaskStatus::InProgress
            ) {
                created_ids.push(existing.id);
            }

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
                log::warn!(
                    "[ctr_audit] Failed to serialize context for article {}: {}",
                    id,
                    e
                );
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

        let issue_signature = ctr_issue_signature(article);
        let idempotency_key = format!(
            "ctr_fix:article:{}:{}:{}",
            parent_task.project_id, id, issue_signature
        );

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_ctr_article".to_string(),
            title: Some(format!("CTR fix: {}", url_slug)),
            description: Some(format!("Apply CTR fixes to article {} ({})", id, url_slug)),
            priority: crate::models::task::Priority::Medium,
            run_policy: Some(TaskRunPolicy::AutoEnqueue),
            agent_policy: crate::models::task::AgentPolicy::Optional,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![artifact],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
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
                log::warn!(
                    "[ctr_audit] Failed to create fix task for article {}: {}",
                    id,
                    e
                );
            }
        }
    }

    // Record schema rendering compatibility warning if source FAQ exists but
    // rendered HTML lacks FAQPage JSON-LD. We do NOT spawn an agentic task
    // for this — the fix belongs in the target repo's parser/layout/head code.
    // See PAGESEEDS_REPO_INTEGRATION.md for the ownership contract.
    record_ctr_schema_warning(conn, parent_task, project_path);

    let total_scanned = articles.len();
    let spawned = created_ids.len();
    log::info!(
        "[ctr_audit] Spawner result: {} scanned, {} healthy skipped, {} existing skipped, {} fix task(s) returned",
        total_scanned,
        skipped_healthy,
        skipped_existing,
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

fn ctr_issue_signature(article: &serde_json::Value) -> String {
    let issues = &article["issues_detected"];
    let mut parts = Vec::new();

    for issue in [
        "file_not_found",
        "title_too_long",
        "meta_too_short",
        "snippet_suboptimal",
        "missing_faq_schema",
    ] {
        if issues[issue].as_bool().unwrap_or(false) {
            parts.push(issue);
        }
    }

    let issue_set = if parts.is_empty() {
        "no_issues".to_string()
    } else {
        parts.join("+")
    };

    match article["content_hash"]
        .as_str()
        .filter(|hash| !hash.is_empty())
    {
        Some(hash) => format!("{}:{}", hash, issue_set),
        None => issue_set,
    }
}

fn find_active_ctr_fix_task_for_article(
    conn: &rusqlite::Connection,
    project_id: &str,
    article_id: i64,
) -> Option<Task> {
    let mut matches: Vec<Task> = crate::engine::task_store::list_tasks(conn, project_id)
        .ok()?
        .into_iter()
        .filter(|task| task.task_type == "fix_ctr_article")
        .filter(|task| {
            matches!(
                task.status,
                TaskStatus::Todo | TaskStatus::Queued | TaskStatus::InProgress | TaskStatus::Review
            )
        })
        .filter(|task| ctr_fix_task_article_id(task) == Some(article_id))
        .collect();

    matches.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    matches.into_iter().next()
}

fn ctr_fix_task_article_id(task: &Task) -> Option<i64> {
    task.artifacts
        .iter()
        .find(|artifact| artifact.key == "ctr_context")
        .and_then(|artifact| artifact.content.as_ref())
        .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok())
        .and_then(|value| {
            value
                .get("articles")?
                .as_array()?
                .first()?
                .get("id")?
                .as_i64()
        })
}

/// Detect articles where source FAQ exists but rendered HTML has no FAQPage JSON-LD.
/// Instead of spawning an agentic fix task (which cannot reliably edit arbitrary
/// target-repo framework code), store the result as a warning artifact on the
/// parent task so the UI can surface a compatibility message.
fn record_ctr_schema_warning(conn: &rusqlite::Connection, parent_task: &Task, project_path: &str) {
    let result =
        crate::engine::exec::ctr_audit::exec_ctr_schema_detect(parent_task, project_path, conn);

    if !result.success {
        log::warn!("[ctr_schema] Schema detection failed: {}", result.message);
        return;
    }

    let affected: Vec<serde_json::Value> = match result.output.as_deref() {
        Some(json) => serde_json::from_str(json).unwrap_or_default(),
        None => return,
    };

    if affected.is_empty() {
        return;
    }

    let warning = serde_json::json!({
        "level": "warning",
        "category": "repo_compatibility",
        "message": format!(
            "{} article(s) have source FAQ content but the rendered pages do not emit FAQPage JSON-LD. \
             The target repo's schema rendering code needs to be updated. \
             See PAGESEEDS_REPO_INTEGRATION.md for setup instructions.",
            affected.len()
        ),
        "affected_count": affected.len(),
        "affected_sample": affected,
        "doc_ref": "PAGESEEDS_REPO_INTEGRATION.md",
    });

    let artifact = TaskArtifact {
        key: "ctr_schema_compatibility_warning".to_string(),
        path: None,
        artifact_type: Some("warning".to_string()),
        source: Some("ctr_schema_detect".to_string()),
        content: Some(warning.to_string()),
    };

    if let Err(e) =
        crate::engine::task_store::append_task_artifact(conn, &parent_task.id, &artifact)
    {
        log::warn!("[ctr_schema] Failed to append warning artifact: {}", e);
        return;
    }

    log::warn!(
        "[ctr_schema] Compatibility warning: {} article(s) missing rendered FAQPage JSON-LD. \
         Stored on parent task {} as ctr_schema_compatibility_warning.",
        affected.len(),
        parent_task.id
    );
}
