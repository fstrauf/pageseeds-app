//! PostHog conversion tape — deterministic store layer (issue #308).
//!
//! Collects conversion event counts by page × day via the PostHog Query API
//! (HogQL). Consumers: `content_outcome_review` (second evidence dimension)
//! and `site-overview` (optional conversion block). No LLM in the data path.

pub mod client;
pub mod db;
pub mod export;
pub mod models;

#[allow(unused_imports)]
pub use client::{normalize_host, PosthogClient, PosthogClientConfig};
#[allow(unused_imports)]
pub use models::*;
