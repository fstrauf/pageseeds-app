/// Consolidate cluster execution module.
///
/// Covers:
///   - merge_load_plan          (deterministic)
///   - merge_preflight          (deterministic)
///   - merge_extract_sections   (deterministic)
///   - merge_draft_patch        (agentic with merge-content skill)
///   - merge_apply_patch        (deterministic)
///   - merge_generate_redirects (deterministic)
///   - merge_validate_output    (deterministic)
use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;


mod load_plan;
mod preflight;
mod extract_sections;
mod draft_patch;
mod apply_patch;
mod generate_redirects;
mod validate_output;
mod sync_articles;
mod helpers;
mod tests;

pub(crate) use load_plan::exec_merge_load_plan;
pub(crate) use preflight::exec_merge_preflight;
pub(crate) use extract_sections::exec_merge_extract_sections;
pub(crate) use draft_patch::exec_merge_draft_patch;
pub(crate) use apply_patch::exec_merge_apply_patch;
pub(crate) use generate_redirects::exec_merge_generate_redirects;
pub(crate) use validate_output::exec_merge_validate_output;
pub(crate) use sync_articles::*;
pub(crate) use helpers::*;
