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
