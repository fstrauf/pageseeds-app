use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;
use super::*;
use super::shared::*;
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
pub struct ArticleListTool { pub(crate) ctx: InvestigationContext }

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
pub struct ArticleFrontmatterTool { pub(crate) ctx: InvestigationContext }

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
        // Shared resolver: path as written, then NN_slug.mdx / normalized stem / frontmatter.
        let file_path = crate::content::ops::resolve_slug_or_path(&paths.repo_root, &args.slug_or_file)
            .map_err(|e| InvestigationToolError::NotAvailable(e))?;

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
pub struct ArticleBodyHashTool { pub(crate) ctx: InvestigationContext }

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
pub struct ArticleTitleScanTool { pub(crate) ctx: InvestigationContext }

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

            // Build token counts for duplication detection
            let t_lower = t.to_lowercase();
            let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for word in t_lower.split_whitespace() {
                let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());
                if cleaned.len() > 2 {
                    *counts.entry(cleaned.to_string()).or_insert(0) += 1;
                }
            }

            // Check literal template variables
            if t_lower.contains("| brand |") || t_lower.contains("{brand}") || t_lower.contains("{{title}}") {
                literal_var += 1;
                if examples.len() < 5 {
                    examples.push(TitleScanExample {
                        title: t.to_string(), slug: a.url_slug.clone(),
                        issue: "Contains literal template variable".into(),
                    });
                }
            }

            // Check token duplication (any token appears >= 2 times — brand dup, keyword stuffing)
            if counts.values().any(|&c| c >= 2) {
                dup_token += 1;
                if examples.len() < 5 {
                    let dup_word = counts.iter().find(|(_, &c)| c >= 2).map(|(w, _)| w.clone()).unwrap_or_default();
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
// Tool: validate_article
// ═══════════════════════════════════════════════════════════════════════════════

/// Standalone: run structural SEO gates (shared by Rig tool + CLI).
pub fn validate_article_json(
    ctx: &InvestigationContext,
    slug: &str,
) -> Result<crate::content::validate_article::ValidateArticleResult, InvestigationToolError> {
    let db = ctx
        .open_db()
        .map_err(InvestigationToolError::Execution)?;
    let project_path = std::path::Path::new(&ctx.project_path);
    crate::content::validate_article::validate_article(&db, &ctx.project_id, project_path, slug)
        .map_err(|e| InvestigationToolError::Execution(e.to_string()))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateArticleArgs {
    /// Article url_slug (or path resolvable via resolve_slug_or_path)
    pub slug: String,
}

#[derive(Debug, Clone)]
pub struct ValidateArticleTool {
    pub(crate) ctx: InvestigationContext,
}

impl Tool for ValidateArticleTool {
    const NAME: &'static str = "validate_article";
    type Error = InvestigationToolError;
    type Args = ValidateArticleArgs;
    type Output = crate::content::validate_article::ValidateArticleResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run deterministic structural SEO quality gates on one article by slug. \
                Checks MDX structure, H1, frontmatter title, meta description length (120–155), \
                target keyword in body, internal /blog/ link resolution, and min word count (≥800). \
                Returns ok + per-check pass/fail. Not for prose/strategy judgments."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": { "type": "string", "description": "Article url_slug (e.g. best-cold-brew-maker)" }
                },
                "required": ["slug"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        validate_article_json(&self.ctx, &args.slug)
    }
}

