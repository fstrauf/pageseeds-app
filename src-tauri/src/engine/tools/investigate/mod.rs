//! Agentic investigation tools — rig-native `Tool` trait implementations.
//!
//! Each tool is a thin wrapper around existing Rust module functions.
//! Tools are read-only by default; mutators (`run_content_audit`, `create_task`,
//! `enqueue_task`, `write_feature_spec`) are for **standalone investigate only**.
//! In-workflow callers MUST use [`InvestigationAccess::ReadOnly`] /
//! [`investigation_read_only_tools`] so unattended agents cannot mutate state
//! outside TaskSpawner / review lifecycle.
//!
//! ## Source of truth
//!
//! [`src-tauri/config/tool_catalog.toml`](../../../../config/tool_catalog.toml)
//! is authoritative for catalog preamble text and `mutates` flags (loaded via
//! `include_str!` in [`catalog`]). Full vs ReadOnly tool attachment and catalog
//! text are both derived from those flags so they cannot drift.
//!
//! Runtime constructors are a name→`Tool` match in [`build_tool`]. Adding a
//! tool: append a TOML entry, add a match arm, implement the `Tool` type.
//! Alignment is enforced by tests.

use crate::engine::project_paths::ProjectPaths;

use gsc::*;
use articles::*;
use audit::*;
use project::*;

mod catalog;

// ── Shared context passed to all tools ──────────────────────────────────────

/// Context shared by all investigation tools. Contains project identifiers
/// and path resolution; tools open their own DB connections as needed.
#[derive(Debug, Clone)]
pub struct InvestigationContext {
    pub project_id: String,
    pub project_path: String,
    pub db_path: String,
}

impl InvestigationContext {
    pub fn open_db(&self) -> Result<rusqlite::Connection, String> {
        let conn = rusqlite::Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open database: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Failed to set busy timeout: {e}"))?;
        Ok(conn)
    }

    pub fn paths(&self) -> ProjectPaths {
        ProjectPaths::from_path(&self.project_path)
    }
}

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum InvestigationToolError {
    #[error("Data not available: {0}")]
    NotAvailable(String),
    #[error("Execution error: {0}")]
    Execution(String),
}

// ── Access mode + kit (tools + catalog, single mode) ────────────────────────

/// Access mode for investigation tools and their catalog preamble.
///
/// One mode drives **both** tool attachment and catalog text so callers cannot
/// advertise ReadOnly while attaching mutators (or the reverse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvestigationAccess {
    /// Standalone investigate — full tool set including mutators.
    Full,
    /// In-workflow unattended agents — read-only tools only (wired in #80).
    ReadOnly,
}

/// Tools + catalog text for a single [`InvestigationAccess`] mode.
pub struct InvestigationKit {
    pub tools: Vec<Box<dyn rig::tool::ToolDyn>>,
    pub catalog: String,
}

/// Build tools and catalog from the shared TOML catalog for the given access mode.
pub fn investigation_kit(
    ctx: InvestigationContext,
    access: InvestigationAccess,
) -> InvestigationKit {
    InvestigationKit {
        tools: tools_for_access(ctx, access),
        catalog: catalog::catalog_text_for_access(access),
    }
}

/// Full tool set including mutators (standalone investigate).
///
/// Convenience wrapper around [`investigation_kit`] with [`InvestigationAccess::Full`].
pub fn investigation_tools(ctx: InvestigationContext) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    investigation_kit(ctx, InvestigationAccess::Full).tools
}

/// Read-only tools only (in-workflow unattended use).
///
/// Convenience wrapper around [`investigation_kit`] with [`InvestigationAccess::ReadOnly`].
pub fn investigation_read_only_tools(
    ctx: InvestigationContext,
) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    investigation_kit(ctx, InvestigationAccess::ReadOnly).tools
}

/// Catalog preamble text for the given access mode (no tools built).
pub fn investigation_catalog(access: InvestigationAccess) -> String {
    catalog::catalog_text_for_access(access)
}

// ── Tool construction (names must match tool_catalog.toml) ──────────────────

