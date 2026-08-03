//! CLI setup / list / create orchestration (issue #177).
//!
//! Pure lib surface for free meta tools that used to live in `pageseeds-cli` bin:
//! - `list_projects` payload
//! - `create_project` (shared path/name defaults + create_or_link)
//! - `setup` (license side-effect, create/link, write defaults, first-win desk read)
//! - `setup_status` (report-only readiness)
//!
//! The bin stays argv parse → call → print/exit only.

use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::config::cli_config::{self, GlobalCliConfig, LocalCliConfig};
use crate::engine::project_create::{create_or_link_project, CreateProjectOutcome, CreateProjectParams};
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::license::LicenseStatus;
use crate::models::project::{Project, ProjectMode};

// ─── Shared path / name defaults ─────────────────────────────────────────────

/// Resolved workspace path + display name used by both `create-project` and `setup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathNameDefaults {
    /// Absolute / match-stable path (via [`cli_config::normalize_path_string`]).
    pub path: String,
    pub name: String,
}

/// Collapse shared path/name defaults for `create-project` and `setup`.
///
/// - `path_raw`: `--path` / `-p` / cwd fallback (tilde expanded + normalized)
/// - `name_raw`: `--name` / `-n`; else last path component or `"project"`
pub fn resolve_path_name_defaults(
    path_raw: Option<&str>,
    name_raw: Option<&str>,
    cwd: Option<&Path>,
) -> PathNameDefaults {
    let path_input = path_raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            cwd.map(|p| p.to_string_lossy().to_string())
                .or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| ".".into())
        });
    let path = cli_config::normalize_path_string(&path_input);
    let name = name_raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            Path::new(&path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "project".into())
        });
    PathNameDefaults { path, name }
}

// ─── List projects ───────────────────────────────────────────────────────────

/// JSON-serializable list-projects outcome.
#[derive(Debug, Clone, Serialize)]
pub struct ListProjectsOutcome {
    pub count: usize,
    pub projects: Vec<Project>,
}

/// List registered projects (same DB as desktop).
pub fn list_projects(conn: &Connection) -> Result<ListProjectsOutcome> {
    let projects = task_store::list_projects(conn)?;
    Ok(ListProjectsOutcome {
        count: projects.len(),
        projects,
    })
}

// ─── Create project ──────────────────────────────────────────────────────────

/// Inputs for CLI `create-project` (flags already parsed).
#[derive(Debug, Clone)]
pub struct CreateProjectOpts {
    pub path: Option<String>,
    pub name: Option<String>,
    pub site_url: Option<String>,
    /// Working directory for path default when `path` is None.
    pub cwd: Option<PathBuf>,
}

/// Create or link a workspace project via the shared helper.
pub fn create_project(conn: &Connection, opts: CreateProjectOpts) -> Result<CreateProjectOutcome> {
    let defaults = resolve_path_name_defaults(
        opts.path.as_deref(),
        opts.name.as_deref(),
        opts.cwd.as_deref(),
    );
    create_or_link_project(
        conn,
        CreateProjectParams {
            name: defaults.name,
            path: Some(defaults.path),
            content_dir: None,
            site_url: opts.site_url,
            site_id: None,
            sitemap_url: None,
            project_mode: ProjectMode::Workspace,
            clarity_project_id: None,
        },
    )
}

// ─── Setup ───────────────────────────────────────────────────────────────────

/// Inputs for CLI `setup` (flags already parsed).
#[derive(Debug, Clone)]
pub struct SetupOpts {
    pub path: Option<String>,
    pub name: Option<String>,
    pub site_url: Option<String>,
    pub license_key: Option<String>,
    pub skip_first_win: bool,
    /// Working directory for path default when `path` is None.
    pub cwd: Option<PathBuf>,
}

/// Subset of project fields emitted in setup JSON.
#[derive(Debug, Clone, Serialize)]
pub struct SetupProjectSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    pub site_url: Option<String>,
}

