use std::path::{Path, PathBuf};

/// Result of resolving an article file path.
#[derive(Debug, Clone)]
pub struct ResolvedFile {
    /// The canonical path relative to repo root (no ./ prefix)
    pub relative_path: String,
    /// Absolute path on disk
    pub _absolute_path: PathBuf,
    /// Whether the file was found
    pub found: bool,
    /// Whether the path was repaired (different from stored)
    pub was_repaired: bool,
}

/// Resolve a stored file reference to an actual on-disk file.
///
/// `repo_root` is the project repository root.
/// `stored_path` is the path stored in articles.json / DB.
/// `content_dirs` are known content directories to search (e.g. ["content", "src/blog/posts"]).
pub fn resolve_article_file(
    repo_root: &Path,
    stored_path: &str,
    content_dirs: &[&str],
) -> ResolvedFile {
    if stored_path.is_empty() {
        return ResolvedFile {
            relative_path: String::new(),
            _absolute_path: PathBuf::new(),
            found: false,
            was_repaired: false,
        };
    }

    let clean = stored_path.strip_prefix("./").unwrap_or(stored_path);

    // 1. Try as-is (relative to repo root)
    let direct = repo_root.join(clean);
    if direct.exists() {
        return ResolvedFile {
            relative_path: clean.to_string(),
            _absolute_path: direct,
            found: true,
            was_repaired: clean != stored_path,
        };
    }

    // 2. Try with ./ prefix added back
    let with_prefix = repo_root.join(stored_path);
    if with_prefix.exists() {
        return ResolvedFile {
            relative_path: stored_path.to_string(),
            _absolute_path: with_prefix,
            found: true,
            was_repaired: false,
        };
    }

    // 3. Search by basename in known content directories
    let basename = Path::new(clean)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(stored_path);

    for dir in content_dirs {
        let candidate = repo_root.join(dir).join(basename);
        if candidate.exists() {
            let rel = candidate
                .strip_prefix(repo_root)
                .unwrap_or(candidate.as_path())
                .to_string_lossy()
                .to_string();
            return ResolvedFile {
                relative_path: rel,
                _absolute_path: candidate,
                found: true,
                was_repaired: true,
            };
        }
    }

    // 4. Fallback: repo-wide markdown search by basename
    if let Some(found) = find_file_by_basename(repo_root, basename) {
        let rel = found
            .strip_prefix(repo_root)
            .unwrap_or(found.as_path())
            .to_string_lossy()
            .to_string();
        return ResolvedFile {
            relative_path: rel,
            _absolute_path: found,
            found: true,
            was_repaired: true,
        };
    }

    // Not found anywhere
    ResolvedFile {
        relative_path: stored_path.to_string(),
        _absolute_path: direct,
        found: false,
        was_repaired: false,
    }
}

/// Find a file by basename anywhere under repo_root (limited depth for performance).
fn find_file_by_basename(repo_root: &Path, basename: &str) -> Option<PathBuf> {
    

    // Common content directories to check first (fast path)
    let common_dirs = ["content", "src", "posts", "blog", "articles", "docs"];
    for dir in &common_dirs {
        let dir_path = repo_root.join(dir);
        if dir_path.is_dir() {
            if let Some(found) = find_in_dir(&dir_path, basename) {
                return Some(found);
            }
        }
    }

    // Full repo walk (slower, but catches edge cases)
    find_in_dir_limited(repo_root, basename, 4)
}

fn find_in_dir(dir: &Path, basename: &str) -> Option<PathBuf> {
    use std::fs;

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if path.file_name().map(|n| n == basename).unwrap_or(false) {
                    return Some(path);
                }
            } else if path.is_dir() {
                if let Some(found) = find_in_dir(&path, basename) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_in_dir_limited(dir: &Path, basename: &str, depth: usize) -> Option<PathBuf> {
    use std::fs;

    if depth == 0 {
        return None;
    }

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if path.file_name().map(|n| n == basename).unwrap_or(false) {
                    return Some(path);
                }
            } else if path.is_dir() {
                // Skip hidden dirs and common non-content dirs
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                if let Some(found) = find_in_dir_limited(&path, basename, depth - 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Build a lookup of known content directories for a project.
///
/// Tries common locations and falls back to heuristics.
pub fn discover_content_dirs(repo_root: &Path) -> Vec<String> {
    let mut dirs = Vec::new();

    let candidates = [
        "content",
        "src/blog/posts",
        "src/posts",
        "src/blog",
        "posts",
        "blog",
        "articles",
        "docs",
    ];

    for candidate in &candidates {
        let path = repo_root.join(candidate);
        if path.is_dir() {
            // Verify it contains at least one .md or .mdx file
            if has_markdown_files(&path) {
                dirs.push(candidate.to_string());
            }
        }
    }

    dirs
}

fn has_markdown_files(dir: &Path) -> bool {
    use std::fs;

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "md" || ext == "mdx" {
                    return true;
                }
            } else if path.is_dir() {
                if has_markdown_files(&path) {
                    return true;
                }
            }
        }
    }
    false
}

/// Update articles.json and DB with repaired paths.
///
/// Returns (number_repaired, list_of_changes).
pub fn repair_article_paths_in_batch(
    repo_root: &Path,
    project_id: &str,
    conn: &rusqlite::Connection,
) -> Result<crate::models::article::RepairPathResult, String> {
    use crate::db::export;
    use crate::engine::task_store;
    use crate::models::article::RepairPathResult;

    let content_dirs = discover_content_dirs(repo_root);
    let content_dirs_refs: Vec<&str> = content_dirs.iter().map(|s| s.as_str()).collect();

    let articles = task_store::list_articles(conn, project_id)
        .map_err(|e| format!("Failed to list articles: {}", e))?;

    let mut checked = 0usize;
    let mut repaired = 0usize;
    let mut removed = 0usize;
    let mut not_found = Vec::new();

    for article in &articles {
        checked += 1;
        let resolved = resolve_article_file(repo_root, &article.file, &content_dirs_refs);
        if resolved.found && resolved.was_repaired {
            conn.execute(
                "UPDATE articles SET file = ?1 WHERE id = ?2 AND project_id = ?3",
                rusqlite::params![&resolved.relative_path, article.id, project_id],
            )
            .map_err(|e| format!("Failed to update article {}: {}", article.id, e))?;

            repaired += 1;
        } else if !resolved.found {
            not_found.push(article.file.clone());
            removed += 1;
        }
    }

    if repaired > 0 || removed > 0 {
        export::write_articles_to_repo(conn, project_id, repo_root)
            .map_err(|e| format!("Failed to export articles.json: {}", e))?;
    }

    Ok(RepairPathResult {
        checked,
        repaired,
        removed,
        not_found,
    })
}
