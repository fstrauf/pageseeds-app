use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;
use super::*;
use super::shared::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tool: framework_files
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FrameworkFilesArgs {
    /// Specific file to read (e.g. "app/layout.tsx", "next.config.js"). Omit to list all found.
    pub file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FrameworkFilesTool { pub(crate) ctx: InvestigationContext }

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
pub struct ArticleLinkGraphTool { pub(crate) ctx: InvestigationContext }

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
pub struct CreateTaskTool { pub(crate) ctx: InvestigationContext }

impl Tool for CreateTaskTool {
    const NAME: &'static str = "create_task";
    type Error = InvestigationToolError;
    type Args = CreateTaskArgs;
    type Output = CreateTaskOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create a task in the PageSeeds task system. \
                Valid types: fix_content_article, consolidate_cluster, content_cleanup, \
                interlinking, seo_health_scan. DO NOT call this without first explaining \
                what the task will do and why. Max 3 tasks per investigation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_type": { "type": "string", "description": "Task type: fix_content_article, consolidate_cluster, content_cleanup, interlinking, seo_health_scan" },
                    "title": { "type": "string", "description": "Human-readable task title" },
                    "reason": { "type": "string", "description": "Why this task is needed — appears in the task description" }
                },
                "required": ["task_type", "title", "reason"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let valid_types = ["fix_content_article", "consolidate_cluster", "content_cleanup", "interlinking", "seo_health_scan"];
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
pub struct WriteFeatureSpecTool { pub(crate) ctx: InvestigationContext }

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
