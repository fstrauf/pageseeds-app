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
//! [`TOOL_INVENTORY`] is the single structured inventory: tool name, `mutates`
//! flag, catalog section text, and constructor. [`investigation_kit`] filters
//! that inventory by [`InvestigationAccess`] and returns both the tool vec and
//! the catalog preamble text so they cannot drift.
//!
//! Full TOML externalization of the catalog is tracked in issue #83. Until then
//! the embedded inventory here is authoritative; `src-tauri/config/tool_catalog.toml`
//! is a historical draft and is **not** loaded at runtime.

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

/// Build tools and catalog from the shared inventory for the given access mode.
pub fn investigation_kit(
    ctx: InvestigationContext,
    access: InvestigationAccess,
) -> InvestigationKit {
    InvestigationKit {
        tools: tools_for_access(ctx, access),
        catalog: catalog_for_access(access),
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
    catalog_for_access(access)
}

// ── Structured inventory (single source of truth) ───────────────────────────

/// Metadata for one investigation tool. Constructors live in [`build_tool`].
struct ToolInventoryEntry {
    name: &'static str,
    mutates: bool,
    purpose: &'static str,
    when_to_use: &'static str,
    when_not_to_use: &'static str,
}

/// Canonical inventory: order, names, mutates flags, and catalog section text.
///
/// Adding a tool: append an entry here, add a match arm in [`build_tool`], and
/// implement the `Tool` type. Full vs ReadOnly tool/catalog sets are derived
/// by filtering `mutates` — do not maintain parallel lists.
const TOOL_INVENTORY: &[ToolInventoryEntry] = &[
    ToolInventoryEntry {
        name: "gsc_performance",
        mutates: false,
        purpose: "Get GSC page-level performance data (clicks, impressions, CTR, position)",
        when_to_use: "When investigating impression trends, CTR changes, or ranking movements",
        when_not_to_use: "Do not use if GSC is not connected",
    },
    ToolInventoryEntry {
        name: "gsc_queries",
        mutates: false,
        purpose: "Get GSC query-level data: which search queries drive traffic to pages",
        when_to_use: "When investigating what queries bring traffic or low CTR",
        when_not_to_use: "Do not use if GSC is not connected",
    },
    ToolInventoryEntry {
        name: "gsc_movers",
        mutates: false,
        purpose: "Compare GSC performance between two periods",
        when_to_use: "When investigating traffic changes or plateau detection",
        when_not_to_use: "Do not use if GSC is not connected",
    },
    ToolInventoryEntry {
        name: "article_list",
        mutates: false,
        purpose: "List all articles with metadata",
        when_to_use: "When you need to know what content exists",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "article_frontmatter",
        mutates: false,
        purpose: "Read frontmatter from MDX files for specific articles",
        when_to_use: "When checking individual article metadata",
        when_not_to_use: "Use article_list first",
    },
    ToolInventoryEntry {
        name: "article_body_hash",
        mutates: false,
        purpose: "Hash article bodies to find exact duplicate content",
        when_to_use: "When investigating duplicate content or SSR fallback pages",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "article_title_scan",
        mutates: false,
        purpose: "Scan all article titles for patterns: duplicated tokens, literal template variables, truncation",
        when_to_use: "When investigating title quality or template bugs",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "content_audit_report",
        mutates: false,
        purpose: "Return the full content_audit.json with 21 checks per article",
        when_to_use: "When you need comprehensive article health data",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "run_content_audit",
        mutates: true,
        purpose: "Run the deterministic content audit and write fresh content_audit.json",
        when_to_use: "When you need fresh audit data",
        when_not_to_use: "If recent audit exists, use content_audit_report instead",
    },
    ToolInventoryEntry {
        name: "cannibalization_clusters",
        mutates: false,
        purpose: "Return cannibalization clusters and merge recommendations",
        when_to_use: "When investigating keyword cannibalization",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "indexing_status",
        mutates: false,
        purpose: "Return GSC URL indexing status",
        when_to_use: "When investigating indexing problems",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "ctr_health",
        mutates: false,
        purpose: "Return per-article CTR health summary",
        when_to_use: "When investigating CTR underperformance",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "framework_files",
        mutates: false,
        purpose: "Read framework config files: layouts, sitemap, robots.txt, redirect rules",
        when_to_use: "When investigating site-wide template bugs",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "article_link_graph",
        mutates: false,
        purpose: "Return the internal link graph",
        when_to_use: "When investigating linking gaps or site structure",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "create_task",
        mutates: true,
        purpose: "Create a fix task in PageSeeds to address issues found",
        when_to_use: "ONLY after investigation found specific, actionable issues",
        when_not_to_use: "Do NOT create tasks speculatively. Max 3 per investigation.",
    },
    ToolInventoryEntry {
        name: "enqueue_task",
        mutates: true,
        purpose: "Enqueue an existing task for execution",
        when_to_use: "After create_task, when the task should run immediately",
        when_not_to_use: "Do not enqueue tasks that still need user review",
    },
    ToolInventoryEntry {
        name: "get_task_status",
        mutates: false,
        purpose: "Get status of a task by ID",
        when_to_use: "When checking whether a previously created task has completed",
        when_not_to_use: "",
    },
    ToolInventoryEntry {
        name: "write_feature_spec",
        mutates: true,
        purpose: "Write a developer feature spec to the target repo's .github/automation/seo_feature_spec.md",
        when_to_use: "When you find code-level issues that require changes to framework files (templates, redirects, sitemap). Each call appends one issue section with file path, current code, and fixed code.",
        when_not_to_use: "Do not use for content-only issues that PageSeeds can auto-fix",
    },
];

fn entry_included(entry: &ToolInventoryEntry, access: InvestigationAccess) -> bool {
    match access {
        InvestigationAccess::Full => true,
        InvestigationAccess::ReadOnly => !entry.mutates,
    }
}

fn tools_for_access(
    ctx: InvestigationContext,
    access: InvestigationAccess,
) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    TOOL_INVENTORY
        .iter()
        .filter(|e| entry_included(e, access))
        .map(|e| build_tool(e.name, ctx.clone()))
        .collect()
}

fn catalog_for_access(access: InvestigationAccess) -> String {
    let header = match access {
        InvestigationAccess::Full => "# Tool catalog for agentic investigation.\n",
        InvestigationAccess::ReadOnly => {
            "# Tool catalog for agentic investigation (read-only).\n"
        }
    };
    let mut s = String::from(header);
    for entry in TOOL_INVENTORY.iter().filter(|e| entry_included(e, access)) {
        s.push_str(&format_catalog_section(entry));
    }
    s
}

fn format_catalog_section(entry: &ToolInventoryEntry) -> String {
    let mut section = format!(
        "\n[tools.{}]\npurpose = \"{}\"\nwhen_to_use = \"{}\"\nwhen_not_to_use = \"{}\"\n",
        entry.name, entry.purpose, entry.when_to_use, entry.when_not_to_use,
    );
    if entry.mutates {
        section.push_str("mutates = true\n");
    }
    section
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
            "TOOL_INVENTORY entry '{other}' has no build_tool match arm — inventory and constructors drifted"
        ),
    }
}

/// Inventory names included for an access mode (for tests / alignment checks).
#[cfg(test)]
pub(crate) fn inventory_names(access: InvestigationAccess) -> Vec<&'static str> {
    TOOL_INVENTORY
        .iter()
        .filter(|e| entry_included(e, access))
        .map(|e| e.name)
        .collect()
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
pub use articles::list_articles_json;
