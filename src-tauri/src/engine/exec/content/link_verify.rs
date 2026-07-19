/// Deterministic post-write link integrity verify.
///
/// Runs as the final step of every agentic content write (`write_article`,
/// `create_hub_page`, `refresh_hub_page`, `optimize_article`,
/// `optimize_content`, `create_content`). The agent writes MDX directly to the
/// repo; this step checks every `/blog/` link it committed:
///
/// 1. Extract all `/blog/` link hrefs (canonical and malformed) via
///    [`crate::content::linking::extract_blog_link_hrefs`].
/// 2. Resolve each slug against the project's valid link targets
///    ([`crate::engine::task_store::load_valid_link_targets`]) via
///    [`crate::content::slug::resolve_slug`] (exact match first, then
///    normalized).
/// 3. Auto-repair resolvable but non-canonical hrefs in place (e.g.
///    `/blog/248_roast_profile_management` → `/blog/roast-profile-management`),
///    keeping anchor text.
/// 4. If any link cannot be resolved, fail the step with a per-link report —
///    and write nothing, so a failed verify never leaves a half-edited file.
///
/// Deterministic because slug → target resolution is a plain set lookup over
/// data collected from SQLite and redirects.csv — no judgment involved.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// How old a content file may be to count as "just written" when the task
/// description does not name the file.
const WRITTEN_FILE_MAX_AGE_SECS: u64 = 30 * 60;

pub(crate) fn exec_link_integrity_verify(task: &Task, project_path: &str) -> StepResult {
    let repo_root = Path::new(project_path);
    let content_dir = crate::content::locator::resolve(repo_root, None).selected;

    let file_path = match find_written_file(task, repo_root, content_dir.as_deref()) {
        Some(p) => p,
        None => {
            return StepResult {
                success: true,
                message: "no written article file found — skipping link verify".to_string(),
                output: None,
            }
        }
    };

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!(
                    "Link verify: failed to read {}: {}",
                    file_path.display(),
                    e
                ),
                output: None,
            }
        }
    };

    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Link verify: failed to open app database: {}", e),
                output: None,
            }
        }
    };

    let valid_targets = match crate::engine::task_store::load_valid_link_targets(
        &db,
        &task.project_id,
        project_path,
    ) {
        Ok(set) => set,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Link verify: failed to load project slugs: {}", e),
                output: None,
            }
        }
    };

    verify_links_in_file(&file_path, &content, &valid_targets)
}

