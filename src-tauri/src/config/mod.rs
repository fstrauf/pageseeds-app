/// Application-level constants matching the Python CLI config.py
pub mod env_resolver;
pub mod task_definitions;

// Re-export for backward compatibility during migration.
pub use task_definitions::{default_execution_mode, default_phase, review_on_success};
