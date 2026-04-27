use crate::models::task::ExecutionMode;

/// Application-level constants matching the Python CLI config.py
pub mod env_resolver;
pub mod task_definitions;

// Re-export for backward compatibility during migration.
pub use task_definitions::{default_execution_mode, default_phase, review_on_success};

#[allow(dead_code)]
pub(crate) const TASK_TYPES: &[&str] = &[
    "write_article",
    "optimize_article",
    "create_landing_page",
    "research_keywords",
    "research_landing_pages",
    "collect_gsc",
    "investigate_gsc",
    "reddit_search",
    "reddit_reply",
    "fix_404s",
    "fix_redirects",
    "fix_indexing",
    "fix_technical",
    "fix_content",
    "fix_gsc_access",
    "technical_seo",
    "content_cleanup",
    "sanitize_content",
    "indexing_diagnostics",
    "ctr_audit",
    "cannibalization_audit",
];
