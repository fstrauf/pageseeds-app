/// Unified indexing health campaign execution module.
///
/// Orchestrates prerequisite checks, drift analysis, cluster context building,
/// agentic distinctiveness review, and campaign plan reduction.
///
/// Split into per-step files as part of Stage A.3 of issue #4. Each sub-module
/// contains the verbatim function bodies from the original monolithic file.
mod build_context;
mod distinctiveness;
mod prerequisites;
mod reduce;
mod spawn;

#[cfg(test)]
mod tests;

pub(crate) use build_context::*;
pub(crate) use distinctiveness::*;
pub(crate) use prerequisites::*;
pub(crate) use reduce::*;
pub(crate) use spawn::*;
