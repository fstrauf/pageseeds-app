//! One-off (idempotent) consolidation of site URL config into `projects.site_url`.
//!
//! Historically the GSC property lived in several places:
//! - SQLite `projects.site_url` / `sitemap_url` (CLI + live GSC tools)
//! - `.github/automation/manifest.json` (`gsc_site` / `url` / `site_url`, `sitemap`)
//! - `.github/automation/seo_workspace.json` (`site_url`)
//!
//! Operational truth is the projects table. This module gathers every non-empty
//! candidate, picks one winner, and writes it (and sitemap when known) into the
//! DB so desk + live tools agree without hand-editing manifests.

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::task_store;
use crate::error::Result;
use crate::models::project::{site_base_url, validate_site_url, Project};

/// Where a candidate value was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteUrlSource {
    ProjectDb,
    ManifestGscSite,
    ManifestUrl,
    ManifestSiteUrl,
    SeoWorkspace,
}

impl SiteUrlSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectDb => "projects.site_url",
            Self::ManifestGscSite => "manifest.gsc_site",
            Self::ManifestUrl => "manifest.url",
            Self::ManifestSiteUrl => "manifest.site_url",
            Self::SeoWorkspace => "seo_workspace.site_url",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteUrlCandidate {
    pub source: SiteUrlSource,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectSiteUrlSync {
    pub project_id: String,
    pub project_name: String,
    pub path: String,
    /// All non-empty values discovered before the write.
    pub candidates: Vec<SiteUrlCandidate>,
    /// Winner written (or already present).
    pub site_url: Option<String>,
    pub sitemap_url: Option<String>,
    pub site_url_source: Option<SiteUrlSource>,
    pub changed: bool,
    /// Human-readable notes (conflicts, skipped, etc.).
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteUrlSyncReport {
    pub projects_scanned: usize,
    pub projects_updated: usize,
    pub projects_already_ok: usize,
    pub projects_missing: usize,
    pub results: Vec<ProjectSiteUrlSync>,
}

/// Sync one registered project: gather → pick → write `projects.site_url`.
pub fn sync_project_site_url(conn: &Connection, project: &Project) -> Result<ProjectSiteUrlSync> {
    let paths = ProjectPaths::from_project(project);
    let candidates = collect_candidates(project, &paths);
    let sitemap_candidates = collect_sitemap_candidates(project, &paths);

    let mut notes = Vec::new();
    let distinct: Vec<String> = {
        let mut v: Vec<String> = candidates.iter().map(|c| c.value.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    if distinct.len() > 1 {
        notes.push(format!(
            "conflicting site URL values found: {}",
            distinct.join(" | ")
        ));
    }

    let Some(winner) = pick_site_url(&candidates) else {
        return Ok(ProjectSiteUrlSync {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            path: project.path.clone(),
            candidates,
            site_url: None,
            sitemap_url: project
                .sitemap_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
            site_url_source: None,
            changed: false,
            notes: {
                notes.push(
                    "no site_url found in projects table, manifest.json, or seo_workspace.json"
                        .into(),
                );
                notes
            },
        });
    };

    if let Err(e) = validate_site_url(&winner.value) {
        notes.push(format!("winner rejected by validate_site_url: {e}"));
        return Ok(ProjectSiteUrlSync {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            path: project.path.clone(),
            candidates,
            site_url: project.site_url.clone(),
            sitemap_url: project.sitemap_url.clone(),
            site_url_source: None,
            changed: false,
            notes,
        });
    }

    let sitemap_url = pick_sitemap(&sitemap_candidates, &winner.value);
    let prev_site = project
        .site_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let prev_sitemap = project
        .sitemap_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let site_changed = prev_site != Some(winner.value.as_str());
    let sitemap_changed = match (&sitemap_url, prev_sitemap) {
        (Some(next), Some(prev)) => next != prev,
        (Some(_), None) => true,
        (None, _) => false,
    };

    if !site_changed && !sitemap_changed {
        notes.push(format!(
            "already canonical (source={})",
            winner.source.as_str()
        ));
        return Ok(ProjectSiteUrlSync {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            path: project.path.clone(),
            candidates,
            site_url: Some(winner.value),
            sitemap_url: prev_sitemap.map(String::from).or(sitemap_url),
            site_url_source: Some(winner.source),
            changed: false,
            notes,
        });
    }

    let mut updated = project.clone();
    updated.site_url = Some(winner.value.clone());
    if let Some(ref sm) = sitemap_url {
        updated.sitemap_url = Some(sm.clone());
    }
    task_store::update_project(conn, &updated)?;

    if site_changed {
        notes.push(format!(
            "projects.site_url: {} → {} (from {})",
            prev_site.unwrap_or("(empty)"),
            winner.value,
            winner.source.as_str()
        ));
    }
    if sitemap_changed {
        notes.push(format!(
            "projects.sitemap_url: {} → {}",
            prev_sitemap.unwrap_or("(empty)"),
            sitemap_url.as_deref().unwrap_or("")
        ));
    }

    Ok(ProjectSiteUrlSync {
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        path: project.path.clone(),
        candidates,
        site_url: Some(winner.value),
        sitemap_url: sitemap_url.or_else(|| prev_sitemap.map(String::from)),
        site_url_source: Some(winner.source),
        changed: true,
        notes,
    })
}

/// Sync every registered project. Idempotent.
pub fn sync_all_site_urls(conn: &Connection) -> Result<SiteUrlSyncReport> {
    let projects = task_store::list_projects(conn)?;
    let mut results = Vec::with_capacity(projects.len());
    let mut updated = 0usize;
    let mut already_ok = 0usize;
    let mut missing = 0usize;

    for project in &projects {
        let row = sync_project_site_url(conn, project)?;
        if row.site_url.is_none() {
            missing += 1;
        } else if row.changed {
            updated += 1;
        } else {
            already_ok += 1;
        }
        results.push(row);
    }

    Ok(SiteUrlSyncReport {
        projects_scanned: projects.len(),
        projects_updated: updated,
        projects_already_ok: already_ok,
        projects_missing: missing,
        results,
    })
}

/// Sync a single project by id.
pub fn sync_site_url_for_id(conn: &Connection, project_id: &str) -> Result<ProjectSiteUrlSync> {
    let project = task_store::get_project(conn, project_id)?;
    sync_project_site_url(conn, &project)
}

// ─── Collection / pick ───────────────────────────────────────────────────────

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|v| !v.is_empty()).map(String::from)
}

fn collect_candidates(project: &Project, paths: &ProjectPaths) -> Vec<SiteUrlCandidate> {
    let mut out = Vec::new();

    if let Some(v) = non_empty(project.site_url.as_deref()) {
        out.push(SiteUrlCandidate {
            source: SiteUrlSource::ProjectDb,
            value: v,
        });
    }

    if let Some(manifest) = read_json(&paths.automation_dir.join("manifest.json")) {
        if let Some(v) = json_str(&manifest, "gsc_site") {
            out.push(SiteUrlCandidate {
                source: SiteUrlSource::ManifestGscSite,
                value: v,
            });
        }
        if let Some(v) = json_str(&manifest, "url") {
            out.push(SiteUrlCandidate {
                source: SiteUrlSource::ManifestUrl,
                value: v,
            });
        }
        if let Some(v) = json_str(&manifest, "site_url") {
            out.push(SiteUrlCandidate {
                source: SiteUrlSource::ManifestSiteUrl,
                value: v,
            });
        }
    }

    if let Some(ws) = read_json(&paths.automation_dir.join("seo_workspace.json")) {
        if let Some(v) = json_str(&ws, "site_url") {
            out.push(SiteUrlCandidate {
                source: SiteUrlSource::SeoWorkspace,
                value: v,
            });
        }
    }

    out
}

fn collect_sitemap_candidates(project: &Project, paths: &ProjectPaths) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = non_empty(project.sitemap_url.as_deref()) {
        out.push(v);
    }
    if let Some(manifest) = read_json(&paths.automation_dir.join("manifest.json")) {
        if let Some(v) = json_str(&manifest, "sitemap").or_else(|| json_str(&manifest, "sitemap_url"))
        {
            out.push(v);
        }
    }
    if let Some(ws) = read_json(&paths.automation_dir.join("seo_workspace.json")) {
        if let Some(v) = json_str(&ws, "sitemap").or_else(|| json_str(&ws, "sitemap_url")) {
            out.push(v);
        }
    }
    out
}

/// Pick the best site_url candidate.
///
/// Priority:
/// 1. Any valid `sc-domain:…` (GSC domain property — preferred API form)
/// 2. Existing `projects.site_url`
/// 3. manifest `gsc_site` → `url` → `site_url`
/// 4. seo_workspace `site_url`
fn pick_site_url(candidates: &[SiteUrlCandidate]) -> Option<SiteUrlCandidate> {
    if candidates.is_empty() {
        return None;
    }

    let valid: Vec<&SiteUrlCandidate> = candidates
        .iter()
        .filter(|c| validate_site_url(&c.value).is_ok())
        .collect();
    let pool = if valid.is_empty() {
        candidates.iter().collect::<Vec<_>>()
    } else {
        valid
    };

    // Prefer GSC domain property form.
    if let Some(c) = pool.iter().find(|c| c.value.starts_with("sc-domain:")) {
        return Some((*c).clone());
    }

    // Prefer existing DB value when present.
    if let Some(c) = pool.iter().find(|c| c.source == SiteUrlSource::ProjectDb) {
        return Some((*c).clone());
    }

    // Manifest field priority, then workspace.
    for source in [
        SiteUrlSource::ManifestGscSite,
        SiteUrlSource::ManifestUrl,
        SiteUrlSource::ManifestSiteUrl,
        SiteUrlSource::SeoWorkspace,
    ] {
        if let Some(c) = pool.iter().find(|c| c.source == source) {
            return Some((*c).clone());
        }
    }

    pool.first().map(|c| (*c).clone())
}

fn pick_sitemap(candidates: &[String], site_url: &str) -> Option<String> {
    if let Some(first) = candidates.first() {
        return Some(first.clone());
    }
    let base = site_base_url(site_url);
    if base.is_empty() {
        None
    } else {
        Some(format!("{base}/sitemap.xml"))
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|u| u.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::project::ProjectMode;
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

    fn with_temp_db<F, R>(f: F) -> R
    where
        F: FnOnce(&Connection) -> R,
    {
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        let dir = unique_temp_dir("site_url_sync_db");
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

    fn insert_project(conn: &Connection, id: &str, path: &str, site_url: Option<&str>) -> Project {
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
        task_store::create_project(conn, &project).unwrap()
    }

    fn write_json(path: &std::path::Path, value: serde_json::Value) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }

    #[test]
    fn pick_prefers_sc_domain_over_https() {
        let candidates = vec![
            SiteUrlCandidate {
                source: SiteUrlSource::ProjectDb,
                value: "https://example.com".into(),
            },
            SiteUrlCandidate {
                source: SiteUrlSource::ManifestGscSite,
                value: "sc-domain:example.com".into(),
            },
        ];
        let winner = pick_site_url(&candidates).unwrap();
        assert_eq!(winner.value, "sc-domain:example.com");
    }

    #[test]
    fn pick_prefers_db_when_no_sc_domain() {
        let candidates = vec![
            SiteUrlCandidate {
                source: SiteUrlSource::SeoWorkspace,
                value: "https://ws.example".into(),
            },
            SiteUrlCandidate {
                source: SiteUrlSource::ProjectDb,
                value: "https://db.example".into(),
            },
            SiteUrlCandidate {
                source: SiteUrlSource::ManifestUrl,
                value: "https://manifest.example".into(),
            },
        ];
        let winner = pick_site_url(&candidates).unwrap();
        assert_eq!(winner.value, "https://db.example");
    }

    #[test]
    fn sync_fills_db_from_manifest_when_empty() {
        with_temp_db(|conn| {
            let root = unique_temp_dir("site_url_sync_manifest");
            let automation = root.join(".github").join("automation");
            write_json(
                &automation.join("manifest.json"),
                serde_json::json!({
                    "gsc_site": "sc-domain:from-manifest.com",
                    "sitemap": "https://from-manifest.com/sitemap.xml"
                }),
            );
            let project = insert_project(conn, "p1", root.to_str().unwrap(), None);

            let row = sync_project_site_url(conn, &project).unwrap();
            assert!(row.changed);
            assert_eq!(row.site_url.as_deref(), Some("sc-domain:from-manifest.com"));
            assert_eq!(
                row.sitemap_url.as_deref(),
                Some("https://from-manifest.com/sitemap.xml")
            );

            let reloaded = task_store::get_project(conn, "p1").unwrap();
            assert_eq!(
                reloaded.site_url.as_deref(),
                Some("sc-domain:from-manifest.com")
            );
            assert_eq!(
                reloaded.sitemap_url.as_deref(),
                Some("https://from-manifest.com/sitemap.xml")
            );

            // Idempotent second run.
            let again = sync_project_site_url(conn, &reloaded).unwrap();
            assert!(!again.changed);

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn sync_upgrades_db_https_to_manifest_sc_domain() {
        with_temp_db(|conn| {
            let root = unique_temp_dir("site_url_sync_upgrade");
            let automation = root.join(".github").join("automation");
            write_json(
                &automation.join("manifest.json"),
                serde_json::json!({ "gsc_site": "sc-domain:example.com" }),
            );
            let project = insert_project(
                conn,
                "p2",
                root.to_str().unwrap(),
                Some("https://example.com"),
            );

            let row = sync_project_site_url(conn, &project).unwrap();
            assert!(row.changed);
            assert_eq!(row.site_url.as_deref(), Some("sc-domain:example.com"));

            let reloaded = task_store::get_project(conn, "p2").unwrap();
            assert_eq!(reloaded.site_url.as_deref(), Some("sc-domain:example.com"));

            let _ = std::fs::remove_dir_all(&root);
        });
    }

    #[test]
    fn sync_all_reports_missing() {
        with_temp_db(|conn| {
            let root = unique_temp_dir("site_url_sync_missing");
            std::fs::create_dir_all(&root).unwrap();
            insert_project(conn, "empty", root.to_str().unwrap(), None);

            let report = sync_all_site_urls(conn).unwrap();
            assert_eq!(report.projects_scanned, 1);
            assert_eq!(report.projects_missing, 1);
            assert_eq!(report.projects_updated, 0);

            let _ = std::fs::remove_dir_all(&root);
        });
    }
}
