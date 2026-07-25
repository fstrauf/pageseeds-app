use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;
use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tool: gsc_performance
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GscPerformanceArgs {
    /// Limit results (default 50, max 200)
    #[serde(default = "default_limit_50")]
    pub limit: usize,
}

fn default_limit_50() -> usize { 50 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct GscPageMetric {
    pub page: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Clone)]
pub struct GscPerformanceTool { pub(crate) ctx: InvestigationContext }

impl Tool for GscPerformanceTool {
    const NAME: &'static str = "gsc_performance";
    type Error = InvestigationToolError;
    type Args = GscPerformanceArgs;
    type Output = Vec<GscPageMetric>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get GSC page-level performance data: clicks, impressions, CTR, position for all pages. \
                Use this to identify top/bottom performers, CTR issues, or ranking patterns.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 50, max 200)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let project = crate::engine::task_store::get_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Project not found: {e}")))?;
        drop(db);

        let site_url = project.site_url.unwrap_or_default();
        if site_url.is_empty() {
            return Err(InvestigationToolError::NotAvailable(
                "No site_url configured for this project".into()
            ));
        }

        let resolver = crate::config::env_resolver::EnvResolver::new(&project.path);
        let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
            .map(|(v, _)| v)
            .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        {
            Some(p) => p,
            None => return Err(InvestigationToolError::NotAvailable(
                "GSC not connected. Set GSC_SERVICE_ACCOUNT_PATH in secrets.".into()
            )),
        };

        let token = crate::gsc::auth::get_service_account_token(&sa_path)
            .await
            .map_err(|e| InvestigationToolError::Execution(format!("GSC auth failed: {e}")))?;

        let end_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let start_date = (chrono::Utc::now() - chrono::Duration::days(90))
            .format("%Y-%m-%d")
            .to_string();

        let metrics = crate::gsc::analytics::fetch_page_rows(
            &token.access_token, &site_url, &start_date, &end_date,
            args.limit.min(200) as u32,
        )
        .await
        .map_err(|e| InvestigationToolError::Execution(format!("GSC API error: {e}")))?;

        Ok(metrics
            .into_iter()
            .map(|m| GscPageMetric {
                page: m.page, clicks: m.clicks, impressions: m.impressions,
                ctr: m.ctr, position: m.position,
            })
            .collect())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: gsc_queries
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GscQueriesArgs {
    /// Filter to a specific page URL (optional; omit for site-wide queries)
    pub page_url: Option<String>,
    /// Limit results (default 50)
    #[serde(default = "default_limit_50")]
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GscQueryMetric {
    pub query: String,
    pub page: Option<String>,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Clone)]
pub struct GscQueriesTool { pub(crate) ctx: InvestigationContext }

