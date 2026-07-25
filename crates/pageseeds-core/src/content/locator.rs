/// Content directory auto-discovery.
///
/// Mirrors `dashboard_ptk/dashboard/engine/content_locator.py`.
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Ordered candidate paths to probe when no override is configured.
const CANDIDATES: &[&str] = &[
    "webapp/content/blog",
    "src/blog/posts",
    "src/content",
    "content/blog",
    "content",
    "posts",
    "blog",
];

#[derive(Debug, Clone, Serialize)]
pub struct ContentDirResolution {
    /// Final selected directory (absolute), or None if nothing found.
    pub selected: Option<PathBuf>,
    /// How the selection was made: "configured" | "auto" | "none"
    pub source: String,
    pub has_markdown: bool,
    pub candidates_searched: Vec<PathBuf>,
}

/// Resolve the content directory for a project.
///
/// If `content_dir_override` is Some (from the projects table), it is tried
/// first. Falls back to `CANDIDATES` probed relative to `repo_root`.
pub fn resolve(repo_root: &Path, content_dir_override: Option<&str>) -> ContentDirResolution {
    let mut candidates_searched = Vec::new();

    // 1. Configured override
    if let Some(rel) = content_dir_override {
        let candidate = if Path::new(rel).is_absolute() {
            PathBuf::from(rel)
        } else {
            repo_root.join(rel)
        };
        let has_md = dir_has_markdown(&candidate);
        return ContentDirResolution {
            selected: Some(candidate.clone()),
            source: "configured".into(),
            has_markdown: has_md,
            candidates_searched: vec![candidate],
        };
    }

    // 2. Auto-discovery
    for rel in CANDIDATES {
        let candidate = repo_root.join(rel);
        candidates_searched.push(candidate.clone());
        if candidate.is_dir() && dir_has_markdown(&candidate) {
            return ContentDirResolution {
                selected: Some(candidate),
                source: "auto".into(),
                has_markdown: true,
                candidates_searched,
            };
        }
    }

    ContentDirResolution {
        selected: None,
        source: "none".into(),
        has_markdown: false,
        candidates_searched,
    }
}

/// Returns true if the directory contains at least one .md or .mdx file.
pub fn dir_has_markdown(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("mdx") {
                    return true;
                }
            }
        }
    }
    false
}

/// Collect all markdown files in a directory (non-recursive, sorted).
pub fn collect_markdown_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("mdx"))
                    .unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n != ".gitkeep")
                    .unwrap_or(true)
        })
        .collect();
    files.sort();
    files
}
