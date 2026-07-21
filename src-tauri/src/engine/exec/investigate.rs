//! Agentic investigation executor.
//!
//! Builds a rig agent with project data tools attached and runs an open-ended
//! investigation. The agent has access to GSC, articles, audit data, indexing
//! status, link graph, framework files, and more. It calls tools freely to
//! answer the user's question.
//!
//! Catalog modes:
//! - **Full** — standalone investigate (includes mutators).
//! - **Read-only** — for unattended in-workflow use (no mutators; see #79/#80).

use crate::engine::tools::{investigation_tools, InvestigationContext};
use rig::completion::Prompt;

/// Run a prompt on a tool-equipped agent and return the response string.
async fn run_tool_agent<A: Prompt + Send>(agent: A, prompt: &str) -> Result<String, String> {
    agent.prompt(prompt).await.map_err(|e| format!("Agent error: {e}"))
}

/// Which tool catalog to embed in the agent preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogMode {
    /// Full catalog including mutators (standalone investigate).
    Full,
    /// Read-only tools only (in-workflow unattended agents; wired in #80).
    #[allow(dead_code)] // used by tests; production callers land in #80
    ReadOnly,
}

/// Shared read-only tool entries (no mutators, no `mutates = true`).
const TOOL_CATALOG_READ_ONLY_ENTRIES: &str = r#"
[tools.gsc_performance]
purpose = "Get GSC page-level performance data (clicks, impressions, CTR, position)"
when_to_use = "When investigating impression trends, CTR changes, or ranking movements"
when_not_to_use = "Do not use if GSC is not connected"

[tools.gsc_queries]
purpose = "Get GSC query-level data: which search queries drive traffic to pages"
when_to_use = "When investigating what queries bring traffic or low CTR"
when_not_to_use = "Do not use if GSC is not connected"

[tools.gsc_movers]
purpose = "Compare GSC performance between two periods"
when_to_use = "When investigating traffic changes or plateau detection"
when_not_to_use = "Do not use if GSC is not connected"

[tools.article_list]
purpose = "List all articles with metadata"
when_to_use = "When you need to know what content exists"
when_not_to_use = ""

[tools.article_frontmatter]
purpose = "Read frontmatter from MDX files for specific articles"
when_to_use = "When checking individual article metadata"
when_not_to_use = "Use article_list first"

[tools.article_body_hash]
purpose = "Hash article bodies to find exact duplicate content"
when_to_use = "When investigating duplicate content or SSR fallback pages"
when_not_to_use = ""

[tools.article_title_scan]
purpose = "Scan all article titles for patterns: duplicated tokens, literal template variables, truncation"
when_to_use = "When investigating title quality or template bugs"
when_not_to_use = ""

[tools.content_audit_report]
purpose = "Return the full content_audit.json with 21 checks per article"
when_to_use = "When you need comprehensive article health data"
when_not_to_use = ""

[tools.cannibalization_clusters]
purpose = "Return cannibalization clusters and merge recommendations"
when_to_use = "When investigating keyword cannibalization"
when_not_to_use = ""

[tools.indexing_status]
purpose = "Return GSC URL indexing status"
when_to_use = "When investigating indexing problems"
when_not_to_use = ""

[tools.ctr_health]
purpose = "Return per-article CTR health summary"
when_to_use = "When investigating CTR underperformance"
when_not_to_use = ""

[tools.framework_files]
purpose = "Read framework config files: layouts, sitemap, robots.txt, redirect rules"
when_to_use = "When investigating site-wide template bugs"
when_not_to_use = ""

[tools.article_link_graph]
purpose = "Return the internal link graph"
when_to_use = "When investigating linking gaps or site structure"
when_not_to_use = ""

[tools.get_task_status]
purpose = "Get status of a task by ID"
when_to_use = "When checking whether a previously created task has completed"
when_not_to_use = ""
"#;

/// Mutator-only catalog entries for standalone investigate.
const TOOL_CATALOG_MUTATOR_ENTRIES: &str = r#"
[tools.run_content_audit]
purpose = "Run the deterministic content audit and write fresh content_audit.json"
when_to_use = "When you need fresh audit data"
when_not_to_use = "If recent audit exists, use content_audit_report instead"
mutates = true