/// Core verify/repair logic, split out for unit testing (no DB or task needed).
fn verify_links_in_file(
    file_path: &Path,
    content: &str,
    valid_targets: &HashSet<String>,
) -> StepResult {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let links = crate::content::linking::extract_blog_link_hrefs(content);

    if links.is_empty() {
        return StepResult {
            success: true,
            message: format!("Link verify passed: no /blog/ links in {}", file_name),
            output: None,
        };
    }

    let mut repairs: HashMap<String, String> = HashMap::new();
    let mut unresolved: Vec<serde_json::Value> = Vec::new();

    for (anchor, raw_href, slug_written) in &links {
        match crate::content::slug::resolve_slug(slug_written, valid_targets) {
            Some(resolved) => {
                let canonical = crate::content::slug::format_blog_link(&resolved);
                if *raw_href != canonical {
                    repairs.insert(raw_href.clone(), canonical);
                }
            }
            None => {
                unresolved.push(serde_json::json!({
                    "file": file_name,
                    "anchor": anchor,
                    "href": raw_href,
                    "normalized_slug": crate::content::slug::normalize_url_slug(slug_written),
                }));
            }
        }
    }

    // All-or-nothing: an unresolvable link fails the step and nothing is
    // written, so the file is never left half-repaired.
    if !unresolved.is_empty() {
        let details = unresolved
            .iter()
            .map(|u| {
                format!(
                    "{} (anchor: \"{}\", normalized: \"{}\")",
                    u["href"].as_str().unwrap_or(""),
                    u["anchor"].as_str().unwrap_or(""),
                    u["normalized_slug"].as_str().unwrap_or(""),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let report = serde_json::json!({
            "file": file_path.to_string_lossy(),
            "unresolved_links": unresolved,
        });
        return StepResult {
            success: false,
            message: format!(
                "Link verify failed for {}: {} unresolvable /blog/ link(s): {}",
                file_name,
                unresolved.len(),
                details
            ),
            output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
        };
    }

    if repairs.is_empty() {
        return StepResult {
            success: true,
            message: format!(
                "Link verify passed: {} /blog/ link(s) valid in {}",
                links.len(),
                file_name
            ),
            output: None,
        };
    }

    let repaired = crate::content::linking::repair_blog_link_hrefs(content, &repairs);
    if let Err(e) = std::fs::write(file_path, repaired) {
        return StepResult {
            success: false,
            message: format!(
                "Link verify: failed to write repairs to {}: {}",
                file_path.display(),
                e
            ),
            output: None,
        };
    }

    let report = serde_json::json!({
        "file": file_path.to_string_lossy(),
        "links_checked": links.len(),
        "repairs": repairs.iter().map(|(from, to)| {
            serde_json::json!({ "from": from, "to": to })
        }).collect::<Vec<_>>(),
    });
    StepResult {
        success: true,
        message: format!(
            "Link verify passed: repaired {} of {} /blog/ link(s) in {}",
            repairs.len(),
            links.len(),
            file_name
        ),
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}

/// Resolve the file a content-write task just wrote.
///
/// 1. `File: <path>` in the task description (same convention as
///    `engine::post_actions`), relative paths resolved against the repo root.
/// 2. Fallback: the most recently modified `.md`/`.mdx` file in the content
///    directory, if modified within the last 30 minutes.
fn find_written_file(task: &Task, repo_root: &Path, content_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(desc) = task.description.as_deref() {
        if let Some(path) = file_path_from_description(desc, repo_root) {
            if path.exists() {
                return Some(path);
            }
        }
    }

    let dir = content_dir?;
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(WRITTEN_FILE_MAX_AGE_SECS);
    crate::content::locator::collect_markdown_files(dir)
        .into_iter()
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            (modified >= cutoff).then_some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

/// Parse `File: <path>` from a task description (with or without a space after
/// the colon; terminated by `" |"` or end of line).
fn file_path_from_description(desc: &str, repo_root: &Path) -> Option<PathBuf> {
    let (start, prefix_len) = desc
        .find("File: ")
        .map(|i| (i, 6))
        .or_else(|| desc.find("File:").map(|i| (i, 5)))?;
    let rest = &desc[start + prefix_len..];
    let end = rest
        .find(" |")
        .or_else(|| rest.find('\n'))
        .unwrap_or(rest.len());
    let path_str = rest[..end].trim();
    if path_str.is_empty() {
        return None;
    }
    let path = Path::new(path_str);
    Some(if path.is_relative() {
        repo_root.join(path)
    } else {
        path.to_path_buf()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_content_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-link-verify-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn valid_set(slugs: &[&str]) -> HashSet<String> {
        slugs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn file_path_from_description_parses_conventions() {
        let root = Path::new("/repo");
        assert_eq!(
            file_path_from_description("Write article. File: ./content/1_post.mdx | Keyword: x", root),
            Some(PathBuf::from("/repo/content/1_post.mdx"))
        );
        assert_eq!(
            file_path_from_description("File:./content/2_post.mdx\nMore text", root),
            Some(PathBuf::from("/repo/content/2_post.mdx"))
        );
        assert_eq!(
            file_path_from_description("File: /abs/path/3_post.mdx", root),
            Some(PathBuf::from("/abs/path/3_post.mdx"))
        );
        assert_eq!(file_path_from_description("no file here", root), None);
    }

    #[test]
    fn repairs_filename_form_link_when_normalized_slug_exists() {
        let dir = temp_content_dir();
        let file = dir.join("1_new.mdx");
        std::fs::write(
            &file,
            "# New\n\nSee [the roast guide](/blog/248_roast_profile_management) now.\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        let result = verify_links_in_file(&file, &content, &valid_set(&["roast-profile-management"]));

        assert!(result.success, "repair path should succeed: {}", result.message);
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("[the roast guide](/blog/roast-profile-management)"));
        assert!(!written.contains("248_roast_profile_management"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repairs_trailing_slash_and_underscore_variants() {
        let dir = temp_content_dir();
        let file = dir.join("1_new.mdx");
        std::fs::write(
            &file,
            "[a](/blog/Roast_Profile_Management/) and [b](/blog/hub-coffee/)\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        let result = verify_links_in_file(
            &file,
            &content,
            &valid_set(&["roast-profile-management", "hub-coffee"]),
        );

        assert!(result.success);
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("[a](/blog/roast-profile-management)"));
        assert!(written.contains("[b](/blog/hub-coffee)"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fails_with_per_link_report_when_unresolvable() {
        let dir = temp_content_dir();
        let file = dir.join("1_new.mdx");
        let original = "# New\n\nCheck [ghost post](/blog/ghost-slug) here.\n";
        std::fs::write(&file, original).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        let result = verify_links_in_file(&file, &content, &valid_set(&["roast-profile-management"]));

        assert!(!result.success);
        assert!(result.message.contains("ghost-slug"), "message names the href: {}", result.message);
        assert!(result.message.contains("ghost post"), "message names the anchor: {}", result.message);
        assert!(result.message.contains("1_new.mdx"), "message names the file: {}", result.message);
        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        let entries = output["unresolved_links"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["normalized_slug"], "ghost-slug");
        // Failed step must not modify the file.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn all_or_nothing_write_when_one_link_fails() {
        let dir = temp_content_dir();
        let file = dir.join("1_new.mdx");
        let original = "[good](/blog/248_roast_profile_management) and [bad](/blog/ghost)\n";
        std::fs::write(&file, original).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        let result = verify_links_in_file(&file, &content, &valid_set(&["roast-profile-management"]));

        assert!(!result.success);
        // The repairable link must NOT be repaired when another link fails.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_file_is_left_untouched() {
        let dir = temp_content_dir();
        let file = dir.join("1_new.mdx");
        let original = "[ok](/blog/roast-profile-management) and [hub](/blog/hub-coffee)\n";
        std::fs::write(&file, original).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        let result = verify_links_in_file(
            &file,
            &content,
            &valid_set(&["roast-profile-management", "hub-coffee"]),
        );

        assert!(result.success);
        assert!(result.message.contains("2 /blog/ link(s) valid"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_with_no_blog_links_passes() {
        let dir = temp_content_dir();
        let file = dir.join("1_new.mdx");
        std::fs::write(&file, "# Post\n\nNo links here. [ext](https://example.com)\n").unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        let result = verify_links_in_file(&file, &content, &valid_set(&[]));

        assert!(result.success);
        assert!(result.message.contains("no /blog/ links"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
