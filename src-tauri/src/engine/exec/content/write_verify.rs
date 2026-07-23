//! Deterministic post-write verification for new-article content tasks.
//!
//! Runs as `content_write_verify` after `content_write_stage` (and its post-step
//! orphan ingestion) for `write_article`, `create_content`, `create_hub_page`,
//! and `refresh_hub_page`. Fails the task loudly when the write produced no
//! registered article file — previously this case logged an info line and let
//! the task complete as Done with zero output (issue #13).
//!
//! After existence + article-index registration, this step runs the shared
//! structural SEO floors via [`content::validate_article`] (issue #122):
//! MDX structure, H1, frontmatter title, meta description length, target
//! keyword in body, internal link resolution, and min word count. No LLM
//! scoring.
//!
//! Deterministic because existence, registration, and structural floors are
//! plain filesystem/DB/string checks — no judgment involved.

use std::path::Path;

use rusqlite::Connection;

use crate::content::validate_article::{
    format_failed_checks, validate_article_content, ValidateArticleInput, ValidateArticleResult,
};
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

    let base = verdict(written.as_deref(), registered, &content_dir, &ingest_note);
    if !base.success {
        return base;
    }

    // Existence + registration passed — run structural SEO floors.
    let path = match written.as_ref() {
        Some(p) => p,
        None => return base, // unreachable given verdict, but keep safe
    };

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!(
                "Write verify failed: could not read written file {}: {e}",
                path.display()
            ));
        }
    };

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let slug = crate::content::ops::slug_from_filename(file_name);

    let target_keyword = resolve_target_keyword(conn, task, file_name, &content);
    let valid_link_targets =
        match crate::engine::task_store::load_valid_link_targets(conn, &task.project_id, project_path)
        {
            Ok(set) => Some(set),
            Err(e) => {
                log::warn!(
                    "[write_verify] load_valid_link_targets failed (internal links auto-pass): {e}"
                );
                None
            }
        };

    let input = ValidateArticleInput {
        target_keyword,
        valid_link_targets,
        min_word_count: None,
    };
    let report = validate_article_content(&slug, &content, &input);

    if !report.ok {
        return structural_fail(&report, file_name);
    }

    StepResult {
        success: true,
        message: format!(
            "{} — structural SEO floors passed{}",
            base.message.trim_end_matches(&ingest_note),
            ingest_note
        ),
        output: serde_json::to_string(&report).ok(),
        artifact_key: None,
    }
}

fn structural_fail(report: &ValidateArticleResult, file_name: &str) -> StepResult {
    let failed = format_failed_checks(report);
    let output = serde_json::to_string_pretty(report).ok();
    StepResult {
        success: false,
        message: format!(
            "Write verify failed structural SEO floors for {file_name}: {failed}"
        ),
        output,
        artifact_key: None,
    }
}

/// Resolve target keyword for the write-verify presence check.
/// Order: DB article row → frontmatter → task title.
fn resolve_target_keyword(
    conn: &Connection,
    task: &Task,
    filename: &str,
    content: &str,
) -> Option<String> {
    // 1. Article row registered for this file.
    if let Ok(kw) = conn.query_row(
        "SELECT target_keyword FROM articles WHERE project_id = ?1 AND file LIKE ?2 LIMIT 1",
        rusqlite::params![task.project_id, format!("%{filename}")],
        |row| row.get::<_, Option<String>>(0),
    ) {
        if let Some(k) = kw {
            let t = k.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }

    // 2. Frontmatter target_keyword.
    if let Some(k) =
        crate::content::frontmatter::extract_frontmatter_string(content, "target_keyword")
    {
        let t = k.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }

    // 3. Task title is often the seed keyword for write_article.
    task.title
        .as_ref()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
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
                 The file-IO agent returned without writing a file. Fix the provider output or \
                 re-enqueue the task.",
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

    #[test]
    fn structural_fail_lists_failed_check_ids() {
        let report = ValidateArticleResult {
            slug: "x".into(),
            ok: false,
            checks: vec![
                crate::content::validate_article::ArticleCheck {
                    id: "has_h1".into(),
                    pass: false,
                    detail: Some("no H1".into()),
                },
                crate::content::validate_article::ArticleCheck {
                    id: "min_word_count".into(),
                    pass: false,
                    detail: Some("12 (want ≥ 800)".into()),
                },
            ],
        };
        let result = structural_fail(&report, "1_x.mdx");
        assert!(!result.success);
        assert!(result.message.contains("has_h1"));
        assert!(result.message.contains("min_word_count"));
        assert!(result.output.is_some());
    }
}
