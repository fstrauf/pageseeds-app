//! Shared merge apply primitives used by both Path B (`merge_package::submit_merge`)
//! and desktop `consolidate_cluster` steps.
//!
//! Single source of truth for:
//! - plan lookup from a consolidate task
//! - redirects.csv upsert
//! - inbound link rewrite to keeper
//! - depublish redirect sources (fail-closed)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::content::slug::{format_blog_link, normalize_url_slug};
use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

// ─── Plan load ───────────────────────────────────────────────────────────────

/// Load the merge-plan recommendation JSON for a `consolidate_cluster` task.
///
/// Looks up `cannibalization_strategy` (task artifact, else automation file),
/// matches `cluster_id` from the task title (`Merge cluster: {id}`), and
/// returns the matching recommendation as a JSON string.
///
/// Fail-closed: missing strategy, missing/empty cluster id, or no matching
/// recommendation → `Err`. Desktop steps that historically returned empty
/// strings should call [`load_plan_json_from_task_soft`].
pub fn load_plan_json_from_task(task: &Task, project_path: &Path) -> Result<String, String> {
    let cluster_id = cluster_id_from_title(task.title.as_deref()).unwrap_or_default();

    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        let path = ProjectPaths::from_path(&project_path.to_string_lossy())
            .automation_dir
            .join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() {
        return Err(
            "No cannibalization_strategy artifact or automation file found for consolidate task"
                .to_string(),
        );
    }

    if cluster_id.is_empty() {
        return Err(
            "Cannot determine cluster_id from task title (expected 'Merge cluster: {id}')"
                .to_string(),
        );
    }

    let strategy: serde_json::Value = serde_json::from_str(&strategy_json)
        .map_err(|e| format!("Invalid strategy JSON: {e}"))?;
    let rec = strategy["merge_recommendations"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|r| r["cluster_id"].as_str().unwrap_or("") == cluster_id)
        })
        .cloned()
        .ok_or_else(|| {
            format!("No merge recommendation found for cluster '{cluster_id}'")
        })?;

    serde_json::to_string(&rec).map_err(|e| e.to_string())
}

/// Soft plan load for desktop consolidate steps — empty string on any miss.
///
/// Preserves historical step behavior where a missing plan yields empty JSON
/// and downstream steps no-op or fail with their own messages.
pub fn load_plan_json_from_task_soft(task: &Task, project_path: &str) -> String {
    load_plan_json_from_task(task, Path::new(project_path)).unwrap_or_default()
}

/// Extract `cluster_id` from a consolidate task title (`Merge cluster: {id}`).
pub fn cluster_id_from_title(title: Option<&str>) -> Option<String> {
    title
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ─── Redirects CSV ───────────────────────────────────────────────────────────

/// Upsert redirect rules into `.github/automation/redirects.csv`.
///
/// Merges with any existing CSV (source-keyed, last write wins). Creates the
/// automation directory when missing.
///
/// Returns the absolute path of the written CSV.
pub fn upsert_redirects_csv(
    project_path: &Path,
    keep_url: &str,
    redirect_urls: &[String],
) -> Result<PathBuf, String> {
    let automation_dir = ProjectPaths::from_path(&project_path.to_string_lossy()).automation_dir;
    std::fs::create_dir_all(&automation_dir)
        .map_err(|e| format!("Failed to create automation dir: {e}"))?;
    let csv_path = automation_dir.join("redirects.csv");

    let mut existing: HashMap<String, (String, i32)> = HashMap::new();
    if let Ok(raw) = std::fs::read_to_string(&csv_path) {
        for line in raw.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                if let Ok(status) = parts[2].trim().parse::<i32>() {
                    existing.insert(
                        parts[0].trim().to_string(),
                        (parts[1].trim().to_string(), status),
                    );
                }
            }
        }
    }

    for source in redirect_urls {
        existing.insert(source.clone(), (keep_url.to_string(), 301));
    }

    let mut csv = String::from("source,destination,status\n");
    for (source, (destination, status)) in &existing {
        csv.push_str(&format!("{source},{destination},{status}\n"));
    }
    std::fs::write(&csv_path, &csv)
        .map_err(|e| format!("Failed to write redirects.csv: {e}"))?;
    Ok(csv_path)
}

