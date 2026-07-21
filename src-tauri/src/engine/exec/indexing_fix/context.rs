//! Step 1 (deterministic): context loading and task-description parsing.

use std::collections::HashSet;
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::{extract_first_h1, find_mdx_by_slug, IndexingFixContext};

/// Structured fields parsed from a fix task description.
///
/// Descriptions come from two spawn sites with different formats:
/// - `gsc_diagnostics`: `URL:` / `Issue:` / `Action:` / `Verdict:`
/// - indexing health campaign (`build_rewrite_spec`): `URL:` /
///   `Recommended action:` / `Reason:` / `Parent campaign:` /
///   `Suggested title:` / `Suggested H1:`
///
/// Parsing matches by prefix on ANY line — never by fixed line index.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FixTaskDescription {
    pub url: String,
    pub issue: Option<String>,
    pub action: Option<String>,
    pub verdict: Option<String>,
    pub recommended_action: Option<String>,
    pub reason: Option<String>,
    pub suggested_title: Option<String>,
    pub suggested_h1: Option<String>,
}

pub(crate) fn parse_fix_task_description(description: &str) -> FixTaskDescription {
    let mut out = FixTaskDescription::default();
    for line in description.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("URL: ") {
            out.url = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("Issue: ") {
            out.issue = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Action: ") {
            out.action = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Verdict: ") {
            out.verdict = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Recommended action: ") {
            out.recommended_action = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Reason: ") {
            out.reason = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Suggested title: ") {
            out.suggested_title = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Suggested H1: ") {
            out.suggested_h1 = Some(v.trim().to_string());
        }
    }
    out
}

/// Deterministic pre-step: gather structured context for the target URL.
pub(crate) fn exec_indexing_fix_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let desc = parse_fix_task_description(task.description.as_deref().unwrap_or(""));
    let url = desc.url.clone();

    if url.is_empty() {
        return StepResult {
            success: false,
            message: "Task description missing URL".to_string(),
            output: None,
        };
    }

    // Resolve content directory
    let content_dir = crate::content::locator::resolve(Path::new(project_path), None)
        .selected
        .unwrap_or_else(|| paths.repo_root.clone());

    // Try to find the MDX file matching the URL slug
    let slug = crate::content::slug::extract_slug_from_url(&url);
    let file_match = find_mdx_by_slug(&content_dir, &slug);

    log::info!(
        "[indexing_fix_context] url={} content_dir={} slug={} matched={}",
        url,
        content_dir.display(),
        slug,
        file_match
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string())
    );

    let mut ctx = IndexingFixContext {
        url: url.clone(),
        file_path: file_match.as_ref().map(|p| p.to_string_lossy().to_string()),
        exists: file_match.is_some(),
        word_count: 0,
        h1: None,
        title: None,
        meta_description: None,
        canonical: None,
        publish_date: None,
        internal_links: vec![],
        internal_link_count: 0,
        issue: desc.issue,
        action: desc.action,
        recommended_action: desc.recommended_action,
        reason: desc.reason,
        suggested_title: desc.suggested_title,
        suggested_h1: desc.suggested_h1,
    };

    if let Some(ref path) = file_match {
        if let Ok(content) = std::fs::read_to_string(path) {
            ctx.word_count = crate::content::ops::count_words(&content);
            ctx.h1 = extract_first_h1(&content);
            ctx.title = crate::content::frontmatter::extract_frontmatter_string(&content, "title");
            ctx.meta_description = crate::content::frontmatter::extract_frontmatter_string(&content, "description");
            ctx.canonical = crate::content::frontmatter::extract_frontmatter_string(&content, "canonical");
            ctx.publish_date = crate::content::frontmatter::extract_frontmatter_string(&content, "date");
            ctx.internal_links = extract_internal_links(&content);
            ctx.internal_link_count = ctx.internal_links.len();
        }
    }

    let output = serde_json::to_string_pretty(&ctx).unwrap_or_default();

    if !ctx.exists {
        return StepResult {
            success: false,
            message: format!(
                "No MDX file found for {} (slug={}). Cannot fix indexing for a page that has no content file.",
                url, slug
            ),
            output: Some(output),
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Context loaded for {}: {} words, {} internal links{}",
            url,
            ctx.word_count,
            ctx.internal_link_count,
            if ctx.exists { "" } else { " (file not found)" }
        ),
        output: Some(output),
    }
}

fn extract_internal_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    // Simple markdown link extraction: [text](path)
    let re = regex::Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    for cap in re.captures_iter(content) {
        let href = cap[2].to_string();
        if href.starts_with('/') && !href.starts_with("//") && !seen.contains(&href) {
            seen.insert(href.clone());
            links.push(href);
        }
    }
    links
}
