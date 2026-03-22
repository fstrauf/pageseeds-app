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
    "research_keywords",
    "collect_gsc",
    "investigate_gsc",
    "reddit_search",
    "reddit_reply",
    "fix_404s",
    "fix_redirects",
    "technical_seo",
    "content_cleanup",
];

pub fn default_execution_mode(task_type: &str) -> &'static str {
    match task_type {
        "collect_gsc" => "automatic",
        "reddit_search" => "batchable",
        "write_article" | "optimize_article" => "spec",
        "reddit_reply" => "manual",
        _ => "manual",
    }
}

pub fn default_phase(task_type: &str) -> &'static str {
    match task_type {
        "collect_gsc" => "collection",
        "investigate_gsc" => "investigation",
        "research_keywords" => "research",
        "write_article" | "optimize_article" => "implementation",
        "reddit_search" | "reddit_reply" => "implementation",
        "fix_404s" | "fix_redirects" | "technical_seo" => "implementation",
        "content_cleanup" => "implementation",
        _ => "implementation",
    }
}
