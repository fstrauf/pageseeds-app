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
pub struct GscPerformanceTool { ctx: InvestigationContext }

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
pub struct GscQueriesTool { ctx: InvestigationContext }

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
pub struct GscMoversTool { ctx: InvestigationContext }

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

/// Standalone: list articles as JSON (shared by Tool trait and CLI).
pub fn list_articles_json(
    ctx: &InvestigationContext, status: Option<&str>, limit: usize,
) -> Result<Vec<serde_json::Value>, InvestigationToolError> {
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    Ok(articles
        .into_iter()
        .filter(|a| status.map_or(true, |s| a.status.to_lowercase() == s.to_lowercase()))
        .take(limit)
        .map(|a| serde_json::json!({
            "id": a.id, "title": a.title, "slug": a.url_slug, "file": a.file,
            "status": a.status, "published_date": a.published_date,
            "target_keyword": a.target_keyword, "word_count": a.word_count,
            "page_type": a.page_type,
        }))
        .collect())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article_list
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleListArgs {
    /// Filter by status (e.g. "published", "draft"). Omit for all.
    pub status: Option<String>,
    /// Max results (default 200)
    #[serde(default = "default_limit_200")]
    pub limit: usize,
}

fn default_limit_200() -> usize { 200 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct ArticleSummary {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub file: String,
    pub status: String,
    pub published_date: Option<String>,
    pub target_keyword: Option<String>,
    pub word_count: i64,
    pub page_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArticleListTool { ctx: InvestigationContext }

impl Tool for ArticleListTool {
    const NAME: &'static str = "article_list";
    type Error = InvestigationToolError;
    type Args = ArticleListArgs;
    type Output = Vec<ArticleSummary>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List all articles with metadata. Use to discover what content exists, \
                filter by status, or get an overview of the site's content inventory.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "Filter by status (published, draft, etc.). Omit for all." },
                    "limit": { "type": "integer", "description": "Max results (default 200)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(list_articles_json(&self.ctx, args.status.as_deref(), args.limit)?
            .into_iter()
            .map(|j| ArticleSummary {
                id: j["id"].as_i64().unwrap_or(0),
                title: j["title"].as_str().unwrap_or("").to_string(),
                slug: j["slug"].as_str().unwrap_or("").to_string(),
                file: j["file"].as_str().unwrap_or("").to_string(),
                status: j["status"].as_str().unwrap_or("").to_string(),
                published_date: j["published_date"].as_str().map(String::from),
                target_keyword: j["target_keyword"].as_str().map(String::from),
                word_count: j["word_count"].as_i64().unwrap_or(0),
                page_type: j["page_type"].as_str().map(String::from),
            })
            .collect())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article_frontmatter
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleFrontmatterArgs {
    /// Article slug (e.g. "my-article") or file path
    pub slug_or_file: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ArticleFrontmatter {
    pub slug: String,
    pub file_name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub published_date: Option<String>,
    pub status: Option<String>,
    pub word_count: usize,
}

#[derive(Debug, Clone)]
pub struct ArticleFrontmatterTool { ctx: InvestigationContext }

impl Tool for ArticleFrontmatterTool {
    const NAME: &'static str = "article_frontmatter";
    type Error = InvestigationToolError;
    type Args = ArticleFrontmatterArgs;
    type Output = ArticleFrontmatter;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read frontmatter from an MDX file by slug or file path. \
                Returns title, description, date, status, and word count. \
                Use article_list first to discover slugs.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug_or_file": { "type": "string", "description": "Article slug or file path" }
                },
                "required": ["slug_or_file"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let content_dir = crate::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Content dir not found: {e}")))?;

        // Try as file path first, then as slug
        let file_path = {
            let direct = paths.repo_root.join(&args.slug_or_file);
            if direct.exists() {
                direct
            } else {
                // Try .mdx and .md variants
                let mdx = content_dir.join(format!("{}.mdx", args.slug_or_file));
                let md = content_dir.join(format!("{}.md", args.slug_or_file));
                if mdx.exists() { mdx } else if md.exists() { md } else {
                    // Try with numeric prefix (e.g. "042_my-article")
                    let slug = args.slug_or_file.trim_start_matches(|c: char| c.is_ascii_digit() || c == '_' || c == '-');
                    let mdx2 = content_dir.join(format!("{}.mdx", slug));
                    if mdx2.exists() { mdx2 } else {
                        return Err(InvestigationToolError::NotAvailable(
                            format!("Article file not found for: {}", args.slug_or_file)
                        ));
                    }
                }
            }
        };

        let meta = crate::content::ops::read_file_metadata(&file_path)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read file: {e}")))?;

        Ok(ArticleFrontmatter {
            slug: meta.url_slug,
            file_name: meta.file_name,
            title: meta.title,
            description: None, // FileMetadata doesn't extract description
            published_date: meta.published_date,
            status: meta.status,
            word_count: meta.word_count,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article_body_hash
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleBodyHashArgs;

#[derive(Debug, Serialize, JsonSchema)]
pub struct DuplicateGroup {
    pub hash: String,
    pub article_count: usize,
    pub articles: Vec<DuplicateArticleRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DuplicateArticleRef {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub file: String,
}

#[derive(Debug, Clone)]
pub struct ArticleBodyHashTool { ctx: InvestigationContext }

impl Tool for ArticleBodyHashTool {
    const NAME: &'static str = "article_body_hash";
    type Error = InvestigationToolError;
    type Args = ArticleBodyHashArgs;
    type Output = Vec<DuplicateGroup>;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Hash all article bodies and find exact duplicate content groups. \
                Articles with identical body hashes are serving the same content — \
                this often indicates SSR fallback pages, template errors, or true duplicates. \
                Groups with 2+ articles indicate a problem.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        use sha2::{Digest, Sha256};

        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;

        let paths = self.ctx.paths();
        let mut hash_groups: std::collections::HashMap<String, Vec<&crate::models::article::Article>> =
            std::collections::HashMap::new();

        for article in &articles {
            let source = crate::engine::exec::utils::read_source_file(&paths.repo_root, &article.file);
            let (_fm, body) = crate::engine::exec::utils::parse_frontmatter(source.as_deref().unwrap_or(""));
            let mut hasher = Sha256::new();
            hasher.update(body.as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            hash_groups.entry(hash).or_default().push(article);
        }

        let groups: Vec<DuplicateGroup> = hash_groups
            .into_iter()
            .filter(|(_, v)| v.len() > 1)
            .map(|(hash, arts)| DuplicateGroup {
                hash,
                article_count: arts.len(),
                articles: arts.iter().map(|a| DuplicateArticleRef {
                    id: a.id, title: a.title.clone(), slug: a.url_slug.clone(), file: a.file.clone(),
                }).collect(),
            })
            .collect();

        Ok(groups)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article_title_scan
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleTitleScanArgs;

#[derive(Debug, Serialize, JsonSchema)]
pub struct TitleScanResult {
    pub total_titles: usize,
    pub missing_titles: usize,
    pub duplicate_token_titles: usize,
    pub literal_var_titles: usize,
    pub long_titles: usize,
    pub examples: Vec<TitleScanExample>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TitleScanExample {
    pub title: String,
    pub slug: String,
    pub issue: String,
}

#[derive(Debug, Clone)]
pub struct ArticleTitleScanTool { ctx: InvestigationContext }

impl Tool for ArticleTitleScanTool {
    const NAME: &'static str = "article_title_scan";
    type Error = InvestigationToolError;
    type Args = ArticleTitleScanArgs;
    type Output = TitleScanResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Scan all article titles for patterns: duplicated tokens, \
                literal template variables (e.g. '| Brand |'), titles that are too long \
                (>60 chars), and missing titles. Returns counts and examples.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;

        let mut missing = 0usize;
        let mut dup_token = 0usize;
        let mut literal_var = 0usize;
        let mut long = 0usize;
        let mut examples: Vec<TitleScanExample> = Vec::new();

        for a in &articles {
            let t = a.title.trim();
            if t.is_empty() {
                missing += 1;
                if examples.len() < 5 {
                    examples.push(TitleScanExample {
                        title: "(empty)".into(), slug: a.url_slug.clone(),
                        issue: "Missing title".into(),
                    });
                }
                continue;
            }

            // Check literal template variables
            let t_lower = t.to_lowercase();
            if t_lower.contains("| brand |") || t_lower.contains("{brand}") || t_lower.contains("{{title}}") {
                literal_var += 1;
                if examples.len() < 5 {
                    examples.push(TitleScanExample {
                        title: t.to_string(), slug: a.url_slug.clone(),
                        issue: "Contains literal template variable".into(),
                    });
                }
            }

            // Check token duplication (any token appears >= 3 times)
            let tokens: Vec<String> = t_lower
                .split(|c: char| !c.is_alphanumeric())
                .filter(|tok| tok.len() > 2)
                .map(String::from)
                .collect();
            let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for tok in &tokens {
                *counts.entry(tok.clone()).or_insert(0) += 1;
            }
            if counts.values().any(|&c| c >= 3) {
                dup_token += 1;
                if examples.len() < 5 {
                    let dup_word = counts.iter().find(|(_, &c)| c >= 3).map(|(w, _)| w.clone()).unwrap_or_default();
                    examples.push(TitleScanExample {
                        title: t.to_string(), slug: a.url_slug.clone(),
                        issue: format!("Token '{}' appears {} times", dup_word, counts[&dup_word]),
                    });
                }
            }

            // Check length
            if t.len() > 60 {
                long += 1;
            }
        }

        Ok(TitleScanResult {
            total_titles: articles.len(),
            missing_titles: missing,
            duplicate_token_titles: dup_token,
            literal_var_titles: literal_var,
            long_titles: long,
            examples,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: content_audit_report
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContentAuditReportArgs;

#[derive(Debug, Clone)]
pub struct ContentAuditReportTool { ctx: InvestigationContext }

impl Tool for ContentAuditReportTool {
    const NAME: &'static str = "content_audit_report";
    type Error = InvestigationToolError;
    type Args = ContentAuditReportArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get the full content_audit.json report with 21 checks per article \
                (keyword usage, meta quality, readability, temporal URLs, page bloat, \
                exact duplicates, literal template variables, title token duplication). \
                Includes health scores and priority rankings.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let audit_path = paths.automation_dir.join("content_audit.json");
        if !audit_path.exists() {
            return Err(InvestigationToolError::NotAvailable(
                "No content_audit.json found. Run run_content_audit first.".into()
            ));
        }
        let content = std::fs::read_to_string(&audit_path)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))?;
        Ok(value)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: run_content_audit
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunContentAuditArgs;

#[derive(Debug, Clone)]
pub struct RunContentAuditTool { ctx: InvestigationContext }

impl Tool for RunContentAuditTool {
    const NAME: &'static str = "run_content_audit";
    type Error = InvestigationToolError;
    type Args = RunContentAuditArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run the 21-check deterministic content audit on all published articles. \
                Writes content_audit.json. Returns summary counts. Must wait for completion \
                before calling content_audit_report to read results.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        use crate::models::task::{
            AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRunPolicy, TaskStatus,
        };

        // Build a minimal task-like struct for the audit function
        let task = Task {
            id: "investigate-audit".to_string(),
            task_type: "content_audit".to_string(),
            project_id: self.ctx.project_id.clone(),
            title: Some("Investigation content audit".to_string()),
            description: None,
            status: TaskStatus::InProgress,
            phase: "audit".to_string(),
            priority: Priority::Medium,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            depends_on: vec![],
            artifacts: vec![],
            run: Default::default(),
        };

        let result = crate::engine::exec::content_audit::exec_content_audit(
            &task, &self.ctx.project_path,
        );

        if !result.success {
            return Err(InvestigationToolError::Execution(result.message));
        }

        // Return the summary
        serde_json::from_str(result.output.as_deref().unwrap_or("{}"))
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to parse audit output: {e}")))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: cannibalization_clusters
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CannibalizationClustersArgs;

#[derive(Debug, Clone)]
pub struct CannibalizationClustersTool { ctx: InvestigationContext }

impl Tool for CannibalizationClustersTool {
    const NAME: &'static str = "cannibalization_clusters";
    type Error = InvestigationToolError;
    type Args = CannibalizationClustersArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get cannibalization clusters and merge recommendations. \
                Shows which articles compete for the same keywords and suggests consolidations. \
                Empty if cannibalization_audit hasn't run yet.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let strategy_path = paths.automation_dir.join("cannibalization_strategy.json");
        if !strategy_path.exists() {
            return Ok(json!({ "clusters": [], "message": "No cannibalization strategy found. Run cannibalization_audit first." }));
        }
        let content = std::fs::read_to_string(&strategy_path)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))?;
        Ok(value)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: indexing_status
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IndexingStatusArgs;

#[derive(Debug, Clone)]
pub struct IndexingStatusTool { ctx: InvestigationContext }

impl Tool for IndexingStatusTool {
    const NAME: &'static str = "indexing_status";
    type Error = InvestigationToolError;
    type Args = IndexingStatusArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get URL indexing status from GSC: how many pages are indexed vs not, \
                reasons for non-indexing, and last inspection dates.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let statuses = crate::gsc::db::list_by_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to load indexing status: {e}")))?;

        let total = statuses.len();
        let indexed = statuses.iter().filter(|s| s.last_reason_code.as_deref() == Some("indexed_pass")).count();
        let not_indexed = total.saturating_sub(indexed);

        let mut reason_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for s in &statuses {
            if let Some(reason) = &s.last_reason_code {
                if reason != "indexed_pass" {
                    *reason_counts.entry(reason.clone()).or_insert(0) += 1;
                }
            }
        }
        let issues_by_reason: Vec<serde_json::Value> = reason_counts
            .into_iter()
            .map(|(reason, count)| json!({ "reason": reason, "count": count }))
            .collect();

        Ok(json!({
            "total_urls": total,
            "indexed": indexed,
            "not_indexed": not_indexed,
            "issues_by_reason": issues_by_reason,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: ctr_health
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CtrHealthArgs;

#[derive(Debug, Clone)]
pub struct CtrHealthTool { ctx: InvestigationContext }

impl Tool for CtrHealthTool {
    const NAME: &'static str = "ctr_health";
    type Error = InvestigationToolError;
    type Args = CtrHealthArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get per-article CTR health: title length, meta description quality, \
                snippet optimization, FAQ schema presence. Shows healthy vs unhealthy counts \
                and specific issues per article.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let project = crate::engine::task_store::get_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Project not found: {e}")))?;
        let project_path = project.path.clone();

        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;

        let repo_root = std::path::Path::new(&project_path);
        let summary = crate::content::ops::build_ctr_health_summary(
            repo_root,
            &articles,
            0,  // pending_fix_tasks
            0,  // completed_audits
            &db,
            &self.ctx.project_id,
        );

        Ok(serde_json::to_value(&summary).unwrap_or(json!({})))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: framework_files
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FrameworkFilesArgs {
    /// Specific file to read (e.g. "app/layout.tsx", "next.config.js"). Omit to list all found.
    pub file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FrameworkFilesTool { ctx: InvestigationContext }

impl Tool for FrameworkFilesTool {
    const NAME: &'static str = "framework_files";
    type Error = InvestigationToolError;
    type Args = FrameworkFilesArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read framework configuration files: Next.js layouts, sitemap config, \
                redirect rules, robots.txt. Use to investigate template bugs, sitemap gaps, \
                or redirect issues. Specify a file to read it, or omit to list all found files.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Specific file to read (optional)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let root = &paths.repo_root;

        // Framework files to look for
        let candidates = [
            ("app/layout.tsx", "Next.js app router layout"),
            ("app/layout.jsx", "Next.js app router layout (JSX)"),
            ("pages/_app.tsx", "Next.js pages router app"),
            ("pages/_document.tsx", "Next.js pages router document"),
            ("next.config.js", "Next.js config"),
            ("next.config.ts", "Next.js config (TS)"),
            ("next.config.mjs", "Next.js config (MJS)"),
            ("next-sitemap.config.js", "Next.js sitemap config"),
            ("app/sitemap.ts", "Next.js app router sitemap"),
            ("robots.txt", "Robots exclusion"),
            ("astro.config.mjs", "Astro config"),
            ("src/layouts/Layout.astro", "Astro layout"),
        ];

        if let Some(ref file) = args.file {
            let file_path = root.join(file);
            if !file_path.exists() {
                return Err(InvestigationToolError::NotAvailable(
                    format!("File not found: {}", file)
                ));
            }
            let content = std::fs::read_to_string(&file_path)
                .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
            // Truncate to 8000 chars for context window
            let truncated = if content.len() > 8000 {
                format!("{}...\n\n[Truncated from {} chars]", &content[..8000], content.len())
            } else {
                content
            };
            Ok(json!({ "file": file, "content": truncated }))
        } else {
            let mut found: Vec<serde_json::Value> = Vec::new();
            for (rel_path, desc) in &candidates {
                let p = root.join(rel_path);
                if p.exists() {
                    found.push(json!({ "path": rel_path, "description": desc, "exists": true }));
                } else {
                    found.push(json!({ "path": rel_path, "description": desc, "exists": false }));
                }
            }
            Ok(json!({ "files": found, "repo_root": root.to_string_lossy() }))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article_link_graph
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleLinkGraphArgs;

#[derive(Debug, Clone)]
pub struct ArticleLinkGraphTool { ctx: InvestigationContext }

impl Tool for ArticleLinkGraphTool {
    const NAME: &'static str = "article_link_graph";
    type Error = InvestigationToolError;
    type Args = ArticleLinkGraphArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get internal link graph: which articles link to which, \
                orphaned articles (no incoming links), and articles with no internal links. \
                Use to find link gaps and site structure issues.".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let content_dir = crate::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
            .map_err(|e| InvestigationToolError::NotAvailable(format!("Content dir not found: {e}")))?;

        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
        drop(db);

        let scan = crate::content::linking::scan_links(&content_dir, &articles)
            .map_err(|e| InvestigationToolError::Execution(format!("Link scan failed: {e}")))?;

        let orphans: Vec<serde_json::Value> = scan.orphan_ids.iter().map(|&id| {
            let a = articles.iter().find(|a| a.id == id);
            json!({ "id": id, "title": a.map(|a| a.title.as_str()).unwrap_or(""), "slug": a.map(|a| a.url_slug.as_str()).unwrap_or("") })
        }).collect();

        let zero_incoming: Vec<serde_json::Value> = scan.zero_incoming_ids.iter().map(|&id| {
            let a = articles.iter().find(|a| a.id == id);
            let profile = scan.profiles.iter().find(|p| p.id == id);
            json!({
                "id": id, "title": a.map(|a| a.title.as_str()).unwrap_or(""),
                "slug": a.map(|a| a.url_slug.as_str()).unwrap_or(""),
                "outgoing_count": profile.map(|p| p.outgoing_ids.len()).unwrap_or(0),
            })
        }).collect();

        Ok(json!({
            "total_articles": scan.total_articles,
            "total_internal_links": scan.total_internal_links,
            "orphan_count": scan.orphan_ids.len(),
            "orphans": orphans,
            "zero_incoming_count": scan.zero_incoming_ids.len(),
            "zero_incoming": zero_incoming,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: create_task
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTaskArgs {
    /// Task type: fix_content_article, consolidate_cluster, content_cleanup, interlinking
    pub task_type: String,
    /// Human-readable title
    pub title: String,
    /// Why this task is needed
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CreateTaskOutput {
    pub task_id: String,
    pub task_type: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct CreateTaskTool { ctx: InvestigationContext }

impl Tool for CreateTaskTool {
    const NAME: &'static str = "create_task";
    type Error = InvestigationToolError;
    type Args = CreateTaskArgs;
    type Output = CreateTaskOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create a fix task in the PageSeeds task system. \
                Valid types: fix_content_article, consolidate_cluster, content_cleanup, \
                interlinking. DO NOT call this without first explaining what the task \
                will do and why. Max 3 tasks per investigation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_type": { "type": "string", "description": "Task type: fix_content_article, consolidate_cluster, content_cleanup, interlinking" },
                    "title": { "type": "string", "description": "Human-readable task title" },
                    "reason": { "type": "string", "description": "Why this task is needed — appears in the task description" }
                },
                "required": ["task_type", "title", "reason"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let valid_types = ["fix_content_article", "consolidate_cluster", "content_cleanup", "interlinking"];
        if !valid_types.contains(&args.task_type.as_str()) {
            return Err(InvestigationToolError::Execution(
                format!("Invalid task_type '{}'. Valid types: {:?}", args.task_type, valid_types)
            ));
        }

        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;

        let task_id = crate::engine::spawner::TaskSpawner::spawn(
            &db,
            crate::engine::spawner::TaskSpec {
                project_id: self.ctx.project_id.clone(),
                task_type: args.task_type.clone(),
                title: Some(args.title.clone()),
                description: Some(args.reason),
                priority: crate::models::task::Priority::Medium,
                ..Default::default()
            },
        )
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to create task: {e}")))?
        .id;

        Ok(CreateTaskOutput {
            task_id: task_id.clone(),
            task_type: args.task_type,
            title: args.title,
            status: "created".to_string(),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: write_feature_spec
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFeatureSpecArgs {
    /// Issue title (e.g., "Title Template Duplicates Brand Name")
    pub title: String,
    /// Severity: critical, warning, or info
    pub severity: String,
    /// Impact description (e.g., "All 150 pages have truncated SERP titles")
    pub impact: String,
    /// Exact file path to edit in the target repo (e.g., "app/layout.tsx")
    pub file_to_edit: String,
    /// What the current code looks like (the problematic code)
    pub current_code: String,
    /// What the fixed code should look like
    pub fixed_code: String,
    /// Additional explanation or context
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WriteFeatureSpecOutput {
    pub path: String,
    pub issue_count: usize,
}

#[derive(Debug, Clone)]
pub struct WriteFeatureSpecTool { ctx: InvestigationContext }

impl Tool for WriteFeatureSpecTool {
    const NAME: &'static str = "write_feature_spec";
    type Error = InvestigationToolError;
    type Args = WriteFeatureSpecArgs;
    type Output = WriteFeatureSpecOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write a developer feature spec to the target repo's .github/automation/seo_feature_spec.md. \
                Use this when you find code-level issues that require changes to the project's framework files \
                (templates, redirects, sitemap config, layouts). Each call appends one issue section to the spec. \
                The developer will read this spec and apply the fixes in their code editor.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Issue title" },
                    "severity": { "type": "string", "description": "critical, warning, or info" },
                    "impact": { "type": "string", "description": "Impact description" },
                    "file_to_edit": { "type": "string", "description": "Exact file path in the target repo" },
                    "current_code": { "type": "string", "description": "The problematic code" },
                    "fixed_code": { "type": "string", "description": "What the fixed code should look like" },
                    "notes": { "type": "string", "description": "Additional context (optional)" }
                },
                "required": ["title", "severity", "impact", "file_to_edit", "current_code", "fixed_code"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let paths = self.ctx.paths();
        let spec_path = paths.automation_dir.join("seo_feature_spec.md");

        let severity_label = match args.severity.as_str() {
            "critical" => "Critical",
            "warning" => "Warning",
            _ => "Info",
        };

        let notes_section = args.notes
            .as_ref()
            .map(|n| format!("\n**Notes:** {n}\n"))
            .unwrap_or_default();

        let section = format!(
            "\n---\n\n## {title}\n\n\
             **Severity:** {severity} | **Impact:** {impact}\n\
             **File to edit:** `{file}`\n\n\
             **Current code:**\n```\n{current}\n```\n\n\
             **Fixed code:**\n```\n{fixed}\n```{notes}\n",
            title = args.title,
            severity = severity_label,
            impact = args.impact,
            file = args.file_to_edit,
            current = args.current_code,
            fixed = args.fixed_code,
            notes = notes_section,
        );

        // Read existing spec or create new
        let header = if spec_path.exists() {
            String::new()
        } else {
            format!(
                "# SEO Feature Specification\n\n\
                 Generated by PageSeeds on {}\n\n\
                 These issues require code changes in this repository. \
                 Each section contains the exact file to edit, the current \
                 problematic code, and the suggested fix.\n",
                chrono::Utc::now().format("%Y-%m-%d")
            )
        };

        let existing = if spec_path.exists() {
            std::fs::read_to_string(&spec_path).unwrap_or_default()
        } else {
            String::new()
        };

        let updated = format!("{header}{existing}{section}");

        // Count issue sections
        let issue_count = updated.matches("\n## ").count();

        std::fs::write(&spec_path, &updated)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to write spec: {e}")))?;

        Ok(WriteFeatureSpecOutput {
            path: spec_path.to_string_lossy().to_string(),
            issue_count,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Standalone tool functions — shared by Tool impls and CLI
// ═══════════════════════════════════════════════════════════════════════════════

/// Scan all article titles and return pattern counts.
pub fn scan_article_titles(ctx: &InvestigationContext) -> Result<serde_json::Value, InvestigationToolError> {
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    let mut missing = 0usize; let mut dup = 0usize; let mut lit = 0usize; let mut long = 0usize;
    let mut examples: Vec<serde_json::Value> = Vec::new();
    for a in &articles {
        let t = a.title.trim();
        if t.is_empty() { missing += 1; continue; }
        let tl = t.to_lowercase();
        if tl.contains("| brand |") || tl.contains("{brand}") || tl.contains("{{title}}") {
            lit += 1;
            if examples.len() < 5 { examples.push(serde_json::json!({"title": t, "slug": a.url_slug, "issue": "literal template variable"})); }
        }
        let tokens: Vec<&str> = tl.split(|c: char| !c.is_alphanumeric()).filter(|s| s.len() > 2).collect();
        let mut counts = std::collections::HashMap::new();
        for tok in &tokens { *counts.entry(*tok).or_insert(0) += 1; }
        if counts.values().any(|&c| c >= 3) {
            dup += 1;
            if examples.len() < 5 {
                let w = counts.iter().find(|(_, &c)| c >= 3).map(|(w, _)| *w).unwrap_or("");
                examples.push(serde_json::json!({"title": t, "slug": a.url_slug, "issue": format!("token '{}' appears {} times", w, counts[w])}));
            }
        }
        if t.len() > 60 { long += 1; }
    }
    Ok(serde_json::json!({
        "total_titles": articles.len(), "missing_titles": missing,
        "duplicate_token_titles": dup, "literal_var_titles": lit,
        "long_titles": long, "examples": examples,
    }))
}

/// Hash all article bodies and find exact duplicate groups.
pub fn hash_article_bodies(ctx: &InvestigationContext) -> Result<Vec<serde_json::Value>, InvestigationToolError> {
    use sha2::{Digest, Sha256};
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    let paths = ctx.paths();
    let mut groups: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    for a in &articles {
        let source = crate::engine::exec::utils::read_source_file(&paths.repo_root, &a.file);
        let (_fm, body) = crate::engine::exec::utils::parse_frontmatter(source.as_deref().unwrap_or(""));
        let mut h = Sha256::new();
        h.update(body.as_bytes());
        let hash = format!("{:x}", h.finalize());
        groups.entry(hash).or_default().push(serde_json::json!({
            "id": a.id, "title": a.title, "slug": a.url_slug, "file": a.file,
        }));
    }
    Ok(groups.into_iter().filter(|(_, v)| v.len() > 1)
        .map(|(hash, arts)| serde_json::json!({"hash": hash, "count": arts.len(), "articles": arts}))
        .collect())
}

/// Read content_audit.json from disk.
pub fn read_content_audit_report(project_path: &str) -> Result<serde_json::Value, InvestigationToolError> {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let p = paths.automation_dir.join("content_audit.json");
    if !p.exists() {
        return Err(InvestigationToolError::NotAvailable("No content_audit.json found. Run run_content_audit first.".into()));
    }
    let s = std::fs::read_to_string(&p)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
    serde_json::from_str(&s)
        .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))
}

/// Read cannibalization_strategy.json from disk.
pub fn read_cannibalization_clusters(project_path: &str) -> Result<serde_json::Value, InvestigationToolError> {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let p = paths.automation_dir.join("cannibalization_strategy.json");
    if !p.exists() { return Ok(serde_json::json!({"clusters": [], "note": "No strategy found. Run cannibalization_audit first."})); }
    let s = std::fs::read_to_string(&p)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
    serde_json::from_str(&s)
        .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))
}

/// Get GSC URL indexing status summary.
pub fn get_indexing_status(ctx: &InvestigationContext) -> Result<serde_json::Value, InvestigationToolError> {
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let statuses = crate::gsc::db::list_by_project(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to load indexing status: {e}")))?;
    let total = statuses.len();
    let indexed = statuses.iter().filter(|s| s.last_reason_code.as_deref() == Some("indexed_pass")).count();
    let mut reasons: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in &statuses {
        if let Some(r) = &s.last_reason_code { if r != "indexed_pass" { *reasons.entry(r.clone()).or_default() += 1; } }
    }
    Ok(serde_json::json!({
        "total_urls": total, "indexed": indexed, "not_indexed": total.saturating_sub(indexed),
        "issues_by_reason": reasons.iter().map(|(k, v)| serde_json::json!({"reason": k, "count": v})).collect::<Vec<_>>(),
    }))
}

/// Read framework files from the project repo.
pub fn read_framework_files(project_path: &str, file: Option<&str>) -> Result<serde_json::Value, InvestigationToolError> {
    let root = std::path::Path::new(project_path);
    let candidates = [
        ("app/layout.tsx", "Next.js app layout"),
        ("pages/_app.tsx", "Next.js pages app"),
        ("next.config.js", "Next.js config"),
        ("next-sitemap.config.js", "Sitemap config"),
        ("app/sitemap.ts", "App router sitemap"),
        ("robots.txt", "Robots exclusion"),
    ];
    if let Some(f) = file {
        let p = root.join(f);
        if !p.exists() { return Err(InvestigationToolError::NotAvailable(format!("File not found: {f}"))); }
        let content = std::fs::read_to_string(&p)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
        let truncated = if content.len() > 8000 { format!("{}...\n[truncated from {} chars]", &content[..8000], content.len()) } else { content };
        Ok(serde_json::json!({"file": f, "content": truncated}))
    } else {
        let found: Vec<serde_json::Value> = candidates.iter().map(|(f, desc)| {
            serde_json::json!({"path": f, "description": desc, "exists": root.join(f).exists()})
        }).collect();
        Ok(serde_json::json!({"files": found, "repo_root": root.to_string_lossy()}))
    }
}

/// Scan internal link graph.
pub fn scan_link_graph(ctx: &InvestigationContext) -> Result<serde_json::Value, InvestigationToolError> {
    let paths = ctx.paths();
    let content_dir = crate::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
        .map_err(|e| InvestigationToolError::NotAvailable(format!("Content dir not found: {e}")))?;
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    drop(db);
    let scan = crate::content::linking::scan_links(&content_dir, &articles)
        .map_err(|e| InvestigationToolError::Execution(format!("Link scan failed: {e}")))?;
    let orphans: Vec<serde_json::Value> = scan.orphan_ids.iter().map(|&id| {
        let a = articles.iter().find(|a| a.id == id);
        serde_json::json!({"id": id, "title": a.map(|a| a.title.as_str()).unwrap_or(""), "slug": a.map(|a| a.url_slug.as_str()).unwrap_or("")})
    }).collect();
    Ok(serde_json::json!({
        "total_articles": scan.total_articles, "total_internal_links": scan.total_internal_links,
        "orphan_count": scan.orphan_ids.len(), "orphans": orphans,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_open_db_invalid_path() {
        let ctx = InvestigationContext {
            project_id: "test".into(),
            project_path: ".".into(),
            db_path: "/nonexistent/test.db".into(),
        };
        assert!(ctx.open_db().is_err());
    }

    #[test]
    fn test_tool_definitions_smoke() {
        let ctx = InvestigationContext {
            project_id: "test".into(),
            project_path: ".".into(),
            db_path: ":memory:".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();

        let tools = investigation_tools(ctx);
        assert_eq!(tools.len(), 16);

        // Verify each tool's definition compiles
        for tool in &tools {
            let def = rt.block_on(async {
                rig::tool::ToolDyn::definition(tool.as_ref(), "test".to_string()).await
            });
            assert!(!def.name.is_empty(), "Tool name must not be empty");
            assert!(!def.description.is_empty(), "Tool description must not be empty for {}", def.name);
        }
    }
}
