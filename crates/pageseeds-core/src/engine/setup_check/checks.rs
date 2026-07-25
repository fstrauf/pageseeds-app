use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
// ─── Individual checks ────────────────────────────────────────────────────────

pub(crate) fn check_automation_dir(automation_dir: &Path, checks: &mut Vec<SetupCheckItem>) {
    if !automation_dir.exists() {
        checks.push(SetupCheckItem {
            id: "automation_dir_missing".into(),
            severity: Severity::Error,
            title: "Automation workspace missing".into(),
            detail: format!(
                ".github/automation/ does not exist at {}",
                automation_dir.display()
            ),
            fix_hint: Some(
                "Click 'Initialize Project' to create the workspace automatically".into(),
            ),
            auto_fixable: true, // Now fixable via initialize_project_workspace
        });
    }
}

pub(crate) fn check_articles_json(path: &Path, exists: bool, checks: &mut Vec<SetupCheckItem>) {
    if !exists {
        checks.push(SetupCheckItem {
            id: "articles_json_missing".into(),
            severity: Severity::Error,
            title: "articles.json not found".into(),
            detail: format!(
                "Expected at {} — content workflows cannot run without it",
                path.display()
            ),
            fix_hint: Some("Click 'Initialize Project' to create an empty articles.json".into()),
            auto_fixable: true, // Now fixable via initialize_project_workspace
        });
    }
}

pub(crate) fn check_workspace_config(
    path: &Path,
    exists: bool,
    config: Option<&SeoWorkspaceConfig>,
    _content_dir: &ContentDirResult,
    checks: &mut Vec<SetupCheckItem>,
) {
    if !exists {
        checks.push(SetupCheckItem {
            id: "workspace_config_missing".into(),
            severity: Severity::Warn,
            title: "seo_workspace.json not configured".into(),
            detail: format!(
                "Without it the app auto-discovers the content directory which may find the wrong path. \
                 Expected at {}",
                path.display()
            ),
            fix_hint: Some("Click 'Create config' to generate seo_workspace.json from the detected settings".into()),
            auto_fixable: true,
        });
        return;
    }

    if let Some(cfg) = config {
        if cfg
            .content_dir
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            checks.push(SetupCheckItem {
                id: "workspace_config_no_content_dir".into(),
                severity: Severity::Warn,
                title: "seo_workspace.json has no content_dir".into(),
                detail: "Add a \"content_dir\" field pointing to your content folder (e.g. \"src/blog/posts\")".into(),
                fix_hint: Some(format!("Edit {} and add: \"content_dir\": \"src/blog/posts\"", path.display())),
                auto_fixable: true,
            });
        }
    }
}

