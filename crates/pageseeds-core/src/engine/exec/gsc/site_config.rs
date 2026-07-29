//! Shared GSC site URL resolution.
//!
//! **Canonical store:** `projects.site_url` (SQLite). Use
//! [`crate::engine::site_url_sync`] to consolidate scattered values into it.
//!
//! Fallbacks (legacy / incomplete setup only):
//! - `.github/automation/manifest.json` (`gsc_site` / `url` / `site_url`)
//! - `.github/automation/seo_workspace.json` (`site_url`)

use crate::engine::project_paths::ProjectPaths;
use crate::models::project::site_base_url;

/// Where a resolved site config came from (for logs / error detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SiteConfigSource {
    ProjectDb,
    Manifest,
    SeoWorkspace,
}

impl SiteConfigSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ProjectDb => "projects.site_url",
            Self::Manifest => "manifest.json",
            Self::SeoWorkspace => "seo_workspace.json",
        }
    }
}

/// Resolved GSC property + sitemap for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSiteConfig {
    /// Raw GSC property id (`sc-domain:…` or `https://…`). Pass this to GSC APIs.
    pub site_url: String,
    pub sitemap_url: String,
    pub source: SiteConfigSource,
}

/// Resolve `site_url` + `sitemap_url` for GSC workflows.
///
/// Order (canonical first):
/// 1. `projects.site_url` in the app SQLite DB
/// 2. `manifest.json` — `gsc_site` | `url` | `site_url` (non-empty)
/// 3. `seo_workspace.json` — `site_url` (written by workspace init)
///
/// Sitemap: explicit `sitemap` / `sitemap_url` on the same source when present,
/// otherwise `{site_base_url}/sitemap.xml`.
pub(crate) fn resolve_site_config(
    project_id: &str,
    project_path: &str,
) -> Result<ResolvedSiteConfig, String> {
    let paths = ProjectPaths::from_path(project_path);
    let manifest_path = paths.automation_dir.join("manifest.json");
    let workspace_path = paths.automation_dir.join("seo_workspace.json");

    if let Some(cfg) = site_from_project_db(project_id) {
        return Ok(cfg);
    }
    if let Some(cfg) = site_from_manifest(&manifest_path) {
        return Ok(cfg);
    }
    if let Some(cfg) = site_from_seo_workspace(&workspace_path) {
        return Ok(cfg);
    }

    Err(format!(
        "No site_url configured for project '{project_id}'. \
         Checked: projects.site_url, {} (gsc_site/url/site_url), and {} (site_url). \
         Fix: set site_url on the project (pageseeds-cli setup --site-url … / sync-site-urls), \
         or add \"gsc_site\" to {}.",
        manifest_path.display(),
        workspace_path.display(),
        manifest_path.display()
    ))
}

/// Convenience: only the GSC property id.
pub(crate) fn resolve_site_url(project_id: &str, project_path: &str) -> Result<String, String> {
    resolve_site_config(project_id, project_path).map(|c| c.site_url)
}

