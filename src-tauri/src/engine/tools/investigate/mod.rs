//! Agentic investigation tools — rig-native `Tool` trait implementations.
//!
//! Each tool is a thin wrapper around existing Rust module functions.
//! Tools are read-only by default; only `RunContentAuditTool` and `CreateTaskTool`
//! mutate state. The tool catalog (`tool_catalog.toml`) describes each tool's
//! purpose and usage rules to the agent.
//!
//! These tools are attached to a rig `Agent` during the investigate flow,
//! allowing the LLM to explore project data freely.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;

use gsc::*;
use articles::*;
use audit::*;
use project::*;


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

// ── Tool set builder ────────────────────────────────────────────────────────

/// Build a Vec of boxed tools for the investigation agent.
pub fn investigation_tools(ctx: InvestigationContext) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    vec![
        Box::new(GscPerformanceTool { ctx: ctx.clone() }),
        Box::new(GscQueriesTool { ctx: ctx.clone() }),
        Box::new(GscMoversTool { ctx: ctx.clone() }),
        Box::new(ArticleListTool { ctx: ctx.clone() }),
        Box::new(ArticleFrontmatterTool { ctx: ctx.clone() }),
        Box::new(ArticleBodyHashTool { ctx: ctx.clone() }),
        Box::new(ArticleTitleScanTool { ctx: ctx.clone() }),
        Box::new(ContentAuditReportTool { ctx: ctx.clone() }),
        Box::new(RunContentAuditTool { ctx: ctx.clone() }),
        Box::new(CannibalizationClustersTool { ctx: ctx.clone() }),
        Box::new(IndexingStatusTool { ctx: ctx.clone() }),
        Box::new(CtrHealthTool { ctx: ctx.clone() }),
        Box::new(FrameworkFilesTool { ctx: ctx.clone() }),
        Box::new(ArticleLinkGraphTool { ctx: ctx.clone() }),
        Box::new(CreateTaskTool { ctx: ctx.clone() }),
        Box::new(WriteFeatureSpecTool { ctx: ctx.clone() }),
    ]
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
};
pub use articles::list_articles_json;
