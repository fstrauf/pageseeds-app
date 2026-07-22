use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;
use rusqlite::Connection;

/// Native Rust scan for `cluster_and_link_scan` step.
///
/// Reads articles.json from the automation dir, resolves the content directory,
/// and calls `content::linking::scan_links()`.  Returns the scan result as JSON
/// so the downstream agentic step has concrete link-graph data to work with.
pub(crate) fn exec_cluster_link_scan(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // --- Cache check: if link_scan.json exists and is < 1 hour old, reuse it ---
    let scan_path = paths.automation_dir.join("link_scan.json");
    if let Ok(metadata) = std::fs::metadata(&scan_path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                if elapsed.as_secs() < 3600 {
                    if let Ok(cached) = std::fs::read_to_string(&scan_path) {
                        log::info!(
                            "[cluster_link_scan] using cached link_scan.json ({} min old)",
                            elapsed.as_secs() / 60
                        );
                        // Try to extract summary stats from cached JSON for the message
                        let summary =
                            serde_json::from_str::<serde_json::Value>(&cached)
                                .map(|v| {
                                    format!(
                                "{} articles, {} internal links, {} orphans, {} zero-incoming",
                                v["total_articles"].as_u64().unwrap_or(0),
                                v["total_internal_links"].as_u64().unwrap_or(0),
                                v["orphan_ids"].as_array().map(|a| a.len()).unwrap_or(0),
                                v["zero_incoming_ids"].as_array().map(|a| a.len()).unwrap_or(0),
                            )
                                })
                                .unwrap_or_else(|_| "cached".to_string());
                        return crate::engine::workflows::StepResult {
                            success: true,
                            message: format!("Link scan complete (cached): {}", summary),
                            output: Some(cached),
                            artifact_key: None,
                        };
                    }
                }
            }
        }
    }

    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Failed to open app database: {}", e))
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a
            .into_iter()
            .filter(|a| !a.file.is_empty())
            .collect::<Vec<_>>(),
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Failed to load articles from DB: {}", e))
        }
    };

    if articles.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No articles in app index — nothing to scan".to_string(),
            output: Some(
                r#"{"total_articles":0,"total_internal_links":0,"orphan_ids":[],"profiles":[]}"#
                    .to_string(),
            ),
            artifact_key: None,
        };
    }

    // Locate the content directory via the standard locator (project override → heuristics)
    let resolution = crate::content::locator::resolve(&paths.repo_root, None);

    let content_dir = match resolution.selected {
        Some(d) => d,
        None => {
            return crate::engine::workflows::StepResult::fail("Could not locate content directory — set content_dir in project config"
                    .to_string())
        }
    };

    log::info!(
        "[cluster_link_scan] scanning {} articles in {}",
        articles.len(),
        content_dir.display()
    );

    match crate::content::linking::scan_links(&content_dir, &articles) {
        Ok(result) => {
            let json = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

            // Persist to link_scan.json so the downstream strategy step can read it.
            let scan_path = paths.automation_dir.join("link_scan.json");
            if let Err(e) = std::fs::write(&scan_path, &json) {
                log::warn!("[cluster_link_scan] failed to write link_scan.json: {}", e);
            }

            crate::engine::workflows::StepResult {
                success: true,
                message: format!(
                    "Link scan complete: {} articles, {} internal links, {} orphans, {} zero-incoming; {} unresolved links{}",
                    result.total_articles,
                    result.total_internal_links,
                    result.orphan_ids.len(),
                    result.zero_incoming_ids.len(),
                    result.unresolved_links.len(),
                    if result.unresolved_links.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " [{}]",
                            result
                                .unresolved_links
                                .iter()
                                .take(10)
                                .map(|u| format!("{} → {}", u.file, u.target))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                ),
                output: Some(json),
                artifact_key: None,
            }
        }
        Err(e) => crate::engine::workflows::StepResult::fail(format!("Link scan failed: {}", e)),
    }
}

/// Create a `cluster_and_link` follow-up task after a successful `write_article`.
///
/// De-duplicates: if an active `cluster_and_link` task already exists for this
/// project, no second task is created.
pub(crate) fn create_cluster_and_link_task(
    conn: &Connection,
    parent_task: &Task,
    _project_path: &str,
) -> Option<String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, Priority, TaskRunPolicy};

    let parent_title = parent_task
        .title
        .as_deref()
        .map_or(
            "new article",
            crate::engine::post_actions::strip_content_task_title_prefix,
        );

    let title = format!("Cluster and link: {}", parent_title);
    let description = format!(
        "Scan internal link graph and add missing hub-to-spoke, \
         spoke-to-hub, and cross-cluster links following the article: {}. \
         Depends on: {}",
        parent_title, parent_task.id,
    );

    // Use spawn with custom idempotency key to allow specific execution_mode and agent_policy
    let idempotency_key = format!("followup:{}:cluster_and_link:{}", parent_task.id, title);

    let spec = TaskSpec {
        project_id: parent_task.project_id.clone(),
        task_type: "cluster_and_link".to_string(),
        title: Some(title),
        description: Some(description),
        phase: Some("implementation".to_string()),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        artifacts: vec![],
        depends_on: vec![parent_task.id.clone()],
        ..Default::default()
    };

    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[cluster_link] spawned cluster_and_link task {} after write_article {}",
                task.id,
                parent_task.id
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!(
                "[cluster_link] failed to create cluster_and_link task: {}",
                e
            );
            None
        }
    }
}