fn tools_for_access(
    ctx: InvestigationContext,
    access: InvestigationAccess,
) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    catalog::TOOL_CATALOG
        .iter()
        .filter(|e| catalog::entry_included(e, access))
        .map(|e| build_tool(&e.name, ctx.clone()))
        .collect()
}

fn build_tool(name: &str, ctx: InvestigationContext) -> Box<dyn rig::tool::ToolDyn> {
    match name {
        "gsc_performance" => Box::new(GscPerformanceTool { ctx }),
        "gsc_queries" => Box::new(GscQueriesTool { ctx }),
        "gsc_movers" => Box::new(GscMoversTool { ctx }),
        "article_list" => Box::new(ArticleListTool { ctx }),
        "article_frontmatter" => Box::new(ArticleFrontmatterTool { ctx }),
        "article_body_hash" => Box::new(ArticleBodyHashTool { ctx }),
        "article_title_scan" => Box::new(ArticleTitleScanTool { ctx }),
        "validate_article" => Box::new(ValidateArticleTool { ctx }),
        "content_audit_report" => Box::new(ContentAuditReportTool { ctx }),
        "run_content_audit" => Box::new(RunContentAuditTool { ctx }),
        "cannibalization_clusters" => Box::new(CannibalizationClustersTool { ctx }),
        "indexing_status" => Box::new(IndexingStatusTool { ctx }),
        "ctr_health" => Box::new(CtrHealthTool { ctx }),
        "framework_files" => Box::new(FrameworkFilesTool { ctx }),
        "article_link_graph" => Box::new(ArticleLinkGraphTool { ctx }),
        "create_task" => Box::new(CreateTaskTool { ctx }),
        "enqueue_task" => Box::new(EnqueueTaskTool { ctx }),
        "get_task_status" => Box::new(GetTaskStatusTool { ctx }),
        "write_feature_spec" => Box::new(WriteFeatureSpecTool { ctx }),
        other => panic!(
            "tool_catalog.toml entry '{other}' has no build_tool match arm — \
             catalog and constructors drifted"
        ),
    }
}

/// Inventory names included for an access mode (for tests / alignment checks).
#[cfg(test)]
pub(crate) fn inventory_names(access: InvestigationAccess) -> Vec<&'static str> {
    catalog::catalog_names_for_access(access)
}

mod gsc;
mod articles;
mod audit;
mod project;
mod shared;
mod tests;

pub use shared::{
    scan_article_titles, hash_article_bodies, read_content_audit_report,
    read_cannibalization_clusters, get_indexing_status, read_framework_files, scan_link_graph,
    list_research_shortlist, list_article_quality_reviews,
};
pub use articles::{list_articles_json, validate_article_json};

#[cfg(test)]
mod alignment_tests {
    use super::*;

    /// Constructor names in the same order as the TOML catalog.
    /// Keep in sync with `config/tool_catalog.toml` keys — tests fail on drift.
    const CONSTRUCTOR_NAMES: &[&str] = &[
        "gsc_performance",
        "gsc_queries",
        "gsc_movers",
        "article_list",
        "article_frontmatter",
        "article_body_hash",
        "article_title_scan",
        "validate_article",
        "content_audit_report",
        "run_content_audit",
        "cannibalization_clusters",
        "indexing_status",
        "ctr_health",
        "framework_files",
        "article_link_graph",
        "create_task",
        "enqueue_task",
        "get_task_status",
        "write_feature_spec",
    ];

    #[test]
    fn constructors_and_toml_names_aligned() {
        let catalog_names = catalog::all_catalog_names();
        assert_eq!(
            catalog_names, CONSTRUCTOR_NAMES,
            "tool_catalog.toml order/names must match CONSTRUCTOR_NAMES / build_tool"
        );
        // Touch build_tool for every catalog name so unknown names panic in tests.
        let ctx = InvestigationContext {
            project_id: "align".into(),
            project_path: ".".into(),
            db_path: ":memory:".into(),
        };
        for name in &catalog_names {
            let _ = build_tool(name, ctx.clone());
        }
    }
}
