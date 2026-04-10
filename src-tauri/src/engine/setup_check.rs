/// Project setup validation — single source of truth for understanding a project's state.
///
/// Every piece of engine code that needs to locate content, articles.json, or the
/// automation workspace should go through `ProjectSetup::resolve()`.  The returned
/// struct is fully serialisable so it can be sent directly to the UI, which can
/// then surface actionable warnings to the user.
///
/// # Workspace layout expected
/// ```text
/// <repo_root>/
///   .github/
///     automation/
///       articles.json          ← required for content workflows
///       task_list.json
///       seo_workspace.json     ← optional but strongly recommended; pins content_dir
/// ```
///
/// # seo_workspace.json format
/// ```json
/// {
///   "content_dir": "src/blog/posts",
///   "site_url": "https://example.com"
/// }
/// ```
///  `content_dir` is a path relative to `repo_root` (or absolute).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ─── Public config struct ─────────────────────────────────────────────────────

/// Deserialized form of `seo_workspace.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SeoWorkspaceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
}

// ─── Resolution result ────────────────────────────────────────────────────────

/// How a content directory was resolved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentDirSource {
    /// Explicitly set in `seo_workspace.json`.
    WorkspaceConfig,
    /// Inferred from the `content_dir` column on the projects table (legacy).
    ProjectOverride,
    /// Found by probing standard candidate paths under `repo_root`.
    AutoDiscovered,
    /// Not found.
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ContentDirResult {
    pub source: ContentDirSource,
    /// Absolute resolved path, if found.
    pub path: Option<PathBuf>,
    /// Human-readable label explaining how (or why not) the directory was found.
    pub how: String,
    /// Number of markdown files found (0 means directory exists but is empty).
    pub file_count: usize,
}

// ─── Check severity ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Workflow will fail without this.
    Error,
    /// Workflow will run but may produce incomplete/incorrect results.
    Warn,
    /// Informational — nothing is broken.
    Info,
}

// ─── Individual check ─────────────────────────────────────────────────────────

/// One discrete check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetupCheckItem {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    /// Short instruction for what the user should do to fix it.
    pub fix_hint: Option<String>,
    /// Whether the app can auto-fix this (e.g. create the config file).
    pub auto_fixable: bool,
}

// ─── Full setup result ────────────────────────────────────────────────────────

/// Complete setup diagnostic for a project.  Returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSetup {
    pub project_id: String,
    pub repo_root: PathBuf,
    pub automation_dir: PathBuf,
    pub workspace_config_path: PathBuf,
    pub workspace_config_exists: bool,
    pub workspace_config: Option<SeoWorkspaceConfig>,
    pub articles_json_exists: bool,
    pub content_dir: ContentDirResult,
    pub checks: Vec<SetupCheckItem>,
    /// `true` if there are no Error-severity checks.
    pub is_valid: bool,
    /// Summary message for quick display.
    pub summary: String,
}

// ─── Config file status (Settings page) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfigFileStatus {
    pub id: String,
    pub category: String,
    pub label: String,
    /// Path relative to repo root (e.g. ".github/automation/manifest.json").
    pub relative_path: String,
    /// Absolute path on disk.
    pub full_path: String,
    /// File URL form of `full_path`.
    pub full_link: String,
    pub used_by: String,
    pub required: bool,
    pub configured: bool,
    pub detail: String,
}

/// Build a complete list of known project config files and whether they are configured.
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
        let has_legacy = (legacy_summary.exists() && file_has_non_whitespace_content(&legacy_summary))
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

// ─── Standard candidate paths for auto-discovery ─────────────────────────────

const CANDIDATES: &[&str] = &[
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

    let is_valid = checks.iter().all(|c| c.severity != Severity::Error);

    let summary = if is_valid {
        let warns = checks.iter().filter(|c| c.severity == Severity::Warn).count();
        if warns == 0 {
            "Project is fully configured".into()
        } else {
            format!("{} warning{}", warns, if warns == 1 { "" } else { "s" })
        }
    } else {
        let errors = checks.iter().filter(|c| c.severity == Severity::Error).count();
        format!("{} setup error{} must be fixed", errors, if errors == 1 { "" } else { "s" })
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

// ─── Individual checks ────────────────────────────────────────────────────────

fn check_automation_dir(automation_dir: &Path, checks: &mut Vec<SetupCheckItem>) {
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
                "Run: mkdir -p .github/automation && pageseeds automation repo init".into(),
            ),
            auto_fixable: false,
        });
    }
}