/// Pure-Rust heuristic: warn when `.gitignore` likely covers `redirects.csv`
/// (or a parent), so the file may not be committed/deployed.
///
/// Checks `.gitignore` at repo root, `.github/`, and `.github/automation/`
/// when present. No git subprocess; matches common path patterns only.
pub fn redirects_gitignore_warning(project_path: &Path) -> Option<String> {
    if !redirects_path_is_gitignored(project_path) {
        return None;
    }
    Some(
        "WARNING: redirects.csv appears to be covered by .gitignore and may not be committed. \
         Commit redirects.csv or port the rules into your deploy config (e.g. next.config) \
         so redirects ship with the site."
            .to_string(),
    )
}

/// Returns true when a .gitignore in the chain likely covers
/// `.github/automation/redirects.csv`.
fn redirects_path_is_gitignored(project_path: &Path) -> bool {
    let gitignore_paths = [
        project_path.join(".gitignore"),
        project_path.join(".github").join(".gitignore"),
        project_path
            .join(".github")
            .join("automation")
            .join(".gitignore"),
    ];
    for gi in &gitignore_paths {
        if let Ok(content) = std::fs::read_to_string(gi) {
            if gitignore_content_covers_redirects(&content) {
                return true;
            }
        }
    }
    false
}

/// Match common .gitignore patterns that would hide redirects.csv.
fn gitignore_content_covers_redirects(content: &str) -> bool {
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if gitignore_pattern_covers_redirects(line) {
            return true;
        }
    }
    false
}

fn gitignore_pattern_covers_redirects(pattern: &str) -> bool {
    // Leading slash = repo-root anchored; trailing slash / /** = directory.
    let p = pattern.trim_start_matches('/');
    let p = p.strip_suffix("/**").unwrap_or(p);
    let p = p.trim_end_matches('/');

    matches!(
        p,
        "redirects.csv"
            | ".github/automation/redirects.csv"
            | "automation/redirects.csv"
            | ".github/automation"
            | "automation"
    )
}

// ─── Inbound link rewrite ────────────────────────────────────────────────────

/// Rewrite every `/blog/` link that points at a redirected slug to the keeper
/// URL, across all MDX files in the project content dir.
///
/// Fail-closed when the content directory cannot be located.
///
/// Returns `(total_rewrites, per-file summaries)`.
pub fn rewrite_inbound_links_to_keeper(
    project_path: &Path,
    keep_url: &str,
    redirect_slugs: &[String],
) -> Result<(usize, Vec<serde_json::Value>), String> {
    if redirect_slugs.is_empty() {
        return Ok((0, vec![]));
    }

    let destination = format_blog_link(keep_url);
    let source_slugs: HashSet<String> = redirect_slugs
        .iter()
        .map(|s| normalize_url_slug(s))
        .filter(|s| !s.is_empty())
        .collect();
    if source_slugs.is_empty() {
        return Ok((0, vec![]));
    }

    let content_dir = crate::content::locator::resolve(project_path, None)
        .selected
        .ok_or_else(|| "Could not locate content directory".to_string())?;

    rewrite_links_to_redirected_slugs(&content_dir, &source_slugs, &destination)
}

/// Core rewrite loop over a resolved content directory.
///
/// Returns `(total_rewrites, per-file summaries)`. Counts distinct rewritten
/// hrefs per file (every occurrence of each href is replaced).
pub fn rewrite_links_to_redirected_slugs(
    content_dir: &Path,
    source_slugs: &HashSet<String>,
    destination: &str,
) -> Result<(usize, Vec<serde_json::Value>), String> {
    let matches = crate::content::linking::find_links_to_slugs(content_dir, source_slugs);

    // Group matched hrefs into per-file repair maps, preserving traversal
    // order (matches for one file are consecutive).
    let mut per_file: Vec<(PathBuf, HashMap<String, String>)> = Vec::new();
    for m in matches {
        match per_file.last_mut() {
            Some((file, repairs)) if *file == m.file => {
                repairs.insert(m.raw_href, destination.to_string());
            }
            _ => per_file.push((
                m.file,
                [(m.raw_href, destination.to_string())]
                    .into_iter()
                    .collect(),
            )),
        }
    }

    let mut total = 0usize;
    let mut files: Vec<serde_json::Value> = Vec::new();

    for (file, repairs) in per_file {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        let repaired = crate::content::linking::repair_blog_link_hrefs(&content, &repairs);
        std::fs::write(&file, repaired)
            .map_err(|e| format!("Failed to write {}: {}", file.display(), e))?;

        total += repairs.len();
        files.push(serde_json::json!({
            "file": file.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "rewrites": repairs.len(),
        }));
    }

    Ok((total, files))
}

