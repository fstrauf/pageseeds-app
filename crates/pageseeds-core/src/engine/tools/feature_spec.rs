//! Feature spec generation tools — focused Rig tool set for the agentic
//! spec generator. Each tool wraps existing deterministic module functions
//! and returns typed ground-truth data.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::tools::{InvestigationContext, InvestigationToolError};

/// Build the tool set for the feature spec agent.
pub fn feature_spec_tools(ctx: InvestigationContext) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    vec![
        Box::new(ArticleIndexTool { ctx: ctx.clone() }),
        Box::new(ReadArticleTool { ctx: ctx.clone() }),
        Box::new(GscNotIndexedTool { ctx: ctx.clone() }),
        Box::new(AnalyzeTitleTool { ctx: ctx.clone() }),
        Box::new(CheckTemporalUrlTool { ctx: ctx.clone() }),
        Box::new(LinkGraphSummaryTool { ctx: ctx.clone() }),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: article_index
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArticleIndexArgs {
    /// Optional status filter: "published", "draft", "live", or omit for all
    #[serde(default)]
    pub status_filter: Option<String>,
    /// Max articles to return (default 20). Increase only if you need more.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ArticleIndexEntry {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub db_path: String,
    pub word_count: i64,
    pub status: String,
    pub published_date: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ArticleIndexOutput {
    pub articles: Vec<ArticleIndexEntry>,
    pub total: usize,
    pub returned: usize,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct ArticleIndexTool {
    pub ctx: InvestigationContext,
}

impl Tool for ArticleIndexTool {
    const NAME: &'static str = "article_index";
    type Error = InvestigationToolError;
    type Args = ArticleIndexArgs;
    type Output = ArticleIndexOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List the most suspect articles in the project (sorted by risk: low word count first, then non-published). \
                Returns a curated subset so you can focus on problematic content.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status_filter": { "type": "string", "description": "Optional: published, draft, live" },
                    "limit": { "type": "integer", "description": "Maximum articles to return (default 20, max 30)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;

        let limit = args.limit.unwrap_or(20).min(30);

        let mut entries: Vec<ArticleIndexEntry> = articles
            .into_iter()
            .filter(|a| {
                if let Some(ref filter) = args.status_filter {
                    a.status.eq_ignore_ascii_case(filter)
                } else {
                    true
                }
            })
            .map(|a| ArticleIndexEntry {
                id: a.id,
                slug: a.url_slug,
                title: a.title,
                db_path: a.file,
                word_count: a.word_count,
                status: a.status,
                published_date: a.published_date,
            })
            .collect();

        let total = entries.len();

        // Sort: low word count first, then non-published status, then by slug for stability
        entries.sort_by(|a, b| {
            let a_suspect = a.word_count < 100 || a.status != "published";
            let b_suspect = b.word_count < 100 || b.status != "published";
            match (a_suspect, b_suspect) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.word_count.cmp(&b.word_count).then_with(|| a.slug.cmp(&b.slug)),
            }
        });

        entries.truncate(limit);
        let returned = entries.len();
        let note = if total > returned {
            format!("Showing {returned} of {total} articles (most suspect first). Focus on these.")
        } else {
            format!("All {total} articles returned.")
        };

        Ok(ArticleIndexOutput { articles: entries, total, returned, note })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: read_article
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArticleArgs {
    /// URL slug of the article to read (e.g. "french-press-coffee-brewing-guide")
    pub slug: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadArticleOutput {
    pub slug: String,
    pub db_path: String,
    pub actual_path: Option<String>,
    pub found: bool,
    pub was_repaired: bool,
    pub file_size_bytes: u64,
    pub word_count: usize,
    pub title: String,
    pub description: Option<String>,
    pub published_date: Option<String>,
    pub status: String,
    pub frontmatter: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ReadArticleTool {
    pub ctx: InvestigationContext,
}

impl Tool for ReadArticleTool {
    const NAME: &'static str = "read_article";
    type Error = InvestigationToolError;
    type Args = ReadArticleArgs;
    type Output = ReadArticleOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read an MDX article from disk using path resolution. \
                Returns the ACTUAL path found, file size, word count, and frontmatter. \
                Use this to verify file existence and inspect content. \
                The tool searches known content directories and repairs moved files.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "URL slug of the article" }
                },
                "required": ["slug"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let repo_root = std::path::Path::new(&self.ctx.project_path);

        // Find the article in the DB
        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("DB error: {e}")))?;

        let article = articles.into_iter()
            .find(|a| a.url_slug == args.slug)
            .ok_or_else(|| InvestigationToolError::NotAvailable(format!("Article '{}' not found in DB", args.slug)))?;

        // Use article_resolver to find the actual file
        let content_dirs_raw = crate::content::article_resolver::discover_content_dirs(repo_root);
        let content_dirs: Vec<&str> = content_dirs_raw.iter().map(|s| s.as_str()).collect();
        let resolved = crate::content::article_resolver::resolve_article_file(
            repo_root,
            &article.file,
            &content_dirs,
        );

        let (found, actual_path, file_size, word_count, frontmatter_json) = if resolved.found {
            let abs = resolved._absolute_path;
            let meta = crate::content::ops::read_file_metadata(&abs)
                .map_err(|e| InvestigationToolError::Execution(format!("Failed to read file: {e}")))?;

            let mut fm = serde_json::Map::new();
            fm.insert("title".to_string(), json!(meta.title));
            fm.insert("date".to_string(), json!(meta.published_date));
            fm.insert("status".to_string(), json!(meta.status));

            (
                true,
                Some(resolved.relative_path),
                abs.metadata().map(|m| m.len()).unwrap_or(0),
                meta.word_count,
                serde_json::Value::Object(fm),
            )
        } else {
            (false, None, 0, 0, serde_json::Value::Object(serde_json::Map::new()))
        };

        Ok(ReadArticleOutput {
            slug: article.url_slug,
            db_path: article.file,
            actual_path,
            found,
            was_repaired: resolved.was_repaired,
            file_size_bytes: file_size,
            word_count,
            title: article.title,
            description: None,
            published_date: article.published_date,
            status: article.status,
            frontmatter: frontmatter_json,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: gsc_not_indexed
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GscNotIndexedArgs {
    /// Limit results (default 50)
    #[serde(default = "default_limit_50")]
    pub limit: usize,
}

fn default_limit_50() -> usize { 50 }

#[derive(Debug, Serialize, JsonSchema)]
pub struct GscNotIndexedEntry {
    pub url: String,
    pub reason_code: String,
    pub last_crawled: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GscNotIndexedOutput {
    pub urls: Vec<GscNotIndexedEntry>,
    pub total: usize,
}

#[derive(Debug, Clone)]
pub struct GscNotIndexedTool {
    pub ctx: InvestigationContext,
}

impl Tool for GscNotIndexedTool {
    const NAME: &'static str = "gsc_not_indexed";
    type Error = InvestigationToolError;
    type Args = GscNotIndexedArgs;
    type Output = GscNotIndexedOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get URLs that Google Search Console reports as not indexed. \
                Use this to identify indexing gaps and crawl issues.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 50)" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;

        let statuses = crate::gsc::db::list_by_project(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("GSC DB error: {e}")))?;

        let urls: Vec<GscNotIndexedEntry> = statuses
            .into_iter()
            .filter(|s| s.last_verdict.as_deref() != Some("PASS"))
            .take(args.limit)
            .map(|s| GscNotIndexedEntry {
                url: s.url,
                reason_code: s.last_reason_code.clone().unwrap_or_default(),
                last_crawled: s.last_inspected_at.clone(),
            })
            .collect();

        let total = urls.len();
        Ok(GscNotIndexedOutput { urls, total })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: analyze_title
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeTitleArgs {
    /// Title text to analyze
    pub title: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AnalyzeTitleOutput {
    pub tokens: Vec<String>,
    pub token_counts: serde_json::Value,
    pub max_token_count: usize,
    pub has_duplication: bool,
    pub duplicated_tokens: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AnalyzeTitleTool {
    pub ctx: InvestigationContext,
}

impl Tool for AnalyzeTitleTool {
    const NAME: &'static str = "analyze_title";
    type Error = InvestigationToolError;
    type Args = AnalyzeTitleArgs;
    type Output = AnalyzeTitleOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Analyze a page title for token duplication. \
                Returns each token, its frequency, and whether any token appears 2+ times. \
                Use this to detect titles like 'Coffee Coffee Beans Guide' where 'coffee' repeats.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Title text to analyze" }
                },
                "required": ["title"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let title_lower = args.title.to_lowercase();
        let tokens: Vec<String> = title_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty() && t.len() > 2)
            .map(String::from)
            .collect();

        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for token in &tokens {
            *counts.entry(token.clone()).or_insert(0) += 1;
        }

        let max_count = counts.values().copied().max().unwrap_or(0);
        let has_duplication = max_count >= 2;
        let duplicated_tokens: Vec<String> = counts
            .iter()
            .filter(|(_, &c)| c >= 2)
            .map(|(t, _)| t.clone())
            .collect();

        let token_counts_json = serde_json::to_value(&counts)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));

        Ok(AnalyzeTitleOutput {
            tokens: tokens.clone(),
            token_counts: token_counts_json,
            max_token_count: max_count,
            has_duplication,
            duplicated_tokens,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: check_temporal_url
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckTemporalUrlArgs {
    /// URL slug to check (e.g. "best-coffee-deals-september-2025")
    pub slug: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckTemporalUrlOutput {
    pub slug: String,
    pub is_temporal: bool,
    pub matched_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CheckTemporalUrlTool {
    pub ctx: InvestigationContext,
}

impl Tool for CheckTemporalUrlTool {
    const NAME: &'static str = "check_temporal_url";
    type Error = InvestigationToolError;
    type Args = CheckTemporalUrlArgs;
    type Output = CheckTemporalUrlOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Check whether a URL slug contains temporal patterns (year, month, season, relative time). \
                Use this to identify URLs that will decay in search relevance.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "URL slug to check" }
                },
                "required": ["slug"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let slug_lower = args.slug.to_lowercase();

        // Same patterns as content_audit.rs
        let patterns = [
            (r"\b20\d{2}\b", "year"),
            (r"\b(january|february|march|april|may|june|july|august|september|october|november|december)\b", "month"),
            (r"\b(jan|feb|mar|apr|jun|jul|aug|sep|oct|nov|dec)\b", "month_abbrev"),
            (r"\b(spring|summer|autumn|fall|winter)\b", "season"),
            (r"\b(this-week|this-weeks|next-week|last-week)\b", "relative_week"),
            (r"\b(today|tomorrow|yesterday|now|current)\b", "relative_time"),
        ];

        let mut matched = Vec::new();
        for (pat, label) in &patterns {
            if let Ok(re) = regex::Regex::new(pat) {
                if re.is_match(&slug_lower) {
                    matched.push(label.to_string());
                }
            }
        }

        Ok(CheckTemporalUrlOutput {
            slug: args.slug,
            is_temporal: !matched.is_empty(),
            matched_patterns: matched,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool: link_graph_summary
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinkGraphSummaryArgs {
    /// Optional slug to get per-article link data; omit for full summary
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkGraphArticle {
    pub slug: String,
    pub incoming_count: usize,
    pub outgoing_count: usize,
    pub is_orphan: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkGraphSummaryOutput {
    pub total_articles: usize,
    pub total_links: usize,
    pub orphan_count: usize,
    pub zero_incoming_count: usize,
    pub articles: Vec<LinkGraphArticle>,
}

#[derive(Debug, Clone)]
pub struct LinkGraphSummaryTool {
    pub ctx: InvestigationContext,
}

impl Tool for LinkGraphSummaryTool {
    const NAME: &'static str = "link_graph_summary";
    type Error = InvestigationToolError;
    type Args = LinkGraphSummaryArgs;
    type Output = LinkGraphSummaryOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get the internal link graph summary: orphans, zero-incoming pages, per-article link counts. \
                Use this to find structurally isolated pages.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Optional: filter to one slug" }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = self.ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
        let repo_root = std::path::Path::new(&self.ctx.project_path);
        let content_dir = crate::content::ops::resolve_content_dir(repo_root, repo_root)
            .map_err(|e| InvestigationToolError::Execution(format!("No content dir: {e}")))?;

        let articles = crate::engine::task_store::list_articles(&db, &self.ctx.project_id)
            .map_err(|e| InvestigationToolError::Execution(format!("DB error: {e}")))?;

        let scan = crate::content::linking::scan_links(&content_dir, &articles)
            .map_err(|e| InvestigationToolError::Execution(format!("Link scan failed: {e}")))?;

        let article_summaries: Vec<LinkGraphArticle> = scan
            .profiles
            .into_iter()
            .filter(|a| {
                if let Some(ref slug) = args.slug {
                    a.file.contains(slug)
                } else {
                    true
                }
            })
            .map(|a| LinkGraphArticle {
                slug: a.file,
                incoming_count: a.incoming_ids.len(),
                outgoing_count: a.outgoing_ids.len(),
                is_orphan: scan.orphan_ids.contains(&a.id),
            })
            .collect();

        Ok(LinkGraphSummaryOutput {
            total_articles: scan.total_articles,
            total_links: scan.total_internal_links,
            orphan_count: scan.orphan_ids.len(),
            zero_incoming_count: scan.zero_incoming_ids.len(),
            articles: article_summaries,
        })
    }
}
