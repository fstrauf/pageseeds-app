//! Agentic investigation executor.
//!
//! Builds a rig agent with project data tools attached and runs an open-ended
//! investigation. The agent has access to GSC, articles, audit data, indexing
//! status, link graph, framework files, and more. It calls tools freely to
//! answer the user's question.
//!
//! Access mode (tools + catalog) is owned by
//! [`crate::engine::tools::InvestigationAccess`] via [`investigation_kit`].
//! Standalone investigate always uses [`InvestigationAccess::Full`].

use crate::engine::tools::{
    investigation_kit, InvestigationAccess, InvestigationContext,
};
use crate::rig::provider::LlmBackend;
use rig::completion::Prompt;

/// Run a prompt on a tool-equipped agent and return the response string.
async fn run_tool_agent<A: Prompt + Send>(agent: A, prompt: &str) -> Result<String, String> {
    agent.prompt(prompt).await.map_err(|e| format!("Agent error: {e}"))
}

/// Whether this backend can run multi-turn tool calling for investigation.
///
/// Tool-capable: KimiBridge, Claude, OpenAi, Ollama.
/// Not supported: KimiCli (print mode has no tool_calls), KimiDirect, others.
pub(crate) fn backend_supports_tool_calling(backend: &LlmBackend) -> bool {
    matches!(
        backend,
        LlmBackend::KimiBridge { .. }
            | LlmBackend::Claude { .. }
            | LlmBackend::OpenAi { .. }
            | LlmBackend::Ollama { .. }
    )
}

/// Build a tool-equipped agent for `backend` and run `prompt` (with `preamble`).
///
/// Shared by standalone investigate and in-workflow content_review investigate.
/// Callers must gate with [`backend_supports_tool_calling`] first.
pub(crate) async fn run_tool_equipped_agent(
    backend: &LlmBackend,
    tools: Vec<Box<dyn rig::tool::ToolDyn>>,
    preamble: &str,
    prompt: &str,
) -> Result<String, String> {
    use rig::client::CompletionClient;

    match backend {
        LlmBackend::KimiBridge { base_url, model } => {
            let client = rig::providers::openai::Client::builder()
                .base_url(base_url)
                .api_key("dummy")
                .build()
                .map_err(|e| format!("Failed to build bridge client: {e}"))?;
            // Bridge completions API has no separate preamble slot — fold in.
            let full_prompt = format!("{preamble}\n\n{prompt}");
            let agent = client
                .completions_api()
                .agent(model)
                .tools(tools)
                .build();
            run_tool_agent(agent, &full_prompt).await
        }
        LlmBackend::Claude { api_key, model } => {
            let client = rig::providers::anthropic::Client::new(api_key)
                .map_err(|e| format!("Failed to build Claude client: {e}"))?;
            let agent = client
                .agent(model)
                .preamble(preamble)
                .tools(tools)
                .build();
            run_tool_agent(agent, prompt).await
        }
        LlmBackend::OpenAi { api_key, model } => {
            let client = rig::providers::openai::Client::new(api_key)
                .map_err(|e| format!("Failed to build OpenAI client: {e}"))?;
            let agent = client
                .agent(model)
                .preamble(preamble)
                .tools(tools)
                .build();
            run_tool_agent(agent, prompt).await
        }
        LlmBackend::Ollama { base_url, model } => {
            use rig::client::Nothing;
            let client = rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
                .map_err(|e| format!("Failed to build Ollama client: {e}"))?;
            let agent = client
                .agent(model)
                .preamble(preamble)
                .tools(tools)
                .build();
            run_tool_agent(agent, prompt).await
        }
        _ => Err(
            "Backend does not support tool calling. Use Kimi bridge, Claude, OpenAI, or Ollama."
                .to_string(),
        ),
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
    let ctx = InvestigationContext {
        project_id: project_id.to_string(),
        project_path: project_path.to_string(),
        db_path: db_path.to_string(),
    };

    // Standalone investigate always uses Full — tools and catalog from one kit.
    let kit = investigation_kit(ctx.clone(), InvestigationAccess::Full);
    let preamble = build_investigation_preamble(&ctx, &kit.catalog).await;
    let tools = kit.tools;

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

    let response = run_tool_equipped_agent(&backend, tools, &preamble, &prompt).await?;

    // Try to parse the response as JSON; if the agent returned prose, wrap it
    let parsed: serde_json::Value = serde_json::from_str(&response)
        .unwrap_or_else(|_| serde_json::json!({
            "answer": response,
            "findings": [],
            "summary": "Agent returned prose; no structured findings extracted."
        }));

    Ok(parsed)
}

/// Build the investigation preamble from catalog text (from [`investigation_kit`]).
///
/// Standalone investigate uses the kit with [`InvestigationAccess::Full`].
/// In-workflow callers (issue #80) should use [`InvestigationAccess::ReadOnly`].
pub(crate) async fn build_investigation_preamble(
    ctx: &InvestigationContext,
    catalog: &str,
) -> String {
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

    preamble.push_str(catalog);
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
    use crate::engine::tools::investigation_catalog;

    #[test]
    fn tool_catalog_read_only_excludes_mutators() {
        let ro = investigation_catalog(InvestigationAccess::ReadOnly);
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
        let full = investigation_catalog(InvestigationAccess::Full);
        assert!(full.contains("[tools.get_task_status]"));
        assert!(full.contains("[tools.create_task]"));
        assert!(full.contains("[tools.enqueue_task]"));
        assert!(full.contains("[tools.run_content_audit]"));
        assert!(full.contains("[tools.write_feature_spec]"));
        assert!(full.contains("mutates = true"));
    }
}
