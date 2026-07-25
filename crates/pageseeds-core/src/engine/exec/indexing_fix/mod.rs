//! Canonical 4-step pipeline for fix_indexing and fix_technical tasks.
//!
//! Step 1 (deterministic): Load the target MDX file and extract structured context
//! (including the parsed task description fields) so the agent doesn't waste
//! time hunting for files.
//!
//! Step 2 (agentic): Generate a structured `IndexingFixPlan` JSON. The agent
//! NEVER edits files — direct mode has no file I/O on most providers (Kimi
//! bridge `direct` advertises `file_io: false`; Claude/OpenAI/Ollama rig
//! agents are built with no tools). It only proposes changes.
//!
//! Step 3 (deterministic): Apply the plan to the MDX file with
//! snapshot/restore. Fails loudly when the plan produces no effective change.
//!
//! Step 4 (deterministic): Re-read the file and verify every planned change
//! landed. Fails loudly when the file is unchanged.
//!
//! Split into per-step files mirroring `indexing_health/`. The step functions
//! are re-exported here so the `step_registry.rs` call sites stay unchanged.
mod apply;
mod context;
mod generate;
mod verify;

#[cfg(test)]
mod tests;

pub(crate) use apply::*;
pub(crate) use context::*;
pub(crate) use generate::*;
pub(crate) use verify::*;

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexingFixContext {
    pub url: String,
    pub file_path: Option<String>,
    pub exists: bool,
    pub word_count: usize,
    pub h1: Option<String>,
    pub title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical: Option<String>,
    pub publish_date: Option<String>,
    pub internal_links: Vec<String>,
    pub internal_link_count: usize,
    // ─── Parsed from the task description (by prefix, any line) ─────────────
    #[serde(default)]
    pub issue: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub recommended_action: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub suggested_title: Option<String>,
    #[serde(default)]
    pub suggested_h1: Option<String>,
}

/// Typed fix plan returned by the agentic generate step (step 2).
///
/// The agent returns this as JSON; it never edits files directly. The
/// deterministic apply step (step 3) performs all writes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IndexingFixPlan {
    /// One-line summary of the root cause being addressed.
    #[serde(default)]
    pub diagnosis: String,
    #[serde(default)]
    pub changes: IndexingFixChanges,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IndexingFixChanges {
    /// New frontmatter `title`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New top-level `# ` heading in the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<String>,
    /// New frontmatter `description` (meta description).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Replacement first paragraph of the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,
    /// Frontmatter scalar updates for technical fixes (e.g. set `canonical`,
    /// change `robots` from noindex to index).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<Vec<FrontmatterEdit>>,
}

impl IndexingFixChanges {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.h1.is_none()
            && self.description.is_none()
            && self.intro.is_none()
            && self.frontmatter.as_ref().map(|f| f.is_empty()).unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterEdit {
    pub key: String,
    pub value: String,
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Load the IndexingFixPlan from the task artifact (preferred, persisted by
/// the executor from the generate step) or fall back to latest_raw.
fn resolve_plan(task: &Task, latest_raw: Option<&str>) -> Result<IndexingFixPlan, StepResult> {
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "indexing_fix_plan") {
        if let Some(content) = &artifact.content {
            match serde_json::from_str::<IndexingFixPlan>(content) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    return Err(StepResult::fail_with_output(format!(
                            "indexing_fix_plan artifact exists but is invalid JSON: {}",
                            e
                        ), content.clone()))
                }
            }
        }
    }

    if let Some(raw) = latest_raw {
        if let Some(p) = crate::engine::text::extract_json_as::<IndexingFixPlan>(raw) {
            return Ok(p);
        }
    }

    Err(StepResult::fail("No indexing_fix_plan artifact or latest_raw found. \
             Run the generate step first."
            .to_string()))
}

/// Re-resolve the target MDX file deterministically from the task description
/// URL (same logic as the context step). Never trust an agent-provided path.
fn resolve_target_file(task: &Task, project_path: &str) -> Result<PathBuf, StepResult> {
    let desc = parse_fix_task_description(task.description.as_deref().unwrap_or(""));
    if desc.url.is_empty() {
        return Err(StepResult::fail("Task description missing URL".to_string()));
    }

    let paths = ProjectPaths::from_path(project_path);
    let content_dir = crate::content::locator::resolve(Path::new(project_path), None)
        .selected
        .unwrap_or_else(|| paths.repo_root.clone());

    let slug = crate::content::slug::extract_slug_from_url(&desc.url);
    match find_mdx_by_slug(&content_dir, &slug) {
        Some(p) => Ok(p),
        None => Err(StepResult::fail(format!(
                "No MDX file found for {} (slug={}). Cannot apply indexing fix.",
                desc.url, slug
            ))),
    }
}

fn find_mdx_by_slug(content_dir: &Path, slug: &str) -> Option<std::path::PathBuf> {
    if slug.is_empty() {
        return None;
    }

    // Strip numeric prefix from URL segments too (e.g. "127_net_worth_tracker" → "net_worth_tracker")
    let last_segment = crate::content::slug::strip_numeric_prefix(
        slug.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(slug),
    )
    .replace('_', "-");

    let full_slug_dashed = crate::content::slug::strip_numeric_prefix(slug.trim_end_matches('/'))
        .replace('/', "-")
        .replace('_', "-");

    let mut best_match: Option<std::path::PathBuf> = None;

    for entry in walkdir::WalkDir::new(content_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
            continue;
        }

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let stem_clean = crate::content::slug::strip_numeric_prefix(stem).replace('_', "-");

        // Exact stem match on last segment — highest confidence
        if stem_clean == last_segment {
            return Some(path.to_path_buf());
        }

        // Full slug match (for flat structures)
        if stem_clean == full_slug_dashed && best_match.is_none() {
            best_match = Some(path.to_path_buf());
        }

        // Also check if the relative path (without extension) matches the slug
        if let Ok(rel) = path.strip_prefix(content_dir) {
            let rel_str = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let rel_without_ext = rel_str.trim_end_matches(".mdx").trim_end_matches(".md");
            let rel_clean = crate::content::slug::strip_numeric_prefix(rel_without_ext)
                .replace('/', "-")
                .replace('_', "-");
            if rel_clean == full_slug_dashed {
                return Some(path.to_path_buf());
            }
        }
    }

    best_match
}

fn extract_first_h1(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.trim_start().starts_with("# ") {
            return Some(line.trim_start_matches("# ").trim().to_string());
        }
    }
    None
}
