//! Shared project create / link helper for Tauri commands and CLI (issue #177).
//!
//! Link / idempotency (locked):
//! - Path already registered → reuse that project id; no duplicate row
//! - Id exists with a **different** path → refuse with a clear error (no silent relink)
//! - Fresh path + id → create, seed scheduler, initialize workspace (Workspace mode)

use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::project::{Project, ProjectMode};

/// Inputs for create-or-link (mirrors the Tauri `create_project` surface).
#[derive(Debug, Clone)]
pub struct CreateProjectParams {
    pub name: String,
    pub path: Option<String>,
    pub content_dir: Option<String>,
    pub site_url: Option<String>,
    pub site_id: Option<String>,
    pub sitemap_url: Option<String>,
    pub project_mode: ProjectMode,
    pub clarity_project_id: Option<String>,
}

/// Result of create-or-link.
#[derive(Debug, Clone)]
pub struct CreateProjectOutcome {
    pub project: Project,
    /// `true` when a new row was inserted; `false` when an existing project was reused.
    pub created: bool,
}

/// Slugify a display name into a stable project id.
/// Same rules as the historical Tauri command: lowercase, non-alnum → `_`, trim `_`.
pub fn slugify_project_id(name: &str) -> String {
    let id = name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "_")
        .trim_matches('_')
        .to_string();
    if id.is_empty() {
        format!("project_{}", chrono::Utc::now().timestamp())
    } else {
        id
    }
}

fn managed_project_root(project_id: &str) -> Result<PathBuf> {
    let db_path = crate::db::default_db_path();
    let app_dir = db_path.parent().ok_or_else(|| {
        Error::Other("Could not resolve application data directory".to_string())
    })?;
    let root = app_dir.join("managed_projects").join(project_id);
    std::fs::create_dir_all(&root).map_err(|e| {
        Error::Other(format!("Failed to create managed project directory: {e}"))
    })?;
    Ok(root)
}

