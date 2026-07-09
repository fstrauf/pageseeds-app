/// Per-target internal link fix execution module.
///
/// Each `fix_indexing_internal_links` task carries a single target in its
/// `indexing_link_target` artifact. The four steps are:
///   1. indexing_link_context  — deterministic: build target + source shortlist
///   2. indexing_link_plan     — agentic: choose source and anchor from shortlist
///   3. indexing_link_apply    — deterministic: append Related Articles link
///   4. indexing_link_verify   — deterministic: prove target gained inbound links
use std::collections::HashMap;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;


mod context;
mod plan;
mod apply;
mod verify;
mod helpers;
mod tests;

pub(crate) use context::exec_indexing_link_context;
pub(crate) use plan::exec_indexing_link_plan;
pub(crate) use apply::*;
pub(crate) use verify::exec_indexing_link_verify;
pub(crate) use helpers::*;
