use crate::engine::task_store;
use crate::models::task::Task;

use super::PostStepContext;

// ─── Content-task keyword / title helpers ───────────────────────────────────

/// Known imperative prefixes used by content-task factories
/// ("Write article: X", "Create hub: X", …).
const CONTENT_TASK_TITLE_PREFIXES: &[&str] = &[
    "Write territory article:",
    "Write article:",
    "Create hub:",
    "Refresh hub:",
];

/// Strip known content-task title prefixes, returning the bare topic.
///
/// Single source of truth for title-prefix stripping — previously inlined in
/// `exec/agentic.rs` (`task_topic_stem`, `hub_spoke_context`) and
/// `exec/content/cluster_link.rs`.
pub(crate) fn strip_content_task_title_prefix(title: &str) -> &str {
    let mut topic = title.trim();
    loop {
        let before = topic;
        for prefix in CONTENT_TASK_TITLE_PREFIXES {
            if let Some(rest) = topic.strip_prefix(prefix) {
                topic = rest.trim_start();
            }
        }
        if topic == before {
            return topic;
        }
    }
}

/// Parse the `"Target keyword:"` line from a content task's description.
///
/// Single source of truth for the keyword line — previously triplicated in
/// `parse_content_task_keyword_meta` and `exec/agentic.rs::task_topic_stem`.
pub(crate) fn content_task_target_keyword(task: &Task) -> Option<String> {
    let desc = task.description.as_deref()?;
    for line in desc.lines() {
        if let Some(rest) = line.strip_prefix("Target keyword:") {
            let keyword = rest.trim();
            if !keyword.is_empty() {
                return Some(keyword.to_string());
            }
        }
    }
    None
}

/// Parse keyword metadata embedded in the write_article task description.
pub(crate) fn parse_content_task_keyword_meta(task: &Task) -> (Option<String>, Option<String>, i64) {
    let desc = match task.description.as_deref() {
        Some(d) if !d.is_empty() => d,
        _ => return (None, None, 0),
    };
    let keyword = content_task_target_keyword(task);
    let mut kd: Option<String> = None;
    let mut volume = 0i64;
    for line in desc.lines() {
        if let Some(rest) = line.strip_prefix("KD:") {
            if let Ok(n) = rest.trim().parse::<i64>() {
                kd = Some(n.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("Volume:") {
            volume = rest.trim().parse::<i64>().unwrap_or(0);
        }
    }
    (keyword, kd, volume)
}

/// Derive the expected URL slug from a filename stem and find the resolved file path
/// for a content-modifying task.
///
/// Returns `Some((expected_slug, absolute_file_path))` if the task modifies a single
/// known article file, or `None` for new-article tasks where no baseline exists.
pub(crate) fn find_expected_slug_and_file(ctx: &PostStepContext<'_>) -> Option<(String, std::path::PathBuf)> {
    let project_path = std::path::Path::new(ctx.project_path);
    let desc = ctx.task.description.as_deref().unwrap_or("");

    // Try to extract a file path from the description via the shared parser
    // (content::ops::file_path_from_description). Patterns:
    //   "File: ./src/blog/posts/02_post.mdx"
    //   "File: ./webapp/content/blog/13_post.mdx"
    // Fallback: try to parse "Article ID: X" and look up the file in DB.
    let file_path = if let Some(path) =
        crate::content::ops::file_path_from_description(desc, project_path)
    {
        path
    } else if let Some(start) = desc.find("Article ID:") {
        let rest = &desc[start + 11..];
        let id_str = rest.trim_start().split(|c: char| !c.is_ascii_digit()).next().unwrap_or("");
        if let Ok(article_id) = id_str.parse::<i64>() {
            if let Ok(articles) = task_store::list_articles(ctx.conn, &ctx.task.project_id) {
                if let Some(article) = articles.iter().find(|a| a.id == article_id) {
                    let path = std::path::Path::new(&article.file);
                    if path.is_relative() {
                        project_path.join(path)
                    } else {
                        path.to_path_buf()
                    }
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        return None;
    };

    if !file_path.exists() {
        return None;
    }

    // Derive expected slug from filename stem (same logic as article_index.rs)
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let expected = crate::content::slug::strip_numeric_prefix(stem)
        .to_lowercase()
        .replace('_', "-");

    if expected.is_empty() {
        return None;
    }

    Some((expected, file_path))
}
