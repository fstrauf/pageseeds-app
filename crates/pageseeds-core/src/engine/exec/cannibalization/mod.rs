/// Keyword cannibalization audit execution module.
///
/// Covers:
///   - exec_can_build_context   (deterministic TF-IDF clustering + link graph + hub gaps)
///   - exec_can_exact_keyword_dupes (deterministic duplicate keyword detection)
///   - exec_can_select_candidates (deterministic candidate selection)
///   - exec_can_analyze_candidates (agentic merge analysis)
///   - exec_can_reduce_strategy (deterministic strategy reduction)
///   - create_can_fix_tasks     (spawn follow-up fix tasks)
///
/// Split into per-concern files as part of Stage A.4 of issue #4. Each
/// sub-module contains verbatim function bodies from the original monolithic
/// file; internal helpers were made `pub(crate)` to preserve the original
/// call-site form across module boundaries.
mod analyze;
mod build_context;
mod candidates;
mod clustering;
mod exact_dupes;
mod file_reader;
mod hub_gaps;
mod link_metrics;
mod reduce;
mod spawn;
mod tfidf;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use analyze::*;
pub(crate) use build_context::*;
pub(crate) use candidates::*;
pub(crate) use clustering::*;
pub(crate) use exact_dupes::*;
pub(crate) use file_reader::*;
pub(crate) use hub_gaps::*;
pub(crate) use link_metrics::*;
pub(crate) use reduce::*;
pub(crate) use spawn::*;
pub(crate) use tfidf::*;
pub(crate) use types::*;
