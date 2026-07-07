/// CTR (Click-Through Rate) audit execution module.
///
/// Covers:
///   - exec_ctr_build_context   (deterministic data collection + clicks_lost scoring)
///   - exec_ctr_analyze         (agentic analysis with ctr-optimization skill)
///   - create_ctr_fix_tasks     (spawn follow-up fix tasks)
mod analyze;
mod apply;
mod context;
mod generate;
mod outcome;
mod patch;
pub mod rendered;
mod task_spawner;
mod template;

#[cfg(test)]
mod tests;

pub(crate) use analyze::*;
pub(crate) use apply::*;
pub(crate) use context::*;
pub(crate) use generate::*;
pub(crate) use outcome::*;
pub(crate) use patch::*;
pub(crate) use rendered::*;
pub(crate) use task_spawner::*;
pub(crate) use template::*;
