use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
// ─── Public config struct ─────────────────────────────────────────────────────

/// Deserialized form of `seo_workspace.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SeoWorkspaceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter_schema: Option<crate::content::validator::FrontmatterSchema>,
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