fn check_articles_json(
    path: &Path,
    exists: bool,
    checks: &mut Vec<SetupCheckItem>,
) {
    if !exists {
        checks.push(SetupCheckItem {
            id: "articles_json_missing".into(),
            severity: Severity::Error,
            title: "articles.json not found".into(),
            detail: format!(
                "Expected at {} — content workflows cannot run without it",
                path.display()
            ),
            fix_hint: Some(
                "Import articles.json via the Articles tab, or copy it into .github/automation/".into(),
            ),
            auto_fixable: false,
        });
    }
}

fn check_workspace_config(
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
        if cfg.content_dir.as_deref().map(str::trim).unwrap_or("").is_empty() {
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

fn check_content_dir(content_dir: &ContentDirResult, workspace_config_exists: bool, checks: &mut Vec<SetupCheckItem>) {
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
                    content_dir.path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
                ),
                fix_hint: Some("Add \"content_dir\" to seo_workspace.json to pin this permanently".into()),
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
                    content_dir.path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
                ),
                fix_hint: Some("Check that the path in seo_workspace.json points to the correct folder".into()),
                auto_fixable: false,
            });
        }
        _ => {}
    }
}

// ─── CLI availability ───────────────────────────────────────────────────────

/// CLIs that the app launches as subprocesses.
const REQUIRED_CLIS: &[(&str, &str)] = &[
    (
        "seo-content-cli",
        "Content workflows (keyword research, article planning)",
    ),
];

fn check_clis(checks: &mut Vec<SetupCheckItem>) {
    for (bin, desc) in REQUIRED_CLIS {
        if !is_on_path(bin) {
            checks.push(SetupCheckItem {
                id: format!("cli_missing_{}", bin.replace('-', "_")),
                severity: Severity::Error,
                title: format!("{} not found on PATH", bin),
                detail: format!("Required for: {}.", desc),
                fix_hint: Some(
                    "uv tool install git+https://github.com/fstrauf/pageseeds-cli"
                        .to_string(),
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

fn check_secrets(repo_root: &Path, checks: &mut Vec<SetupCheckItem>) {
    use crate::config::env_resolver::EnvResolver;
    let resolver = EnvResolver::new(repo_root);

    // GSC: either service account OR oauth client secrets
    let gsc_ok = resolver.resolve("GSC_SERVICE_ACCOUNT_PATH").is_some()
        || resolver.resolve("GSC_REPORT_OAUTH_CLIENT_SECRETS").is_some();
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

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn read_workspace_config(path: &Path) -> (bool, Option<SeoWorkspaceConfig>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (false, None);
    };
    match serde_json::from_str::<SeoWorkspaceConfig>(&text) {
        Ok(cfg) => (true, Some(cfg)),
        Err(e) => {
            log::warn!("[setup_check] seo_workspace.json parse error at {}: {}", path.display(), e);
            (true, None)
        }
    }
}

fn file_has_non_whitespace_content(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn manifest_configured(path: &Path) -> (bool, String) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (false, "Optional file missing".to_string());
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (false, "Invalid JSON".to_string());
    };
    let has_site = json
        .get("gsc_site")
        .or_else(|| json.get("url"))
        .and_then(|v| v.as_str())
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    if has_site {
        (true, "Configured (url/gsc_site present)".to_string())
    } else {
        (false, "Missing 'url' or 'gsc_site'".to_string())
    }
}

fn path_strings(path: &Path) -> (String, String) {
    let full_path = path.to_string_lossy().to_string();
    let full_link = format!("file://{}", full_path);
    (full_path, full_link)
}

/// Load the `seo_workspace.json` from `{automation_dir}/seo_workspace.json`.
/// Returns `None` if the file is absent or unparseable.
pub fn load_workspace_config(automation_dir: &Path) -> Option<SeoWorkspaceConfig> {
    let path = automation_dir.join("seo_workspace.json");
    read_workspace_config(&path).1
}

fn resolve_possibly_relative(path_str: &str, base: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

fn count_markdown(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let p = e.path();
            if !p.is_file() {
                return false;
            }
            p.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("mdx"))
                .unwrap_or(false)
        })
        .count()
}

/// Write seo_workspace.json from a template.
/// The caller should pass the best-guess content_dir (e.g. from auto-discovery).
pub fn write_workspace_config(
    automation_dir: &Path,
    content_dir: &str,
    site_url: &str,
) -> std::result::Result<PathBuf, String> {
    let path = automation_dir.join("seo_workspace.json");
    let content = workspace_config_template(content_dir, site_url);
    std::fs::create_dir_all(automation_dir)
        .map_err(|e| format!("Cannot create automation directory: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Cannot write seo_workspace.json: {}", e))?;
    Ok(path)
}