[tools.create_task]
purpose = "Create a fix task in PageSeeds to address issues found"
when_to_use = "ONLY after investigation found specific, actionable issues"
when_not_to_use = "Do NOT create tasks speculatively. Max 3 per investigation."
mutates = true

[tools.enqueue_task]
purpose = "Enqueue an existing task for execution"
when_to_use = "After create_task, when the task should run immediately"
when_not_to_use = "Do not enqueue tasks that still need user review"
mutates = true

[tools.write_feature_spec]
purpose = "Write a developer feature spec to the target repo's .github/automation/seo_feature_spec.md"
when_to_use = "When you find code-level issues that require changes to framework files (templates, redirects, sitemap). Each call appends one issue section with file path, current code, and fixed code."
when_not_to_use = "Do not use for content-only issues that PageSeeds can auto-fix"
mutates = true
"#;

/// Tool catalog text for the given mode.
///
/// - [`CatalogMode::Full`] — all tools including mutators (standalone investigate).
/// - [`CatalogMode::ReadOnly`] — read-only tools only (in-workflow; no mutators).
pub(crate) fn tool_catalog_text(mode: CatalogMode) -> String {
    match mode {
        CatalogMode::Full => {
            let mut s = String::from("# Tool catalog for agentic investigation.\n");
            s.push_str(TOOL_CATALOG_READ_ONLY_ENTRIES);
            s.push_str(TOOL_CATALOG_MUTATOR_ENTRIES);
            s
        }
        CatalogMode::ReadOnly => {
            let mut s = String::from("# Tool catalog for agentic investigation (read-only).\n");
            s.push_str(TOOL_CATALOG_READ_ONLY_ENTRIES);
            s
        }
    }
}

/// Run an agentic investigation with full tool access.
///
/// 1. Builds the agent preamble from the full tool catalog
/// 2. Attaches all investigation tools (including mutators) to the agent
/// 3. Runs the agent with the user's question
/// 4. Parses the structured output via rig Extractor
pub async fn exec_investigate(
    project_id: &str,
    project_path: &str,
    db_path: &str,
    question: &str,
    agent_provider: &str,
) -> Result<serde_json::Value, String> {
    use rig::client::CompletionClient;

    let ctx = InvestigationContext {
        project_id: project_id.to_string(),
        project_path: project_path.to_string(),
        db_path: db_path.to_string(),
    };

    // Standalone investigate always uses the full catalog + full tool set.
    let preamble = build_investigation_preamble(&ctx, CatalogMode::Full).await;

    let prompt = format!(
        "Investigate the following and report your findings:\n\nQuestion: {question}\n\n\
        Instructions:\n\
        1. Use the available tools to gather evidence. Call tools as needed — do not guess.\n\
        2. For each finding, cite which tool produced the evidence.\n\
        3. Be specific: include file paths, article slugs, counts, and code snippets where relevant.\n\
        4. If you find actionable issues, explain what should be fixed and whether it's auto-fixable or needs a developer.\n\
        5. Structure your findings with: title, description, evidence, fix_type, severity.\n\
        6. Limit yourself to at most 20 tool calls total.\n\
        7. Return your complete analysis in valid JSON format."
    );

    let backend = crate::rig::provider::resolve_backend(agent_provider, None, None, None).await
        .map_err(|e| format!("Provider error: {e}"))?;

    let response = match &backend {
        crate::rig::provider::LlmBackend::KimiBridge { base_url, model } => {
            let client = rig::providers::openai::Client::builder()
                .base_url(base_url)
                .api_key("dummy")
                .build()
                .map_err(|e| format!("Failed to build bridge client: {e}"))?;
            let full_prompt = format!("{preamble}\n\n{prompt}");
            let agent = client
                .completions_api()
                .agent(model)
                .tools(investigation_tools(ctx))
                .build();
            run_tool_agent(agent, &full_prompt).await?
        }
        crate::rig::provider::LlmBackend::Claude { api_key, model } => {
            let client = rig::providers::anthropic::Client::new(api_key)
                .map_err(|e| format!("Failed to build Claude client: {e}"))?;
            let agent = client
                .agent(model)
                .preamble(&preamble)
                .tools(investigation_tools(ctx))
                .build();
            run_tool_agent(agent, &prompt).await?
        }
        crate::rig::provider::LlmBackend::OpenAi { api_key, model } => {
            let client = rig::providers::openai::Client::new(api_key)
                .map_err(|e| format!("Failed to build OpenAI client: {e}"))?;
            let agent = client
                .agent(model)
                .preamble(&preamble)
                .tools(investigation_tools(ctx))
                .build();
            run_tool_agent(agent, &prompt).await?
        }
        crate::rig::provider::LlmBackend::Ollama { base_url, model } => {
            use rig::client::Nothing;
            let client = rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
                .map_err(|e| format!("Failed to build Ollama client: {e}"))?;
            let agent = client
                .agent(model)
                .preamble(&preamble)
                .tools(investigation_tools(ctx))
                .build();
            run_tool_agent(agent, &prompt).await?
        }
        _ => {
            return Err(format!(
                "Backend does not support tool calling. Use Kimi bridge, Claude, OpenAI, or Ollama."
            ));
        }
    };

    // Try to parse the response as JSON; if the agent returned prose, wrap it
    let parsed: serde_json::Value = serde_json::from_str(&response)
        .unwrap_or_else(|_| serde_json::json!({
            "answer": response,
            "findings": [],
            "summary": "Agent returned prose; no structured findings extracted."
        }));

    Ok(parsed)
}