// ─── Depublish ───────────────────────────────────────────────────────────────

/// Depublish every redirect source slug (fail-closed).
///
/// For each slug in `redirect_slugs` (skipping empty / keeper):
///   1. MDX frontmatter `status` → `redirected` (file stays on disk).
///   2. Matching SQLite `articles` row → `status = 'redirected'`.
///
/// Any failure (missing file, missing DB row, missing frontmatter) returns
/// `Err` rather than leaving a zombie published page.
///
/// Returns the number of depublished sources.
pub fn depublish_redirect_slugs(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    keep_slug: &str,
    redirect_slugs: &[String],
) -> Result<usize, String> {
    if redirect_slugs.is_empty() {
        return Ok(0);
    }

    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("Failed to list articles for depublish: {e}"))?;

    let keep_slug = normalize_url_slug(keep_slug);
    let mut depublished = 0usize;

    for raw in redirect_slugs {
        let slug = normalize_url_slug(raw);
        if slug.is_empty() || slug == keep_slug {
            continue;
        }

        // 1. Frontmatter status → redirected.
        let file = crate::content::ops::find_file_by_slug(project_path, &slug)?
            .ok_or_else(|| format!("Cannot depublish '{slug}': no content file matches"))?;
        let content = std::fs::read_to_string(&file)
            .map_err(|e| format!("Cannot depublish '{slug}': read failed: {e}"))?;
        let (fm, body) = crate::content::frontmatter::split_mdx(&content).ok_or_else(|| {
            format!(
                "Cannot depublish '{slug}': no frontmatter in {}",
                file.display()
            )
        })?;
        let new_fm = crate::content::frontmatter::replace_scalar(fm, "status", "redirected");
        std::fs::write(&file, crate::content::cleaner::rebuild_mdx(&new_fm, body))
            .map_err(|e| format!("Cannot depublish '{slug}': write failed: {e}"))?;

        // 2. SQLite articles row → redirected (fail closed if missing).
        let article = articles
            .iter()
            .find(|a| {
                a.url_slug == slug || normalize_url_slug(&a.url_slug) == slug
            })
            .ok_or_else(|| {
                format!("Cannot depublish '{slug}': no articles row matches the slug")
            })?;
        conn.execute(
            "UPDATE articles SET status = 'redirected' WHERE id = ?1 AND project_id = ?2",
            rusqlite::params![article.id, project_id],
        )
        .map_err(|e| format!("Cannot depublish '{slug}': DB update failed: {e}"))?;

        depublished += 1;
    }

    Ok(depublished)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("{prefix}_{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        dir
    }

    #[test]
    fn redirects_warning_when_automation_dir_gitignored() {
        let dir = unique_temp_dir("ps_redir_gi");
        std::fs::write(dir.join(".gitignore"), ".github/automation/\n").unwrap();
        let warn = redirects_gitignore_warning(&dir);
        assert!(
            warn.is_some(),
            "expected warning when .github/automation/ is gitignored"
        );
        assert!(
            warn.as_deref().unwrap().contains("redirects.csv"),
            "warning should mention redirects.csv: {warn:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redirects_warning_absent_when_gitignore_clean() {
        let dir = unique_temp_dir("ps_redir_clean");
        std::fs::write(dir.join(".gitignore"), "node_modules/\n*.log\n").unwrap();
        assert!(
            redirects_gitignore_warning(&dir).is_none(),
            "clean gitignore should not warn"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redirects_warning_matches_exact_csv_and_slash_variants() {
        assert!(gitignore_pattern_covers_redirects("redirects.csv"));
        assert!(gitignore_pattern_covers_redirects("/.github/automation/"));
        assert!(gitignore_pattern_covers_redirects("automation/"));
        assert!(gitignore_pattern_covers_redirects(
            ".github/automation/redirects.csv"
        ));
        assert!(!gitignore_pattern_covers_redirects("node_modules/"));
        assert!(!gitignore_pattern_covers_redirects("*.log"));
    }

    #[test]
    fn upsert_redirects_csv_writes_file() {
        let dir = unique_temp_dir("ps_redir_write");
        let path = upsert_redirects_csv(
            &dir,
            "/blog/keep",
            &["/blog/old".to_string()],
        )
        .unwrap();
        assert!(path.exists());
        let csv = std::fs::read_to_string(&path).unwrap();
        assert!(csv.contains("/blog/old"));
        assert!(csv.contains("/blog/keep"));
        assert!(csv.contains("301"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