/// Config write markers for setup JSON.
#[derive(Debug, Clone, Serialize)]
pub struct SetupConfigWritten {
    pub global_written: bool,
    pub local_written: bool,
    pub local_path: String,
}

/// License side-effect result for setup JSON.
#[derive(Debug, Clone, Serialize)]
pub struct SetupLicenseInfo {
    pub activated: bool,
    pub error: Option<String>,
    pub status: LicenseStatus,
}

/// Full setup outcome (JSON-serializable; also drives human progress lines).
#[derive(Debug, Clone, Serialize)]
pub struct SetupOutcome {
    pub ok: bool,
    pub created: bool,
    pub project: SetupProjectSummary,
    pub config: SetupConfigWritten,
    pub license: SetupLicenseInfo,
    pub first_win: Option<serde_json::Value>,
    pub first_win_error: Option<String>,
    pub next: Vec<String>,
    /// Not serialized — only drives human "first-win: skipped" wording.
    #[serde(skip)]
    pub first_win_skipped: bool,
}

impl SetupOutcome {
    /// Human progress lines for stderr (product behavior preserved from the bin).
    pub fn human_progress_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "Project {} ({})",
            self.project.id,
            if self.created { "created" } else { "linked" }
        ));
        lines.push(format!("  path: {}", self.project.path));
        lines.push("  wrote global defaults + .pageseeds.yaml".into());
        if self.license.activated {
            lines.push("  license: activated".into());
        } else if let Some(err) = &self.license.error {
            lines.push(format!(
                "  license: activate failed ({err}) — free desk still works"
            ));
        } else {
            match &self.license.status {
                LicenseStatus::Valid { .. } => lines.push("  license: valid".into()),
                LicenseStatus::Missing => {
                    lines.push("  license: none (optional for free desk tools)".into());
                }
                other => lines.push(format!("  license: {other:?}")),
            }
        }
        if self.first_win_skipped {
            lines.push("  first-win: skipped".into());
        } else if self.first_win.is_some() {
            lines.push("  first-win: site-overview ok".into());
        } else if let Some(err) = &self.first_win_error {
            lines.push(format!("  first-win: site-overview failed ({err})"));
        }
        lines.push("Next: pageseeds-cli site-overview".into());
        lines
    }
}

/// One-shot onboarding: optional license activate → create/link → write defaults → first-win.
pub fn setup(conn: &Connection, opts: SetupOpts) -> Result<SetupOutcome> {
    let defaults = resolve_path_name_defaults(
        opts.path.as_deref(),
        opts.name.as_deref(),
        opts.cwd.as_deref(),
    );
    let abs_path = defaults.path.clone();

    // 1. Optional license activate (free path still completes on failure).
    let mut license_activated = false;
    let mut license_error: Option<String> = None;
    if let Some(key) = opts
        .license_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        match crate::license::activate(key) {
            Ok(()) => license_activated = true,
            Err(e) => license_error = Some(e),
        }
    }
    let license_status = crate::license::status();

    // 2–3. Link or create workspace project
    let outcome = create_or_link_project(
        conn,
        CreateProjectParams {
            name: defaults.name,
            path: Some(abs_path.clone()),
            content_dir: None,
            site_url: opts.site_url,
            site_id: None,
            sitemap_url: None,
            project_mode: ProjectMode::Workspace,
            clarity_project_id: None,
        },
    )?;

    // 4. Write global + local config defaults
    cli_config::save_global(&GlobalCliConfig {
        default_project_id: Some(outcome.project.id.clone()),
        default_project_path: Some(outcome.project.path.clone()),
    })
    .map_err(|e| Error::Other(format!("failed to write global CLI config: {e}")))?;

    let local_cwd = Path::new(&abs_path);
    cli_config::save_local(
        local_cwd,
        &LocalCliConfig {
            project_id: Some(outcome.project.id.clone()),
        },
    )
    .map_err(|e| Error::Other(format!("failed to write .pageseeds.yaml: {e}")))?;

    // 5. First-win desk read (free site-overview)
    let mut first_win: Option<serde_json::Value> = None;
    let mut first_win_error: Option<String> = None;
    let first_win_skipped = opts.skip_first_win;
    if !opts.skip_first_win {
        match crate::engine::site_state::build_site_overview(
            conn,
            &outcome.project.id,
            &outcome.project.path,
            None,
        ) {
            Ok(r) => first_win = Some(serde_json::to_value(r).unwrap_or_default()),
            Err(e) => first_win_error = Some(e.to_string()),
        }
    }

    Ok(SetupOutcome {
        ok: true,
        created: outcome.created,
        project: SetupProjectSummary {
            id: outcome.project.id.clone(),
            name: outcome.project.name.clone(),
            path: outcome.project.path.clone(),
            site_url: outcome.project.site_url.clone(),
        },
        config: SetupConfigWritten {
            global_written: true,
            local_written: true,
            local_path: format!("{}/.pageseeds.yaml", abs_path.trim_end_matches('/')),
        },
        license: SetupLicenseInfo {
            activated: license_activated,
            error: license_error,
            status: license_status,
        },
        first_win,
        first_win_error,
        next: vec![
            "pageseeds-cli site-overview".into(),
            "pageseeds-cli articles -m 100 -l 20".into(),
        ],
        first_win_skipped,
    })
}

