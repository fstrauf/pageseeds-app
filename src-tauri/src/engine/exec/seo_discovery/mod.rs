//! Unified SEO discovery ranker.
//!
//! Fuses content audit, CTR audit, indexing health, cannibalization, and Clarity
//! UX signals into a single ranked opportunity backlog.

pub mod rank;

#[cfg(test)]
pub mod tests;

pub use rank::*;
