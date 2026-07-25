/// Build a complete list of known project config files and whether they are configured.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
pub fn collect_config_file_statuses(
    repo_root_str: &str,
    project_content_dir_override: Option<&str>,
) -> Vec<ProjectConfigFileStatus> {
    let repo_root = PathBuf::from(repo_root_str);
    let automation_dir = repo_root.join(".github").join("automation");
    let workspace_config_path = automation_dir.join("seo_workspace.json");
    let (workspace_config_exists, workspace_config) = read_workspace_config(&workspace_config_path);
    let _ = project_content_dir_override;

    let mut files: Vec<ProjectConfigFileStatus> = Vec::new();

    // Core SEO workspace files
    {
        let path = automation_dir.join("seo_workspace.json");
        let (full_path, full_link) = path_strings(&path);
        let configured = workspace_config_exists
            && workspace_config
                .as_ref()
                .and_then(|cfg| cfg.content_dir.as_ref())
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);
        let detail = if !path.exists() {
            "Missing file".to_string()
        } else if configured {
            "Configured (content_dir present)".to_string()
        } else {
            "File exists but content_dir is missing/empty".to_string()
        };
        files.push(ProjectConfigFileStatus {
            id: "seo_workspace".to_string(),
            category: "seo".to_string(),
            label: "SEO workspace".to_string(),
            relative_path: ".github/automation/seo_workspace.json".to_string(),
            full_path,
            full_link,
            used_by: "SEO content workflows".to_string(),
            required: true,
            configured,
            detail,
        });
    }

    {
        let path = automation_dir.join("articles.json");
        let (full_path, full_link) = path_strings(&path);
        files.push(ProjectConfigFileStatus {
            id: "articles_json".to_string(),
            category: "seo".to_string(),
            label: "Article index".to_string(),
            relative_path: ".github/automation/articles.json".to_string(),
            full_path,
            full_link,
            used_by: "SEO research, content review, publishing".to_string(),
            required: true,
            configured: path.exists(),
            detail: if path.exists() {
                "Present".to_string()
            } else {
                "Missing file".to_string()
            },
        });
    }

    {
        let path = automation_dir.join("task_list.json");
        let (full_path, full_link) = path_strings(&path);
        files.push(ProjectConfigFileStatus {
            id: "task_list_json".to_string(),
            category: "workflow".to_string(),
            label: "Task list export".to_string(),
            relative_path: ".github/automation/task_list.json".to_string(),
            full_path,
            full_link,
            used_by: "Task import/export compatibility".to_string(),
            required: false,
            configured: path.exists(),
            detail: if path.exists() {
                "Present".to_string()
            } else {
                "Optional file missing".to_string()
            },
        });
    }

    // GSC / analytics files
    {
        let path = automation_dir.join("manifest.json");
        let (full_path, full_link) = path_strings(&path);
        let (configured, detail) = manifest_configured(&path);
        files.push(ProjectConfigFileStatus {
            id: "manifest_json".to_string(),
            category: "gsc".to_string(),
            label: "Site manifest".to_string(),
            relative_path: ".github/automation/manifest.json".to_string(),
            full_path,
            full_link,
            used_by: "GSC collection and sync".to_string(),
            required: false,
            configured,
            detail,
        });
    }

    // Shared context / sentiment style files (consolidated project.md)
    {
        let path = automation_dir.join("project.md");
        let (full_path, full_link) = path_strings(&path);
        let has_project_md = path.exists() && file_has_non_whitespace_content(&path);

        // Check for legacy files as fallback
        let legacy_summary = automation_dir.join("project_summary.md");
        let legacy_brand = automation_dir.join("brandvoice.md");
        let has_legacy = (legacy_summary.exists()
            && file_has_non_whitespace_content(&legacy_summary))
            || (legacy_brand.exists() && file_has_non_whitespace_content(&legacy_brand));

        let configured = has_project_md || has_legacy;
        let detail = if has_project_md {
            "Present (consolidated)".to_string()
        } else if has_legacy {
            "Legacy files detected — consider migrating to project.md".to_string()
        } else {
            "Optional file missing".to_string()
        };

        files.push(ProjectConfigFileStatus {
            id: "project".to_string(),
            category: "context".to_string(),
            label: "Project context".to_string(),
            relative_path: ".github/automation/project.md".to_string(),
            full_path,
            full_link,
            used_by: "SEO + Reddit prompt context".to_string(),
            required: false,
            configured,
            detail,
        });
    }

    // Reddit-specific files
    {
        let path = automation_dir.join("reddit_config.md");
        let (full_path, full_link) = path_strings(&path);
        let configured = path.exists() && file_has_non_whitespace_content(&path);
        files.push(ProjectConfigFileStatus {
            id: "reddit_config".to_string(),
            category: "reddit".to_string(),
            label: "Reddit config".to_string(),
            relative_path: ".github/automation/reddit_config.md".to_string(),
            full_path,
            full_link,
            used_by: "Reddit opportunity search".to_string(),
            required: false,
            configured,
            detail: if !path.exists() {
                "Optional file missing".to_string()
            } else if configured {
                "Present".to_string()
            } else {
                "File is empty".to_string()
            },
        });
    }

    {
        let path = automation_dir.join("reddit").join("_reply_guardrails.md");
        let (full_path, full_link) = path_strings(&path);
        let configured = path.exists() && file_has_non_whitespace_content(&path);
        files.push(ProjectConfigFileStatus {
            id: "reddit_reply_guardrails".to_string(),
            category: "reddit".to_string(),
            label: "Reddit reply guardrails".to_string(),
            relative_path: ".github/automation/reddit/_reply_guardrails.md".to_string(),
            full_path,
            full_link,
            used_by: "Reddit reply safety and style constraints".to_string(),
            required: false,
            configured,
            detail: if !path.exists() {
                "Optional file missing".to_string()
            } else if configured {
                "Present".to_string()
            } else {
                "File is empty".to_string()
            },
        });
    }

    files
}

// ─── Template ─────────────────────────────────────────────────────────────────

/// Canonical `seo_workspace.json` template.
/// Written when the user asks the app to initialise the config for them.
pub fn workspace_config_template(content_dir_hint: &str, site_url_hint: &str) -> String {
    format!(
        r#"{{
  "content_dir": "{}",
  "site_url": "{}"
}}
"#,
        content_dir_hint, site_url_hint
    )
}

