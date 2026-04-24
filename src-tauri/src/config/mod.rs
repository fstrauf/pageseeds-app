use crate::models::task::ExecutionMode;

/// Application-level constants matching the Python CLI config.py
pub mod env_resolver;

pub const PHASES: &[&str] = &[
    "collection",
    "investigation",
    "research",
    "implementation",
    "verification",
];

pub const STATUSES: &[&str] = &["todo", "in_progress", "review", "done", "cancelled"];

pub const PRIORITIES: &[&str] = &["high", "medium", "low"];

pub const TASK_TYPES: &[&str] = &[
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
    "indexing_diagnostics",
    "ctr_audit",
    "cannibalization_audit",
];

pub fn default_execution_mode(task_type: &str) -> ExecutionMode {
    match task_type {
        "collect_gsc" => ExecutionMode::Automatic,
        "indexing_diagnostics" => ExecutionMode::Automatic,
        "ctr_audit" => ExecutionMode::Automatic,
        "cannibalization_audit" => ExecutionMode::Automatic,
        "reddit_search" => ExecutionMode::Batchable,
        "research_keywords" | "custom_keyword_research" | "research_landing_pages" => ExecutionMode::Manual,
        "write_article" | "optimize_article" | "create_landing_page" => ExecutionMode::Spec,
        "reddit_reply" => ExecutionMode::Manual,
        _ => ExecutionMode::Manual,
    }
}

pub fn default_phase(task_type: &str) -> &'static str {
    match task_type {
        "collect_gsc" => "collection",
        "indexing_diagnostics" => "investigation",
        "investigate_gsc" => "investigation",
        "ctr_audit" => "investigation",
        "cannibalization_audit" => "investigation",
        "research_keywords" => "research",
        "write_article" | "optimize_article" | "create_landing_page" => "implementation",
        "reddit_search" | "reddit_reply" => "implementation",
        "fix_404s" | "fix_redirects" | "technical_seo" => "implementation",
        "fix_indexing" | "fix_technical" | "fix_content" | "fix_gsc_access" => "implementation",
        "content_cleanup" => "implementation",
        _ => "implementation",
    }
}
