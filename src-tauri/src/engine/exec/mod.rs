// Domain-specific execution modules extracted from executor.rs.
// executor.rs orchestrates; these modules implement.

pub mod agentic;
pub mod audit_health;
pub mod cannibalization_audit;
pub mod clarity;
pub mod common;
pub mod consolidate_cluster;
pub mod content;
pub mod content_audit;
pub mod coverage;
pub mod ctr_audit;
pub mod gsc;
pub mod gsc_diagnostics;
pub mod indexing_fix;
pub mod indexing_health;
pub mod intent_classifier;
pub mod keywords;
pub mod quality_rater;
pub mod reddit;
pub mod research;
pub mod social;
pub mod territory_research;
pub mod investigate;
pub mod feature_spec;
pub mod utils;

#[cfg(test)]
pub mod reddit_test;
