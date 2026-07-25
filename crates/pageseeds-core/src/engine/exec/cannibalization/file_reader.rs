//! Article file reader — extracts H1 and first N words from MDX.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Read an MDX file and extract (h1, first_200_words, published_date).
pub(crate) fn read_article_head_and_words(project_path: &str, file_ref: &str) -> (String, String, String) {
    if file_ref.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let repo_root = Path::new(project_path);
    let p = Path::new(file_ref);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };

    let content = match std::fs::read_to_string(&full) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "[cannibalization_audit] Could not read {}: {}",
                full.display(),
                e
            );
            return (String::new(), String::new(), String::new());
        }
    };

    let (frontmatter_raw, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((fm, b)) => (fm, b),
        None => ("", content.as_str()),
    };

    // Extract date from frontmatter if available
    let published_date = crate::content::frontmatter::top_level_scalars(frontmatter_raw)
        .into_iter()
        .find(|f| f.key == "date")
        .map(|f| f.raw_value.trim_matches('"').trim_matches('\'').to_string())
        .unwrap_or_default();

    // Extract h1
    let h1 = body
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("# ") && !t.starts_with("## ")
        })
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();

    // Extract first 200 words from body (strip markdown syntax roughly)
    let plain = body
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && !t.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join(" ");

    let words: Vec<&str> = plain.split_whitespace().collect();
    let first_200_words = words.into_iter().take(200).collect::<Vec<_>>().join(" ");

    (h1, first_200_words, published_date)
}