// ─── Setup status ────────────────────────────────────────────────────────────

/// Report-only readiness check inputs.
#[derive(Debug, Clone)]
pub struct SetupStatusOpts {
    pub path: Option<String>,
    pub cwd: Option<PathBuf>,
}

/// Setup status payload (JSON-serializable).
#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusOutcome {
    pub desk_ready: bool,
    pub binary: bool,
    pub config: SetupStatusConfig,
    pub project: Option<SetupStatusProject>,
    pub license: LicenseStatus,
    pub gsc_env_present: bool,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusConfig {
    pub global: bool,
    pub local: bool,
    pub default_project_id: Option<String>,
    pub default_project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusProject {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// Report-only readiness check (no mutations). `desk_ready == false` → bin exits 1.
pub fn setup_status(
    conn: Option<&Connection>,
    opts: SetupStatusOpts,
) -> Result<SetupStatusOutcome> {
    let defaults = resolve_path_name_defaults(opts.path.as_deref(), None, opts.cwd.as_deref());
    let abs_path = defaults.path;

    let binary_ok = true; // we're running
    let config_global = cli_config::load_global().unwrap_or_default();
    let has_global = config_global.default_project_id.is_some()
        && config_global.default_project_path.is_some();
    let local = cli_config::load_local(Path::new(&abs_path)).ok().flatten();
    let has_local = local
        .as_ref()
        .and_then(|l| l.project_id.as_ref())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let project_registered = conn
        .and_then(|c| task_store::find_project_by_path(c, &abs_path).ok().flatten())
        .or_else(|| {
            let id = config_global
                .default_project_id
                .as_deref()
                .or_else(|| local.as_ref().and_then(|l| l.project_id.as_deref()))?;
            conn.and_then(|c| task_store::get_project(c, id).ok())
        });

    let license_status = crate::license::status();
    // Use EnvResolver (secrets.env → project .env* → shell), not process env
    // alone — otherwise setup --status lies with gsc_env_present=false while
    // gsc-performance works via secrets.env.
    let resolver = crate::config::env_resolver::EnvResolver::new(&abs_path);
    let gsc_env = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH").is_some()
        || resolver
            .resolve("GOOGLE_APPLICATION_CREDENTIALS")
            .is_some()
        || resolver
            .resolve("GSC_REPORT_OAUTH_CLIENT_SECRETS")
            .is_some();

    let desk_ready = has_global || has_local || project_registered.is_some();
    Ok(SetupStatusOutcome {
        desk_ready,
        binary: binary_ok,
        config: SetupStatusConfig {
            global: has_global,
            local: has_local,
            default_project_id: config_global.default_project_id,
            default_project_path: config_global.default_project_path,
        },
        project: project_registered.map(|p| SetupStatusProject {
            id: p.id,
            name: p.name,
            path: p.path,
        }),
        license: license_status,
        gsc_env_present: gsc_env,
        path: abs_path,
        hint: if desk_ready {
            None
        } else {
            Some("Run `pageseeds-cli setup --path . --yes` to register this project.".into())
        },
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
        let dir = unique_temp_dir("ps_cli_setup");
        let db_path = dir.join("test.db");
        let conn = crate::db::init(&db_path).unwrap();
        (dir, conn)
    }

    #[test]
    fn path_name_defaults_from_path_component() {
        let d = resolve_path_name_defaults(Some("/tmp/my-site"), None, None);
        assert!(d.path.contains("my-site") || d.path.ends_with("my-site"));
        assert_eq!(d.name, "my-site");
    }

    #[test]
    fn path_name_defaults_explicit_name_wins() {
        let d = resolve_path_name_defaults(Some("/tmp/my-site"), Some("Custom"), None);
        assert_eq!(d.name, "Custom");
    }

    #[test]
    fn create_project_idempotent_via_cli_setup() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, conn) = temp_db();
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let path = repo.to_string_lossy().to_string();

        let first = create_project(
            &conn,
            CreateProjectOpts {
                path: Some(path.clone()),
                name: Some("Demo".into()),
                site_url: None,
                cwd: None,
            },
        )
        .unwrap();
        assert!(first.created);

        let second = create_project(
            &conn,
            CreateProjectOpts {
                path: Some(path),
                name: Some("Demo".into()),
                site_url: None,
                cwd: None,
            },
        )
        .unwrap();
        assert!(!second.created);
        assert_eq!(first.project.id, second.project.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn setup_writes_defaults_and_links() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = ["PAGESEEDS_CONFIG_DIR", "PAGESEEDS_CONFIG_PATH"];
        let saved: Vec<Option<String>> = keys.iter().map(|k| std::env::var(k).ok()).collect();
        let (dir, conn) = temp_db();
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        std::env::set_var("PAGESEEDS_CONFIG_DIR", cfg.to_string_lossy().as_ref());
        std::env::remove_var("PAGESEEDS_CONFIG_PATH");

        let repo = dir.join("site");
        std::fs::create_dir_all(&repo).unwrap();

        let outcome = setup(
            &conn,
            SetupOpts {
                path: Some(repo.to_string_lossy().to_string()),
                name: Some("Site".into()),
                site_url: None,
                license_key: None,
                skip_first_win: true,
                cwd: None,
            },
        )
        .unwrap();
        assert!(outcome.ok);
        assert!(outcome.created);
        assert!(outcome.config.global_written);
        assert!(outcome.config.local_written);
        assert!(repo.join(".pageseeds.yaml").exists());

        // Status should report desk-ready.
        let status = setup_status(
            Some(&conn),
            SetupStatusOpts {
                path: Some(repo.to_string_lossy().to_string()),
                cwd: None,
            },
        )
        .unwrap();
        assert!(status.desk_ready);

        for (k, prev) in keys.iter().zip(saved.iter()) {
            match prev {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn setup_status_gsc_env_present_via_project_env_file() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, conn) = temp_db();
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let sa = dir.join("sa.json");
        std::fs::write(&sa, "{}").unwrap();
        std::fs::write(
            repo.join(".env"),
            format!("GSC_SERVICE_ACCOUNT_PATH={}", sa.display()),
        )
        .unwrap();

        let status = setup_status(
            Some(&conn),
            SetupStatusOpts {
                path: Some(repo.to_string_lossy().to_string()),
                cwd: None,
            },
        )
        .unwrap();
        assert!(
            status.gsc_env_present,
            "expected gsc_env_present when SA path is in project .env"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
