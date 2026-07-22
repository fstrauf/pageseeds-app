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
//!
//! Tool-agent construction is centralized in
//! [`crate::rig::provider::run_tool_equipped_agent`].

use crate::engine::tools::{
    investigation_kit, InvestigationAccess, InvestigationContext,
};
use crate::models::content_review::StandaloneInvestigationResult;
use crate::rig::provider::run_tool_equipped_agent;

/// Run an agentic investigation with full tool access.
///
/// 1. Builds the agent preamble from the full tool catalog (+ standalone JSON contract)
/// 2. Attaches all investigation tools (including mutators) to the agent
/// 3. Runs the agent with the user's question
/// 4. Resolves structured output:
///    a. First-pass typed JSON parse of the agent reply (`StandaloneInvestigationResult`)
///    b. If that fails, `extract_with_backend::<StandaloneInvestigationResult>` on the prose
///    c. Soft-fallback: prose as `answer`, empty summary/findings (never hard-fails extraction)
pub async fn exec_investigate(
    project_id: &str,
    project_path: &str,
    db_path: &str,
    question: &str,
    agent_provider: &str,
) -> Result<StandaloneInvestigationResult, String> {
    let ctx = InvestigationContext {
        project_id: project_id.to_string(),
        project_path: project_path.to_string(),
        db_path: db_path.to_string(),
    };

    // Standalone investigate always uses Full — tools and catalog from one kit.
    let kit = investigation_kit(ctx.clone(), InvestigationAccess::Full);
    // Core context + catalog, plus standalone freeform JSON output contract.
    let preamble = format!(
        "{}\n\n---\n\n{}",
        build_investigation_preamble(&ctx, &kit.catalog).await,
        standalone_investigation_output_contract()
    );
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

    let response = run_tool_equipped_agent(&backend, tools, &preamble, &prompt)
        .await
        .map_err(|e| e.to_string())?;

    Ok(resolve_standalone_investigation_result(&backend, &response).await)
}

/// Resolve agent text into a typed standalone result.
///
/// Order: direct JSON parse → typed Extractor → prose soft-fallback.
/// Soft-fallback preserves prior command behavior when extraction is unavailable
/// (e.g. KimiDirect) or the model returns pure prose.
async fn resolve_standalone_investigation_result(
    backend: &crate::rig::provider::LlmBackend,
    response: &str,
) -> StandaloneInvestigationResult {
    // a. First-pass: agent was asked for JSON via standalone_investigation_output_contract.
    if let Ok(parsed) = serde_json::from_str::<StandaloneInvestigationResult>(response) {
        return parsed;
    }

    // b. Typed extraction from prose analysis (same helper as content-review path).
    let extract_prompt = format!(
        "Map the following investigation analysis into the StandaloneInvestigationResult \
         schema. Use only evidence present in the analysis; do not invent findings.\n\n\
         Analysis:\n{response}"
    );
    let extract_preamble = "You extract structured StandaloneInvestigationResult from \
        investigation analysis. Always use the submit tool. \
        answer is the natural-language synthesis; summary is a 1-2 sentence TL;DR. \
        severity must be one of: critical, warning, info. \
        fix_type must be one of: auto_fixable, developer_actionable, hybrid, informational.";

    match crate::rig::extraction::extract_with_backend::<StandaloneInvestigationResult>(
        backend,
        &extract_prompt,
        Some(extract_preamble),
        Some("direct"),
        None,
    )
    .await
    {
        Ok(typed) => typed,
        // c. Soft-fallback: pure prose as answer (do not hard-fail the investigate command).
        Err(e) => {
            log::warn!(
                "[investigate] typed extraction failed ({e}); soft-falling back to prose answer"
            );
            StandaloneInvestigationResult::from_prose(response)
        }
    }
}

/// Build the shared investigation core preamble: project context + tool catalog.
///
/// Does **not** include an output contract. Callers that need freeform JSON
/// (standalone [`exec_investigate`]) must append
/// [`standalone_investigation_output_contract`]. Content-review investigate
/// layers its own framing and uses a typed Extractor later instead.
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
    preamble
}

/// Freeform JSON output contract for standalone [`exec_investigate`] only.
///
/// Encourages first-pass structured JSON from the tool agent. Content-review
/// investigate does not use this contract; it extracts typed
/// `InvestigationFindings` after a prose analysis turn. Standalone still
/// soft-falls back to prose if parse + Extractor both fail.
pub(crate) fn standalone_investigation_output_contract() -> &'static str {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tools::investigation_catalog;
    use crate::models::content_review::{Finding, StandaloneInvestigationResult};

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

    #[test]
    fn core_preamble_excludes_standalone_json_contract() {
        let catalog = investigation_catalog(InvestigationAccess::ReadOnly);
        // build_investigation_preamble is async and may open DB; assert the
        // contract helper is separate and the core path must not embed it.
        let contract = standalone_investigation_output_contract();
        assert!(contract.contains("Output format — return your findings as valid JSON"));
        assert!(contract.contains("\"answer\""));
        // Core preamble is only context + catalog; contract is appended by
        // exec_investigate only.
        assert!(!catalog.contains("Output format — return your findings as valid JSON"));
    }

    #[test]
    fn first_pass_json_parses_as_standalone_result() {
        let json = r#"{
            "answer": "Traffic is down on /blog/foo.",
            "summary": "CTR drop on one page.",
            "findings": [{
                "title": "Low CTR",
                "description": "Title tag mismatch",
                "evidence": "get_gsc_page_metrics",
                "severity": "warning",
                "fix_type": "auto_fixable"
            }]
        }"#;
        let parsed: StandaloneInvestigationResult =
            serde_json::from_str(json).expect("valid StandaloneInvestigationResult JSON");
        assert_eq!(parsed.answer, "Traffic is down on /blog/foo.");
        assert_eq!(parsed.summary, "CTR drop on one page.");
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].severity, "warning");
    }

    #[test]
    fn soft_fallback_from_prose_preserves_answer() {
        let prose = "I looked at GSC and found impressions flatlined after March.";
        let result = StandaloneInvestigationResult::from_prose(prose);
        assert_eq!(result.answer, prose);
        assert!(result.summary.is_empty());
        assert!(result.findings.is_empty());
    }

    #[test]
    fn soft_fallback_serializes_for_command_evidence() {
        let result = StandaloneInvestigationResult {
            answer: "full writeup".into(),
            summary: "tldr".into(),
            findings: vec![Finding {
                title: "Issue".into(),
                description: "Desc".into(),
                evidence: "tool X".into(),
                severity: "info".into(),
                fix_type: "informational".into(),
            }],
        };
        let value = serde_json::to_value(&result).expect("serialize");
        assert_eq!(value["answer"], "full writeup");
        assert_eq!(value["summary"], "tldr");
        assert_eq!(value["findings"][0]["title"], "Issue");
    }
}
