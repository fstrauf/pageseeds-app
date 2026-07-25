/// GSC Indexing Recovery Campaign — deterministic prepare, drift, and plan steps.
///
/// Phase 1 (MVP):
///   - gsc_recovery_prepare: refresh stale link scan; report GSC freshness
///   - gsc_recovery_drift: reuse existing exec_gsc_drift
///   - gsc_recovery_plan: filter/score targets, build source candidates, write plan artifact
///
/// Phase 2:
///   - gsc_indexing_outcome_inspect: re-inspect target URL after wait period
///   - gsc_indexing_outcome_report: compare before/after, write outcome artifact
use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;

mod prepare;
mod drift;
mod plan;
mod outcome_review;
mod post_action;
mod helpers;
mod data;
mod tests;

pub(crate) use prepare::exec_gsc_recovery_prepare;
pub(crate) use drift::exec_gsc_recovery_drift;
pub(crate) use plan::exec_gsc_recovery_plan;
pub(crate) use outcome_review::{exec_gsc_indexing_outcome_inspect, exec_gsc_indexing_outcome_report};
pub(crate) use post_action::spawn_recovery_child_tasks;
pub(crate) use helpers::*;
pub(crate) use data::*;
