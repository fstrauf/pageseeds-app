//! Investigate Rig tools for Site State desk reads (issue #120).
//!
//! Thin wrappers around `engine::site_state` builders — no business logic here.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::engine::site_state::{
    self, ArticlePackage, ArticlesCatalog, ArticlesFilter, SiteOverview, DEFAULT_PERIOD_DAYS,
};

use super::{InvestigationContext, InvestigationToolError};

fn map_err(e: crate::error::Error) -> InvestigationToolError {
    InvestigationToolError::Execution(e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: site_overview
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SiteOverviewArgs {
    /// GSC rollup window in days (default 28).
    #[serde(default = "default_period_days")]
    pub period_days: i64,
}

fn default_period_days() -> i64 {
    DEFAULT_PERIOD_DAYS
}

#[derive(Debug, Clone)]
pub struct SiteOverviewTool {
    pub(crate) ctx: InvestigationContext,
}

impl Tool for SiteOverviewTool {
    const NAME: &'static str = "site_overview";
    type Error = InvestigationToolError;
    type Args = SiteOverviewArgs;
    type Output = SiteOverview;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Compact site-wide SEO desk: totals, top pages, movers, indexing sample, \
                and deterministic health hints. Start here for weekly SEO exploration."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "period_days": {
                        "type": "integer",
                        "description": "GSC rollup window in days (default 28)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self
            .ctx
            .open_db()
            .map_err(InvestigationToolError::Execution)?;
        site_state::build_site_overview(
            &db,
            &self.ctx.project_id,
            &self.ctx.project_path,
            Some(args.period_days),
        )
        .map_err(map_err)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: articles
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticlesArgs {
    /// Filter by status (e.g. "published", "draft"). Omit for all.
    pub status: Option<String>,
    /// Minimum impressions in the GSC window (default 0).
    #[serde(default)]
    pub min_impressions: f64,
    /// Include redirected slugs (default false).
    #[serde(default)]
    pub include_redirected: bool,
    /// Max results (default 200).
    #[serde(default = "default_limit_200")]
    pub limit: usize,
    /// GSC rollup window in days (default 28).
    #[serde(default = "default_period_days")]
    pub period_days: i64,
}

fn default_limit_200() -> usize {
    200
}

#[derive(Debug, Clone)]
pub struct ArticlesTool {
    pub(crate) ctx: InvestigationContext,
}

impl Tool for ArticlesTool {
    const NAME: &'static str = "articles";
    type Error = InvestigationToolError;
    type Args = ArticlesArgs;
    type Output = ArticlesCatalog;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Article catalog with GSC rollup and filters. Redirected articles are \
                excluded by default. Use article for full content of one page."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "Filter by status (published, draft, etc.). Omit for all."
                    },
                    "min_impressions": {
                        "type": "number",
                        "description": "Minimum impressions in the GSC window (default 0)"
                    },
                    "include_redirected": {
                        "type": "boolean",
                        "description": "Include redirected slugs (default false)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 200)"
                    },
                    "period_days": {
                        "type": "integer",
                        "description": "GSC rollup window in days (default 28)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self
            .ctx
            .open_db()
            .map_err(InvestigationToolError::Execution)?;
        site_state::list_articles_catalog(
            &db,
            &self.ctx.project_id,
            &self.ctx.project_path,
            ArticlesFilter {
                status: args.status,
                min_impressions: args.min_impressions,
                include_redirected: args.include_redirected,
                limit: Some(args.limit),
                period_days: Some(args.period_days),
            },
        )
        .map_err(map_err)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleArgs {
    /// Article url_slug (e.g. "my-article").
    pub slug: String,
    /// GSC rollup window in days (default 28).
    #[serde(default = "default_period_days")]
    pub period_days: i64,
}

#[derive(Debug, Clone)]
pub struct ArticleTool {
    pub(crate) ctx: InvestigationContext,
}

impl Tool for ArticleTool {
    const NAME: &'static str = "article";
    type Error = InvestigationToolError;
    type Args = ArticleArgs;
    type Output = ArticlePackage;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Full package for one article: catalog row, body/outline, GSC queries, \
                query cannibalization, and empty-safe neighbors. Use when investigating a specific page."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": {
                        "type": "string",
                        "description": "Article url_slug (e.g. my-article)"
                    },
                    "period_days": {
                        "type": "integer",
                        "description": "GSC rollup window in days (default 28)"
                    }
                },
                "required": ["slug"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self
            .ctx
            .open_db()
            .map_err(InvestigationToolError::Execution)?;
        site_state::get_article_package(
            &db,
            &self.ctx.project_id,
            &self.ctx.project_path,
            &args.slug,
            Some(args.period_days),
        )
        .map_err(map_err)
    }
}