pub(crate) fn check_content_dir(
    content_dir: &ContentDirResult,
    workspace_config_exists: bool,
    checks: &mut Vec<SetupCheckItem>,
) {
    match content_dir.source {
        ContentDirSource::NotFound => {
            checks.push(SetupCheckItem {
                id: "content_dir_not_found".into(),
                severity: Severity::Error,
                title: "Content directory not found".into(),
                detail: format!("{}", content_dir.how),
                fix_hint: Some(
                    "Set content_dir in seo_workspace.json pointing to your markdown content folder".into(),
                ),
                auto_fixable: false,
            });
        }
        ContentDirSource::AutoDiscovered if workspace_config_exists => {
            checks.push(SetupCheckItem {
                id: "content_dir_auto_discovered".into(),
                severity: Severity::Warn,
                title: "Content directory auto-discovered (not pinned)".into(),
                detail: format!(
                    "Using {} — this may not be correct if multiple content folders exist",
                    content_dir
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                fix_hint: Some(
                    "Add \"content_dir\" to seo_workspace.json to pin this permanently".into(),
                ),
                auto_fixable: true,
            });
        }
        _ if content_dir.file_count == 0 => {
            checks.push(SetupCheckItem {
                id: "content_dir_empty".into(),
                severity: Severity::Warn,
                title: "Content directory is empty".into(),
                detail: format!(
                    "{} contains no .md/.mdx files",
                    content_dir
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                fix_hint: Some(
                    "Check that the path in seo_workspace.json points to the correct folder".into(),
                ),
                auto_fixable: false,
            });
        }
        _ => {}
    }
}

/// Check for optional but recommended config files
pub(crate) fn check_optional_config_files(automation_dir: &Path, checks: &mut Vec<SetupCheckItem>) {
    // Only check if automation dir exists - otherwise these checks don't make sense
    if !automation_dir.exists() {
        return;
    }

    // Check for project.md (or legacy files)
    let project_md = automation_dir.join("project.md");
    let legacy_summary = automation_dir.join("project_summary.md");
    let legacy_brand = automation_dir.join("brandvoice.md");

    if !project_md.exists() && !legacy_summary.exists() && !legacy_brand.exists() {
        checks.push(SetupCheckItem {
            id: "project_md_missing".into(),
            severity: Severity::Warn,
            title: "Project context not configured".into(),
            detail: "project.md is missing — AI prompts won't have project context".into(),
            fix_hint: Some("Click 'Initialize Project' to create a project.md template".into()),
            auto_fixable: true,
        });
    }

    // Check for reddit_config.md
    let reddit_config = automation_dir.join("reddit_config.md");
    if !reddit_config.exists() {
        checks.push(SetupCheckItem {
            id: "reddit_config_missing".into(),
            severity: Severity::Warn,
            title: "Reddit not configured".into(),
            detail: "reddit_config.md is missing — Reddit features won't work".into(),
            fix_hint: Some(
                "Click 'Initialize Project' to create a reddit_config.md template".into(),
            ),
            auto_fixable: true,
        });
    }

    // Check for reddit/_reply_guardrails.md
    let guardrails = automation_dir.join("reddit").join("_reply_guardrails.md");
    if !guardrails.exists() {
        checks.push(SetupCheckItem {
            id: "reply_guardrails_missing".into(),
            severity: Severity::Warn,
            title: "Reply guardrails not configured".into(),
            detail: "reddit/_reply_guardrails.md is missing — reply safety guidelines not set"
                .into(),
            fix_hint: Some("Click 'Initialize Project' to create guardrails template".into()),
            auto_fixable: true,
        });
    }
}

// ─── CLI availability ───────────────────────────────────────────────────────

/// CLIs that the app launches as subprocesses.
const REQUIRED_CLIS: &[(&str, &str)] = &[(
    "seo-content-cli",
    "Content workflows (keyword research, article planning)",
)];

pub(crate) fn check_clis(checks: &mut Vec<SetupCheckItem>) {
    for (bin, desc) in REQUIRED_CLIS {
        if !is_on_path(bin) {
            checks.push(SetupCheckItem {
                id: format!("cli_missing_{}", bin.replace('-', "_")),
                severity: Severity::Error,
                title: format!("{} not found on PATH", bin),
                detail: format!("Required for: {}.", desc),
                fix_hint: Some(
                    "uv tool install git+https://github.com/fstrauf/pageseeds-cli".to_string(),
                ),
                auto_fixable: false,
            });
        }
    }
}

fn is_on_path(binary: &str) -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var).any(|dir| {
        let candidate = dir.join(binary);
        if candidate.exists() {
            return true;
        }
        #[cfg(target_os = "windows")]
        if dir.join(format!("{}.exe", binary)).exists() {
            return true;
        }
        false
    })
}

// ─── Secrets checks ──────────────────────────────────────────────────────────

pub(crate) fn check_secrets(repo_root: &Path, checks: &mut Vec<SetupCheckItem>) {
    use crate::config::env_resolver::EnvResolver;
    let resolver = EnvResolver::new(repo_root);

    // GSC: either service account OR oauth client secrets
    let gsc_ok = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH").is_some()
        || resolver
            .resolve("GSC_REPORT_OAUTH_CLIENT_SECRETS")
            .is_some();
    if !gsc_ok {
        checks.push(SetupCheckItem {
            id: "secret_gsc_missing".into(),
            severity: Severity::Warn,
            title: "Google Search Console not configured".into(),
            detail: "GSC_SERVICE_ACCOUNT_PATH or GSC_REPORT_OAUTH_CLIENT_SECRETS is required for GSC features.".into(),
            fix_hint: Some("Add credentials to ~/.config/automation/secrets.env".into()),
            auto_fixable: false,
        });
    }

    // Reddit: both client creds needed for API access
    let reddit_ok = resolver.resolve("REDDIT_CLIENT_ID").is_some()
        && resolver.resolve("REDDIT_CLIENT_SECRET").is_some();
    if !reddit_ok {
        checks.push(SetupCheckItem {
            id: "secret_reddit_api_missing".into(),
            severity: Severity::Warn,
            title: "Reddit API credentials not configured".into(),
            detail: "REDDIT_CLIENT_ID and REDDIT_CLIENT_SECRET are needed for Reddit opportunity search.".into(),
            fix_hint: Some("Add credentials to ~/.config/automation/secrets.env".into()),
            auto_fixable: false,
        });
    }

    // Ahrefs traffic / keyword difficulty
    if resolver.resolve("CAPSOLVER_API_KEY").is_none() {
        checks.push(SetupCheckItem {
            id: "secret_capsolver_missing".into(),
            severity: Severity::Warn,
            title: "Ahrefs keyword research not configured".into(),
            detail: "CAPSOLVER_API_KEY is needed for keyword difficulty analysis.".into(),
            fix_hint: Some("Add CAPSOLVER_API_KEY to ~/.config/automation/secrets.env".into()),
            auto_fixable: false,
        });
    }
}

