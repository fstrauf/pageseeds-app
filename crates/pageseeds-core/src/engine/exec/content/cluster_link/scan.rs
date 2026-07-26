use crate::engine::project_paths::ProjectPaths;
use crate::models::task::{Task, TaskArtifact};
use rusqlite::Connection;

/// Native Rust scan for `cluster_and_link_scan` step.
///
/// Reads articles from the app DB, resolves the content directory, and calls
/// `content::linking::scan_links()`. Always rescans — a short-lived
/// `link_scan.json` cache would omit articles written after the last scan
/// (post-write inbound guarantee, issue #196).
pub(crate) fn exec_cluster_link_scan(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    let paths = ProjectPaths::from_path(project_path);

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

/// Build a `focus_slug` task artifact for write-spawned cluster_and_link tasks.
pub(crate) fn focus_slug_artifact(slug: &str) -> TaskArtifact {
    TaskArtifact {
        key: "focus_slug".to_string(),
        path: None,
        artifact_type: Some("text".to_string()),
        source: Some("write_spawn".to_string()),
        content: Some(slug.to_string()),
    }
}

/// Extract the url_slug of the article that triggered a write-spawned
/// `cluster_and_link` task.
///
/// Priority:
/// 1. Parent artifacts: `focus_slug` / `article_slug` / `url_slug`
/// 2. `File: ...` in parent description → stem via `slug_from_filename`
/// 3. Articles table: match file basename from description, else most recent
///    article for the project (prefer matching title when possible)
pub(crate) fn extract_focus_slug_from_parent(
    conn: &Connection,
    parent_task: &Task,
) -> Option<String> {
    for key in ["focus_slug", "article_slug", "url_slug"] {
        if let Some(slug) = parent_task
            .artifacts
            .iter()
            .find(|a| a.key == key)
            .and_then(|a| a.content.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(crate::content::slug::normalize_url_slug(slug));
        }
    }

    let desc = parent_task.description.as_deref().unwrap_or("");
    // Parse File: path without requiring a real repo root (stem only).
    if let Some(start) = desc.find("File: ").or_else(|| desc.find("File:")) {
        let prefix_len = if desc[start..].starts_with("File: ") {
            6
        } else {
            5
        };
        let rest = &desc[start + prefix_len..];
        let end = rest
            .find(" |")
            .or_else(|| rest.find('\n'))
            .unwrap_or(rest.len());
        let path_str = rest[..end].trim();
        if !path_str.is_empty() {
            let from_file = crate::content::ops::slug_from_filename(path_str);
            if !from_file.is_empty() {
                let normalized = crate::content::slug::normalize_url_slug(&from_file);
                // Prefer catalog url_slug when the file is registered.
                if let Ok(articles) =
                    crate::content::article_index::list_articles(conn, &parent_task.project_id)
                {
                    let basename = std::path::Path::new(path_str)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(path_str);
                    if let Some(a) = articles.iter().find(|a| {
                        a.file.ends_with(basename)
                            || std::path::Path::new(&a.file)
                                .file_name()
                                .and_then(|n| n.to_str())
                                == Some(basename)
                    }) {
                        if !a.url_slug.is_empty() {
                            return Some(crate::content::slug::normalize_url_slug(&a.url_slug));
                        }
                    }
                }
                return Some(normalized);
            }
        }
    }

    // Fallback: most recent article for the project (write just finished).
    if let Ok(articles) =
        crate::content::article_index::list_articles(conn, &parent_task.project_id)
    {
        let mut with_file: Vec<_> = articles
            .into_iter()
            .filter(|a| !a.file.is_empty() && !a.url_slug.is_empty())
            .collect();
        // Prefer title match against stripped parent title when present.
        if let Some(title) = parent_task.title.as_deref() {
            let stripped =
                crate::engine::post_actions::strip_content_task_title_prefix(title).to_lowercase();
            if let Some(a) = with_file
                .iter()
                .find(|a| a.title.to_lowercase() == stripped || a.title.to_lowercase().contains(&stripped))
            {
                return Some(crate::content::slug::normalize_url_slug(&a.url_slug));
            }
        }
        // Prefer most recently edited, then highest id (newest write often last).
        with_file.sort_by(|a, b| {
            b.last_edited_at
                .cmp(&a.last_edited_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        if let Some(a) = with_file.first() {
            return Some(crate::content::slug::normalize_url_slug(&a.url_slug));
        }
    }

    None
}

/// Create a `cluster_and_link` follow-up task after a successful `write_article`.
///
/// De-duplicates: if an active `cluster_and_link` task already exists for this
/// project, no second task is created (via spawner idempotency key).
/// Attaches a `focus_slug` artifact when the written article slug can be derived
/// so strategy prioritizes inbound links TO that article.
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

    let mut artifacts = Vec::new();
    if let Some(slug) = extract_focus_slug_from_parent(conn, parent_task) {
        log::info!(
            "[cluster_link] attaching focus_slug={} for cluster_and_link after {}",
            slug,
            parent_task.id
        );
        artifacts.push(focus_slug_artifact(&slug));
    }

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
        artifacts,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskReviewSurface, TaskRun, TaskRunPolicy, TaskStatus,
    };
    use std::fs;
    use uuid::Uuid;

    fn make_task(project_id: &str) -> Task {
        Task {
            id: format!("task-{}", Uuid::new_v4()),
            task_type: "write_article".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: Some("Write article: Focus Topic".to_string()),
            description: None,
            project_id: project_id.to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    #[test]
    fn extract_focus_slug_from_parent_file_description() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        let mut parent = make_task("proj1");
        parent.description = Some(
            "Target keyword: focus topic\nFile: ./content/blog/42_focus_topic.mdx | KD: 10"
                .to_string(),
        );
        let slug = extract_focus_slug_from_parent(&conn, &parent).unwrap();
        assert_eq!(slug, "focus-topic");
    }

    #[test]
    fn extract_focus_slug_from_parent_artifact() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        let mut parent = make_task("proj1");
        parent.artifacts = vec![TaskArtifact {
            key: "article_slug".to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some("my-new-post".to_string()),
        }];
        assert_eq!(
            extract_focus_slug_from_parent(&conn, &parent).as_deref(),
            Some("my-new-post")
        );
    }

    #[test]
    fn create_cluster_and_link_task_attaches_focus_slug() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', '/tmp/cl-scan', 1, 'workspace')",
            [],
        )
        .unwrap();
        let mut parent = make_task("proj1");
        parent.id = "write-parent-1".to_string();
        parent.description =
            Some("File: content/blog/01_fresh_article.mdx\nTarget keyword: fresh".to_string());
        crate::engine::task_store::create_task(&conn, &parent).unwrap();

        let cluster_id = create_cluster_and_link_task(&conn, &parent, "/tmp/cl-scan")
            .expect("spawn cluster task");
        let cluster = crate::engine::task_store::get_task(&conn, &cluster_id).unwrap();
        let focus = cluster
            .artifacts
            .iter()
            .find(|a| a.key == "focus_slug")
            .and_then(|a| a.content.as_deref());
        assert_eq!(focus, Some("fresh-article"));
    }

    /// Origin A: a <1h stale link_scan.json that omits a newly written article
    /// must not short-circuit — scan always rescans and includes the new id.
    #[test]
    fn cluster_link_scan_always_fresh_ignores_stale_cache() {
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("pageseeds-cl-scan-{}", Uuid::new_v4()));
        let content_dir = dir.join("content").join("blog");
        let automation = dir.join(".github").join("automation");
        fs::create_dir_all(&content_dir).unwrap();
        fs::create_dir_all(&automation).unwrap();

        // Stale cache: only article 1, written "just now" (mtime = now).
        let stale = serde_json::json!({
            "total_articles": 1,
            "total_internal_links": 0,
            "articles_with_outgoing": 0,
            "articles_with_incoming": 0,
            "orphan_ids": [1],
            "zero_incoming_ids": [1],
            "unresolved_links": [],
            "profiles": [{
                "id": 1,
                "title": "Old",
                "file": "1_old.mdx",
                "outgoing_ids": [],
                "incoming_ids": [],
                "unresolved_links": []
            }]
        });
        fs::write(
            automation.join("link_scan.json"),
            serde_json::to_string_pretty(&stale).unwrap(),
        )
        .unwrap();

        fs::write(
            content_dir.join("1_old.mdx"),
            "---\ntitle: \"Old\"\n---\n\n# Old\n\nBody.\n",
        )
        .unwrap();
        fs::write(
            content_dir.join("2_new.mdx"),
            "---\ntitle: \"New\"\n---\n\n# New\n\nBody.\n",
        )
        .unwrap();

        let db_path = dir.join("test.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            crate::db::init_with_conn(&conn).unwrap();
            conn.execute(
                "INSERT INTO projects (id, name, path, active, project_mode)
                 VALUES ('proj1', 'Test', ?1, 1, 'workspace')",
                rusqlite::params![dir.to_string_lossy().as_ref()],
            )
            .unwrap();
            for (id, title, slug, file) in [
                (1i64, "Old", "old", "content/blog/1_old.mdx"),
                (2i64, "New", "new", "content/blog/2_new.mdx"),
            ] {
                conn.execute(
                    "INSERT INTO articles (
                        id, project_id, title, url_slug, file, status,
                        content_gaps_addressed, target_volume, word_count, review_count
                     ) VALUES (?1, 'proj1', ?2, ?3, ?4, 'draft', '[]', 0, 0, 0)",
                    rusqlite::params![id, title, slug, file],
                )
                .unwrap();
            }
        }

        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        std::env::set_var("PAGESEEDS_DB_PATH", &db_path);

        let task = make_task("proj1");
        let result = exec_cluster_link_scan(&task, dir.to_string_lossy().as_ref());

        match old_db {
            Some(v) => std::env::set_var("PAGESEEDS_DB_PATH", v),
            None => std::env::remove_var("PAGESEEDS_DB_PATH"),
        }
        let _ = fs::remove_dir_all(&dir);

        assert!(result.success, "scan failed: {}", result.message);
        assert!(
            !result.message.contains("cached"),
            "must not use cache: {}",
            result.message
        );
        let out: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(out["total_articles"].as_u64(), Some(2));
        let profile_ids: Vec<i64> = out["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["id"].as_i64())
            .collect();
        assert!(
            profile_ids.contains(&2),
            "fresh scan must include new article id=2, got {:?}",
            profile_ids
        );
    }
}
