use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
use super::types::ProjectSetup;
// ─── Standard candidate paths for auto-discovery ─────────────────────────────

pub(crate) const CANDIDATES: &[&str] = &[
    "src/blog/posts",
    "src/content/blog",
    "src/content",
    "webapp/content/blog",
    "content/blog",
    "content",
    "posts",
    "blog",
];

// ─── Resolution logic ─────────────────────────────────────────────────────────

/// Resolve the project setup from a repo root path and optional project-level
/// content_dir override (from the projects table, legacy support).
pub fn resolve(
    project_id: &str,
    repo_root_str: &str,
    project_content_dir_override: Option<&str>,
) -> ProjectSetup {
    let repo_root = PathBuf::from(repo_root_str);
    let automation_dir = repo_root.join(".github").join("automation");
    let workspace_config_path = automation_dir.join("seo_workspace.json");
    let articles_json = automation_dir.join("articles.json");

    // 1. Read seo_workspace.json
    let (workspace_config_exists, workspace_config) = read_workspace_config(&workspace_config_path);

    // 2. Resolve articles.json
    let articles_json_exists = articles_json.exists();

    // 3. Resolve content directory (priority order)
    let content_dir = resolve_content_dir(
        &repo_root,
        &automation_dir,
        workspace_config.as_ref(),
        project_content_dir_override,
    );

    // 4. Run all checks
    let mut checks: Vec<SetupCheckItem> = Vec::new();

    check_automation_dir(&automation_dir, &mut checks);
    check_articles_json(&articles_json, articles_json_exists, &mut checks);
    check_workspace_config(
        &workspace_config_path,
        workspace_config_exists,
        workspace_config.as_ref(),
        &content_dir,
        &mut checks,
    );
    check_content_dir(&content_dir, workspace_config_exists, &mut checks);
    check_clis(&mut checks);
    check_secrets(&repo_root, &mut checks);
    check_optional_config_files(&automation_dir, &mut checks);

    let is_valid = checks.iter().all(|c| c.severity != Severity::Error);

    let summary = if is_valid {
        let warns = checks
            .iter()
            .filter(|c| c.severity == Severity::Warn)
            .count();
        if warns == 0 {
            "Project is fully configured".into()
        } else {
            format!("{} warning{}", warns, if warns == 1 { "" } else { "s" })
        }
    } else {
        let errors = checks
            .iter()
            .filter(|c| c.severity == Severity::Error)
            .count();
        format!(
            "{} setup error{} must be fixed",
            errors,
            if errors == 1 { "" } else { "s" }
        )
    };

    ProjectSetup {
        project_id: project_id.to_string(),
        repo_root,
        automation_dir,
        workspace_config_path,
        workspace_config_exists,
        workspace_config,
        articles_json_exists,
        content_dir,
        checks,
        is_valid,
        summary,
    }
}

// ─── Content dir resolution (used by sync_and_validate) ──────────────────────

/// Resolve content directory.  Call this instead of any ad-hoc path scanning.
///
/// Priority:
/// 1. `seo_workspace.json#content_dir`  — most explicit
/// 2. `project.content_dir` column      — legacy per-project override
/// 3. Standard candidate auto-discovery
pub fn resolve_content_dir(
    repo_root: &Path,
    _automation_dir: &Path,
    workspace_config: Option<&SeoWorkspaceConfig>,
    project_override: Option<&str>,
) -> ContentDirResult {
    // 1. seo_workspace.json
    if let Some(cfg) = workspace_config {
        if let Some(ref cd) = cfg.content_dir {
            let cd = cd.trim();
            if !cd.is_empty() {
                let p = resolve_possibly_relative(cd, repo_root);
                let count = count_markdown(&p);
                return ContentDirResult {
                    source: ContentDirSource::WorkspaceConfig,
                    how: format!("seo_workspace.json → {}", p.display()),
                    file_count: count,
                    path: Some(p),
                };
            }
        }
    }

    // 2. Project-level override (legacy)
    if let Some(ov) = project_override {
        let ov = ov.trim();
        if !ov.is_empty() {
            let p = resolve_possibly_relative(ov, repo_root);
            let count = count_markdown(&p);
            return ContentDirResult {
                source: ContentDirSource::ProjectOverride,
                how: format!("project setting → {}", p.display()),
                file_count: count,
                path: Some(p),
            };
        }
    }

    // 3. Auto-discover
    for candidate in CANDIDATES {
        let p = repo_root.join(candidate);
        let count = count_markdown(&p);
        if count > 0 {
            return ContentDirResult {
                source: ContentDirSource::AutoDiscovered,
                how: format!("auto-discovered → {}", p.display()),
                file_count: count,
                path: Some(p),
            };
        }
    }

    ContentDirResult {
        source: ContentDirSource::NotFound,
        path: None,
        how: format!(
            "not found — searched {} candidate paths under {}",
            CANDIDATES.len(),
            repo_root.display()
        ),
        file_count: 0,
    }
}

