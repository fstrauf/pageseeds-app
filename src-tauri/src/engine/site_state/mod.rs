//! Site State desk tools — domain builders for agent reads (issue #120).
//!
//! Provides the single JSON shapes from epic #117:
//! - [`build_site_overview`] — compact site health snapshot
//! - [`list_articles_catalog`] — filtered article inventory
//! - [`get_article_package`] — full package for one slug
//!
//! CLI and investigate Rig tools share these builders. Evidence-index
//! neighbors (#119) ship empty-safe until that lands.

mod builders;
mod types;

#[cfg(test)]
mod tests;

pub use builders::{build_site_overview, get_article_package, list_articles_catalog};
pub use types::*;