impl Tool for GscQueriesTool {
    const NAME: &'static str = "gsc_queries";
    type Error = InvestigationToolError;
    type Args = GscQueriesArgs;
    type Output = Vec<GscQueryMetric>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get GSC query-level data: which search queries drive traffic. \
                Filter by page URL for page-specific queries, or omit for site-wide top queries. \
                Use to find low-CTR queries, cannibalization signals, or keyword gaps.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "page_url": { "type": "string", "description": "Filter to a specific page (optional)" },
                    "limit": { "type": "integer", "description": "Max results (default 50)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let project = crate::engine::task_store::get_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Project not found: {e}")))?;
        drop(db);

        let site_url = project.site_url.unwrap_or_default();
        if site_url.is_empty() {
            return Err(InvestigationToolError::NotAvailable("No site_url configured".into()));
        }

        let resolver = crate::config::env_resolver::EnvResolver::new(&project.path);
        let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
            .map(|(v, _)| v)
            .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        {
            Some(p) => p,
            None => return Err(InvestigationToolError::NotAvailable("GSC not connected".into())),
        };

        let token = crate::gsc::auth::get_service_account_token(&sa_path)
            .await
            .map_err(|e| InvestigationToolError::Execution(format!("GSC auth failed: {e}")))?;

        let end_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let start_date = (chrono::Utc::now() - chrono::Duration::days(90))
            .format("%Y-%m-%d")
            .to_string();

        let metrics = if let Some(ref page_url) = args.page_url {
            crate::gsc::analytics::fetch_queries_for_page(
                &token.access_token, &site_url, page_url, &start_date, &end_date,
                args.limit.min(200) as u32,
            )
            .await
            .map_err(|e| InvestigationToolError::Execution(format!("GSC API error: {e}")))?
            .into_iter()
            .map(|m| GscQueryMetric {
                query: m.query, page: Some(page_url.clone()),
                clicks: m.clicks, impressions: m.impressions, ctr: m.ctr, position: m.position,
            })
            .collect()
        } else {
            crate::gsc::analytics::fetch_page_query_rows(
                &token.access_token, &site_url, &start_date, &end_date,
                args.limit.min(200) as u32,
            )
            .await
            .map_err(|e| InvestigationToolError::Execution(format!("GSC API error: {e}")))?
            .into_iter()
            .map(|m| GscQueryMetric {
                query: m.query, page: Some(m.page),
                clicks: m.clicks, impressions: m.impressions, ctr: m.ctr, position: m.position,
            })
            .collect()
        };

        Ok(metrics)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: gsc_movers
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GscMoversArgs {
    /// Limit results (default 30)
    #[serde(default = "default_limit_30")]
    pub limit: usize,
}

fn default_limit_30() -> usize { 30 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct GscMover {
    pub key: String,
    pub current_clicks: f64, pub current_impressions: f64, pub current_position: f64,
    pub previous_clicks: f64, pub previous_impressions: f64, pub previous_position: f64,
    pub clicks_delta: f64, pub impressions_delta: f64, pub position_delta: f64,
}

#[derive(Debug, Clone)]
pub struct GscMoversTool { pub(crate) ctx: InvestigationContext }

impl Tool for GscMoversTool {
    const NAME: &'static str = "gsc_movers";
    type Error = InvestigationToolError;
    type Args = GscMoversArgs;
    type Output = Vec<GscMover>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Compare GSC performance between last 30 days and previous 30 days. \
                Finds gaining/declining pages and queries. Use to detect plateaus, drops, \
                or post-change impacts.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 30)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let project = crate::engine::task_store::get_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Project not found: {e}")))?;
        drop(db);

        let site_url = project.site_url.unwrap_or_default();
        if site_url.is_empty() {
            return Err(InvestigationToolError::NotAvailable("No site_url configured".into()));
        }

        let resolver = crate::config::env_resolver::EnvResolver::new(&project.path);
        let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
            .map(|(v, _)| v)
            .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
        {
            Some(p) => p,
            None => return Err(InvestigationToolError::NotAvailable("GSC not connected".into())),
        };

        let token = crate::gsc::auth::get_service_account_token(&sa_path)
            .await
            .map_err(|e| InvestigationToolError::Execution(format!("GSC auth failed: {e}")))?;

        let now = chrono::Utc::now();
        let curr_end = now.format("%Y-%m-%d").to_string();
        let curr_start = (now - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
        let prev_end = (now - chrono::Duration::days(31)).format("%Y-%m-%d").to_string();
        let prev_start = (now - chrono::Duration::days(61)).format("%Y-%m-%d").to_string();

        let movers = crate::gsc::analytics::compute_movers(
            &token.access_token, &site_url,
            &curr_start, &curr_end,
            &prev_start, &prev_end,
            args.limit.min(200) as u32,
        )
        .await
        .map_err(|e| InvestigationToolError::Execution(format!("GSC movers error: {e}")))?;

        Ok(movers.into_iter().map(|m| GscMover {
            key: m.key,
            current_clicks: m.current_clicks, current_impressions: m.current_impressions,
            current_position: m.current_position,
            previous_clicks: m.previous_clicks, previous_impressions: m.previous_impressions,
            previous_position: m.previous_position,
            clicks_delta: m.clicks_delta, impressions_delta: m.impressions_delta,
            position_delta: m.position_delta,
        }).collect())
    }
}
