use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
// ─── Helpers ─────────────────────────────────────────────────────────────────

pub(crate) fn read_workspace_config(path: &Path) -> (bool, Option<SeoWorkspaceConfig>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (false, None);
    };
    match serde_json::from_str::<SeoWorkspaceConfig>(&text) {
        Ok(cfg) => (true, Some(cfg)),
        Err(e) => {
            log::warn!(
                "[setup_check] seo_workspace.json parse error at {}: {}",
                path.display(),
                e
            );
            (true, None)
        }
    }
}

pub(crate) fn file_has_non_whitespace_content(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

pub(crate) fn manifest_configured(path: &Path) -> (bool, String) {
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

pub(crate) fn path_strings(path: &Path) -> (String, String) {
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

pub(crate) fn resolve_possibly_relative(path_str: &str, base: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

pub(crate) fn count_markdown(dir: &Path) -> usize {
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

/// Auto-discover content directory by scanning standard candidate paths.
/// Returns the first candidate that contains markdown files, or the first candidate
/// that exists (even if empty), or a default path if none exist.
pub fn auto_discover_content_dir(repo_root: &Path) -> Option<PathBuf> {
    // First pass: find candidate with markdown files
    for candidate in CANDIDATES {
        let p = repo_root.join(candidate);
        let count = count_markdown(&p);
        if count > 0 {
            return Some(p);
        }
    }

    // Second pass: find any candidate that exists (even if empty)
    for candidate in CANDIDATES {
        let p = repo_root.join(candidate);
        if p.exists() && p.is_dir() {
            return Some(p);
        }
    }

    // No candidates found - return None
    None
}