fn non_empty_str(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|u| u.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn default_sitemap(site_url: &str) -> String {
    format!("{}/sitemap.xml", site_base_url(site_url))
}

fn site_from_manifest(path: &std::path::Path) -> Option<ResolvedSiteConfig> {
    let raw = std::fs::read_to_string(path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let site_url = non_empty_str(manifest.get("gsc_site"))
        .or_else(|| non_empty_str(manifest.get("url")))
        .or_else(|| non_empty_str(manifest.get("site_url")))?;
    let sitemap_url = non_empty_str(manifest.get("sitemap"))
        .or_else(|| non_empty_str(manifest.get("sitemap_url")))
        .unwrap_or_else(|| default_sitemap(&site_url));
    Some(ResolvedSiteConfig {
        site_url,
        sitemap_url,
        source: SiteConfigSource::Manifest,
    })
}

fn site_from_seo_workspace(path: &std::path::Path) -> Option<ResolvedSiteConfig> {
    let raw = std::fs::read_to_string(path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let site_url = non_empty_str(doc.get("site_url"))?;
    let sitemap_url = non_empty_str(doc.get("sitemap"))
        .or_else(|| non_empty_str(doc.get("sitemap_url")))
        .unwrap_or_else(|| default_sitemap(&site_url));
    Some(ResolvedSiteConfig {
        site_url,
        sitemap_url,
        source: SiteConfigSource::SeoWorkspace,
    })
}

fn site_from_project_db(project_id: &str) -> Option<ResolvedSiteConfig> {
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open(db_path).ok()?;
    let project = crate::engine::task_store::get_project(&conn, project_id).ok()?;
    let site_url = project
        .site_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)?;
    let sitemap_url = project
        .sitemap_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| default_sitemap(&site_url));
    Some(ResolvedSiteConfig {
        site_url,
        sitemap_url,
        source: SiteConfigSource::ProjectDb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::project::{Project, ProjectMode};
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}_{nanos}_{n}"))
    }

    fn write_json(path: &std::path::Path, value: serde_json::Value) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }

    fn with_temp_db<F, R>(f: F) -> R
    where
        F: FnOnce(&Connection) -> R,
    {
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let dir = unique_temp_dir("site_config_db");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        std::env::set_var("PAGESEEDS_DB_PATH", &db_path);
        let conn = db::init(&db_path).unwrap();
        let result = f(&conn);
        drop(conn);
        match old_db {
            Some(v) => std::env::set_var("PAGESEEDS_DB_PATH", v),
            None => std::env::remove_var("PAGESEEDS_DB_PATH"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    fn insert_project(conn: &Connection, id: &str, path: &str, site_url: Option<&str>) {
        let project = Project {
            id: id.to_string(),
            name: id.to_string(),
            path: path.to_string(),
            content_dir: None,
            site_url: site_url.map(String::from),
            site_id: None,
            sitemap_url: None,
            project_mode: ProjectMode::Workspace,
            active: true,
            agent_provider: None,
            seo_provider: Some("dataforseo".to_string()),
            clarity_project_id: None,
        };
        crate::engine::task_store::create_project(conn, &project).unwrap();
    }

    #[test]
    fn prefers_project_db_over_manifest() {
        with_temp_db(|conn| {
            let root = unique_temp_dir("site_config_manifest");
            let automation = root.join(".github").join("automation");
            write_json(
                &automation.join("manifest.json"),
                serde_json::json!({
                    "gsc_site": "sc-domain:manifest.example",
                    "url": "https://ignored.example"
                }),
            );
            insert_project(conn, "p1", root.to_str().unwrap(), Some("sc-domain:db.example"));

            let cfg = resolve_site_config("p1", root.to_str().unwrap()).unwrap();
            assert_eq!(cfg.site_url, "sc-domain:db.example");
            assert_eq!(cfg.source, SiteConfigSource::ProjectDb);
            assert_eq!(cfg.sitemap_url, "https://db.example/sitemap.xml");

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn accepts_manifest_site_url_alias() {
        with_temp_db(|_conn| {
            let root = unique_temp_dir("site_config_alias");
            let automation = root.join(".github").join("automation");
            write_json(
                &automation.join("manifest.json"),
                serde_json::json!({ "site_url": "https://alias.example/" }),
            );

            let cfg = resolve_site_config("missing", root.to_str().unwrap()).unwrap();
            assert_eq!(cfg.site_url, "https://alias.example/");
            assert_eq!(cfg.source, SiteConfigSource::Manifest);

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn falls_back_to_seo_workspace_when_manifest_missing_site() {
        with_temp_db(|_conn| {
            let root = unique_temp_dir("site_config_workspace");
            let automation = root.join(".github").join("automation");
            // Manifest exists but has no site fields (the common "incomplete" case).
            write_json(
                &automation.join("manifest.json"),
                serde_json::json!({ "name": "Demo" }),
            );
            write_json(
                &automation.join("seo_workspace.json"),
                serde_json::json!({ "site_url": "sc-domain:workspace.example", "content_dir": "content" }),
            );

            let cfg = resolve_site_config("missing", root.to_str().unwrap()).unwrap();
            assert_eq!(cfg.site_url, "sc-domain:workspace.example");
            assert_eq!(cfg.source, SiteConfigSource::SeoWorkspace);

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn falls_back_to_project_db_when_no_manifest() {
        with_temp_db(|conn| {
            let root = unique_temp_dir("site_config_db_only");
            std::fs::create_dir_all(&root).unwrap();
            insert_project(
                conn,
                "days_to_expiry",
                root.to_str().unwrap(),
                Some("sc-domain:daystoexpiry.com"),
            );

            let cfg = resolve_site_config("days_to_expiry", root.to_str().unwrap()).unwrap();
            assert_eq!(cfg.site_url, "sc-domain:daystoexpiry.com");
            assert_eq!(cfg.source, SiteConfigSource::ProjectDb);
            assert_eq!(
                cfg.sitemap_url,
                "https://daystoexpiry.com/sitemap.xml"
            );

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn empty_manifest_fields_do_not_block_db_fallback() {
        with_temp_db(|conn| {
            let root = unique_temp_dir("site_config_empty");
            let automation = root.join(".github").join("automation");
            write_json(
                &automation.join("manifest.json"),
                serde_json::json!({ "gsc_site": "  ", "url": "" }),
            );
            insert_project(
                conn,
                "p-empty",
                root.to_str().unwrap(),
                Some("https://real.example"),
            );

            let cfg = resolve_site_config("p-empty", root.to_str().unwrap()).unwrap();
            assert_eq!(cfg.site_url, "https://real.example");
            assert_eq!(cfg.source, SiteConfigSource::ProjectDb);

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn error_lists_checked_locations() {
        with_temp_db(|_conn| {
            let root = unique_temp_dir("site_config_err");
            std::fs::create_dir_all(&root).unwrap();
            let err = resolve_site_config("nope", root.to_str().unwrap()).unwrap_err();
            assert!(err.contains("No site_url configured"));
            assert!(err.contains("manifest.json"));
            assert!(err.contains("seo_workspace.json"));
            assert!(err.contains("projects.site_url"));
            let _ = std::fs::remove_dir_all(&root);
        });
    }
}