/// Create a new project or link to an existing one by path (idempotent).
pub fn create_or_link_project(
    conn: &Connection,
    params: CreateProjectParams,
) -> Result<CreateProjectOutcome> {
    let id = slugify_project_id(&params.name);

    if let Some(value) = params.site_url.as_deref() {
        crate::models::project::validate_site_url(value)
            .map_err(Error::Other)?;
    }

    let name_for_init = params.name.clone();
    let project_mode = params.project_mode;
    // Normalize Workspace paths once so stored rows and path-match identity are
    // absolute/stable regardless of caller (setup, create-project, Tauri).
    let resolved_path = match project_mode {
        ProjectMode::Workspace => {
            let raw = params
                .path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    Error::Other("Workspace projects require a repository path".to_string())
                })?;
            crate::config::cli_config::normalize_path_string(raw)
        }
        ProjectMode::LiveSite => {
            let site_url = params
                .site_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    Error::Other("Live site projects require a site URL".to_string())
                })?;
            let root = managed_project_root(&id)?;
            log::info!(
                "[create_project] creating live-site project '{}' for {} in {:?}",
                id,
                site_url,
                root,
            );
            root.to_string_lossy().to_string()
        }
    };

    // Prefer path match first — re-setup / re-create on same path reuses the row.
    if let Some(existing) = task_store::find_project_by_path(conn, &resolved_path)? {
        let mut project = existing;
        // Cheap re-setup improvement: apply --site-url when provided on a link.
        if let Some(url) = params
            .site_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if project.site_url.as_deref() != Some(url) {
                project.site_url = Some(url.to_string());
                project = task_store::update_project(conn, &project)?;
            }
        }
        // Refresh scheduler defaults (idempotent) and workspace files for workspace mode.
        if let Err(e) = crate::engine::scheduler::seed_default_rules(conn, &project.id) {
            log::warn!("[create_project] Failed to seed scheduler rules: {}", e);
        }
        if project.project_mode == ProjectMode::Workspace {
            let repo_root = Path::new(&project.path);
            if let Err(e) = crate::engine::setup_check::initialize_project_workspace(
                repo_root,
                project.site_url.as_deref(),
                Some(&project.name),
            ) {
                log::warn!(
                    "[create_project] Failed to auto-initialize workspace: {}",
                    e
                );
            }
        }
        return Ok(CreateProjectOutcome {
            project,
            created: false,
        });
    }

    // Id collision with a different path must refuse (no silent relink).
    if let Ok(existing) = task_store::get_project(conn, &id) {
        if !crate::config::cli_config::paths_equal(&existing.path, &resolved_path) {
            return Err(Error::Other(format!(
                "Project id '{id}' is already registered at a different path ({}). \
Choose a different --name or remove the existing project first.",
                existing.path
            )));
        }
        // Same path (string drift) — treat as link.
        if let Err(e) = crate::engine::scheduler::seed_default_rules(conn, &existing.id) {
            log::warn!("[create_project] Failed to seed scheduler rules: {}", e);
        }
        return Ok(CreateProjectOutcome {
            project: existing,
            created: false,
        });
    }

    let normalized_content_dir = match project_mode {
        ProjectMode::Workspace => params.content_dir,
        ProjectMode::LiveSite => None,
    };

    let project = Project {
        id: id.clone(),
        name: params.name,
        path: resolved_path.clone(),
        content_dir: normalized_content_dir,
        site_url: params.site_url.clone(),
        site_id: params.site_id,
        sitemap_url: params.sitemap_url,
        project_mode: project_mode.clone(),
        active: true,
        agent_provider: None,
        seo_provider: Some("dataforseo".to_string()),
        clarity_project_id: params.clarity_project_id,
    };

    let project = task_store::create_project(conn, &project)?;

    if let Err(e) = crate::engine::scheduler::seed_default_rules(conn, &id) {
        log::warn!("[create_project] Failed to seed scheduler rules: {}", e);
    }

    if project_mode == ProjectMode::Workspace {
        let repo_root = Path::new(&resolved_path);
        if let Err(e) = crate::engine::setup_check::initialize_project_workspace(
            repo_root,
            params.site_url.as_deref(),
            Some(&name_for_init),
        ) {
            log::warn!(
                "[create_project] Failed to auto-initialize workspace: {}",
                e
            );
            // Don't fail project creation if initialization fails - user can fix manually
        }
    }

    Ok(CreateProjectOutcome {
        project,
        created: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("{prefix}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_db() -> (PathBuf, Connection) {
        let dir = unique_temp_dir("ps_proj_create");
        let db_path = dir.join("test.db");
        let conn = crate::db::init(&db_path).unwrap();
        (dir, conn)
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify_project_id("My Site!"), "my_site");
        assert_eq!(slugify_project_id("  Hello World  "), "hello_world");
    }

    #[test]
    fn create_then_link_is_idempotent() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, conn) = temp_db();
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let path = repo.to_string_lossy().to_string();

        let first = create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Demo Site".into(),
                path: Some(path.clone()),
                content_dir: None,
                site_url: None,
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap();
        assert!(first.created);
        assert_eq!(first.project.id, "demo_site");

        let second = create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Demo Site".into(),
                path: Some(path.clone()),
                content_dir: None,
                site_url: None,
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap();
        assert!(!second.created);
        assert_eq!(second.project.id, first.project.id);

        let all = task_store::list_projects(&conn).unwrap();
        assert_eq!(all.len(), 1, "re-setup must not create a duplicate project");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn id_collision_different_path_refuses() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, conn) = temp_db();
        let repo_a = dir.join("a");
        let repo_b = dir.join("b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Same Name".into(),
                path: Some(repo_a.to_string_lossy().to_string()),
                content_dir: None,
                site_url: None,
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap();

        let err = create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Same Name".into(),
                path: Some(repo_b.to_string_lossy().to_string()),
                content_dir: None,
                site_url: None,
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("different path") || msg.contains("already registered"),
            "expected refuse-on-relink error, got: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_project_by_path_matches() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, conn) = temp_db();
        let repo = dir.join("site");
        std::fs::create_dir_all(&repo).unwrap();
        let path = repo.to_string_lossy().to_string();

        create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Site".into(),
                path: Some(path.clone()),
                content_dir: None,
                site_url: None,
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap();

        let found = task_store::find_project_by_path(&conn, &path)
            .unwrap()
            .expect("should find by path");
        assert_eq!(found.id, "site");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_path_normalized_on_store() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, conn) = temp_db();
        let repo = dir.join("normed");
        std::fs::create_dir_all(&repo).unwrap();
        let abs = repo.canonicalize().unwrap().to_string_lossy().to_string();
        // Pass with trailing spaces / uncanonical form — stored row must be normalized.
        let outcome = create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Normed".into(),
                path: Some(format!("  {abs}  ")),
                content_dir: None,
                site_url: None,
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap();
        assert!(outcome.created);
        assert_eq!(outcome.project.path, abs);

        // Relative-looking re-link via the absolute path still hits the same row.
        let again = create_or_link_project(
            &conn,
            CreateProjectParams {
                name: "Normed".into(),
                path: Some(abs.clone()),
                content_dir: None,
                site_url: Some("sc-domain:example.com".into()),
                site_id: None,
                sitemap_url: None,
                project_mode: ProjectMode::Workspace,
                clarity_project_id: None,
            },
        )
        .unwrap();
        assert!(!again.created);
        assert_eq!(
            again.project.site_url.as_deref(),
            Some("sc-domain:example.com"),
            "re-setup with --site-url should update the linked row"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