/// Build the investigation preamble from the tool catalog.
///
/// Standalone investigate uses [`CatalogMode::Full`]. In-workflow callers (issue #80)
/// should use [`CatalogMode::ReadOnly`].
pub(crate) async fn build_investigation_preamble(
    ctx: &InvestigationContext,
    mode: CatalogMode,
) -> String {
    let catalog = tool_catalog_text(mode);

    // Gather quick project context
    let article_count = match ctx.open_db() {
        Ok(db) => {
            match crate::engine::task_store::list_articles(&db, &ctx.project_id) {
                Ok(articles) => articles.len(),
                Err(_) => 0,
            }
        }
        Err(_) => 0,
    };

    let mut preamble = format!(
        "You are an SEO investigation agent. You have access to the project's data \
        through tools. Your job is to investigate the user's question thoroughly \
        using the tools provided, then report specific, actionable findings.\n\n\
        Project context:\n\
        - Project ID: {}\n\
        - Project path: {}\n\
        - Articles: {} total\n\n\
        Available tools — what each does and when to use it:\n\n",
        ctx.project_id, ctx.project_path, article_count,
    );

    preamble.push_str(&catalog);
    preamble.push_str("\n\n---\n\n");

    // Output contract
    preamble.push_str(
        "Output format — return your findings as valid JSON:\n\
        {\n\
          \"answer\": \"Your natural language synthesis of all findings\",\n\
          \"summary\": \"1-2 sentence TL;DR\",\n\
          \"findings\": [\n\
            {\n\
              \"title\": \"Short issue title\",\n\
              \"description\": \"What the issue is and why it matters\",\n\
              \"evidence\": \"What tool data supports this finding\",\n\
              \"severity\": \"critical\" | \"warning\" | \"info\",\n\
              \"fix_type\": \"auto_fixable\" | \"developer_actionable\" | \"hybrid\" | \"informational\"\n\
            }\n\
          ]\n\
        }"
    );

    preamble
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_catalog_read_only_excludes_mutators() {
        let ro = tool_catalog_text(CatalogMode::ReadOnly);
        assert!(
            ro.contains("get_task_status"),
            "RO catalog must include get_task_status"
        );
        for mutator in [
            "create_task",
            "enqueue_task",
            "run_content_audit",
            "write_feature_spec",
        ] {
            // Match section headers only (tools.NAME)
            assert!(
                !ro.contains(&format!("[tools.{mutator}]")),
                "RO catalog must not contain mutator section [{mutator}]"
            );
        }
        assert!(!ro.contains("mutates = true"));
    }

    #[test]
    fn tool_catalog_full_includes_mutators_and_get_task_status() {
        let full = tool_catalog_text(CatalogMode::Full);
        assert!(full.contains("[tools.get_task_status]"));
        assert!(full.contains("[tools.create_task]"));
        assert!(full.contains("[tools.enqueue_task]"));
        assert!(full.contains("[tools.run_content_audit]"));
        assert!(full.contains("[tools.write_feature_spec]"));
        assert!(full.contains("mutates = true"));
    }
}
