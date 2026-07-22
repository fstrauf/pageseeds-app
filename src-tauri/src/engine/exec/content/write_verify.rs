//! Deterministic post-write verification for new-article content tasks.
//!
//! Runs as `content_write_verify` after `content_write_stage` (and its post-step
//! orphan ingestion) for `write_article`, `create_content`, `create_hub_page`,
//! and `refresh_hub_page`. Fails the task loudly when the write produced no
//! registered article file — previously this case logged an info line and let
//! the task complete as Done with zero output (issue #13).
//!
//! File existence + article-index registration is all this step checks.
//! Quality gating (word count, frontmatter validity, keyword placement) is
//! intentionally out of scope here and tracked in issue #7; when built, it
//! should run from this step and reuse `engine::exec::quality_rater`.
//!
//! Deterministic because existence and registration are plain filesystem/DB
//! lookups — no judgment involved.

use std::path::Path;

use rusqlite::Connection;

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

pub(crate) fn exec_content_write_verify(
    conn: &Connection,
    task: &Task,
    project_path: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);
    let content_dir = match crate::content::locator::resolve(repo_root, None).selected {
        Some(dir) => dir,
        None => {
            return StepResult::fail("Write verify failed: no content directory resolved for this project — \
                          the write step had nowhere to persist the article."
                    .to_string())
        }
    };

    let written =
        crate::engine::exec::content::find_written_file(task, repo_root, Some(&content_dir));

    // Idempotent: registers anything the post-step ingestion missed (e.g. if
    // it errored) and refreshes the articles.json projection.
    let mut ingest_note = String::new();
    match crate::engine::post_actions::ingest_content_write_files(conn, task, repo_root) {
        Ok(summary) if summary.ingested > 0 => {
            ingest_note = format!("; registered {} new file(s)", summary.ingested);
        }
        Ok(_) => {}
        Err(e) => log::warn!("[write_verify] orphan ingestion failed: {}", e),
    }

    let registered = written
        .as_ref()
        .map(|path| {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            article_registered(conn, &task.project_id, &filename)
        })
        .unwrap_or(false);

    verdict(written.as_deref(), registered, &content_dir, &ingest_note)
}

/// Whether an article row exists for the given filename (same `LIKE %filename`
/// match the post-write registration uses).
fn article_registered(conn: &Connection, project_id: &str, filename: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM articles WHERE project_id = ?1 AND file LIKE ?2 LIMIT 1",
        rusqlite::params![project_id, format!("%{}", filename)],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

/// Pure decision core, split out for unit testing (no filesystem or DB needed).
fn verdict(
    written: Option<&Path>,
    registered: bool,
    content_dir: &Path,
    ingest_note: &str,
) -> StepResult {
    let file_name = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    match (written, registered) {
        (Some(path), true) => StepResult {
            success: true,
            message: format!(
                "Write verify passed: {} exists and is registered in the article index{}",
                file_name(path),
                ingest_note
            ),
            output: None,
            artifact_key: None,
        },
        (Some(path), false) => StepResult::fail(format!(
                "Write verify failed: {} exists in {} but is not registered in the article index. \
                 Orphan ingestion could not register it — check the file's frontmatter and the \
                 registration logs, then re-enqueue the task.",
                file_name(path),
                content_dir.display()
            )),
        (None, _) => StepResult::fail(format!(
                "Write verify failed: content_write_stage completed but no article file exists in {}. \
                 The agent returned without writing a file (a text-only provider may have produced \
                 no parseable MDX, or the write failed). Fix the provider output or re-enqueue the task.",
                content_dir.display()
            )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn passes_when_file_exists_and_is_registered() {
        let file = PathBuf::from("/repo/content/7_gamma_scalping.mdx");
        let result = verdict(Some(&file), true, Path::new("/repo/content"), "");
        assert!(result.success, "{}", result.message);
        assert!(result.message.contains("7_gamma_scalping.mdx"));
    }

    #[test]
    fn fails_when_file_missing() {
        let result = verdict(None, false, Path::new("/repo/content"), "");
        assert!(!result.success);
        assert!(result.message.contains("no article file exists"));
        assert!(result.message.contains("/repo/content"));
    }

    #[test]
    fn fails_when_file_exists_but_unregistered() {
        let file = PathBuf::from("/repo/content/7_gamma_scalping.mdx");
        let result = verdict(Some(&file), false, Path::new("/repo/content"), "");
        assert!(!result.success);
        assert!(result.message.contains("not registered"));
        assert!(result.message.contains("7_gamma_scalping.mdx"));
    }
}
