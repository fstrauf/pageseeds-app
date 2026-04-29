#![allow(dead_code)]
/// Keyword research execution module.
///
/// Native Rust pipeline:
///   1. `get_keyword_ideas` per theme → keywords WITH volume
///   2. Dedupe against articles.json + coverage analysis
///   3. Filter/prioritize by coverage gaps (skip well-covered topics)
///   4. `get_keyword_difficulty` per top-N keyword → KD scores
///   5. Merge into the standard output schema so KeywordPicker shows both volume and KD.
mod auto_spawn;
mod coverage_filter;
mod research_pipeline;
mod tests;
mod theme_extraction;

pub(crate) use coverage_filter::*;
pub(crate) use research_pipeline::*;
pub(crate) use theme_extraction::*;
