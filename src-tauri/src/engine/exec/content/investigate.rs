//! Content review investigate step — tool-calling judgment when the backend
//! supports tools; falls back to scripted `content_review_recommend` otherwise.
//!
//! Uses [`InvestigationAccess::ReadOnly`] only (no create_task / enqueue / audit
//! mutators). Does **not** write `recommendations.json`, so
//! `create_fix_content_article_tasks` no-ops safely until proposed tasks are
//! wired by a later issue.

use crate::engine::exec::investigate::{
    backend_supports_tool_calling, build_investigation_preamble, run_tool_equipped_agent,
};
use crate::engine::project_paths::ProjectPaths;
use crate::engine::tools::{investigation_kit, InvestigationAccess, InvestigationContext};
use crate::models::content_review::InvestigationFindings;
use crate::models::task::Task;

/// Step runner for `content_review_investigate`.
///
/// 1. Resolve LLM backend
/// 2. If backend lacks tool calling → fall back to `exec_content_review_recommend`
/// 3. Otherwise run a read-only multi-turn tool agent, then typed Extractor
///    for [`InvestigationFindings`] (no prose-JSON fallback on this path)
pub(crate) async fn exec_content_review_investigate(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    let backend = match crate::rig::provider::resolve_backend(agent_provider, None, None, None).await
    {
        Ok(b) => b,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!(
                "Provider resolution failed: {e}"
            ));
        }
    };

    if !backend_supports_tool_calling(&backend) {
        log::info!(
            "[content_review_investigate] backend does not support tool calling; \
             falling back to content_review_recommend"
        );
        return super::exec_content_review_recommend(task, project_path, agent_provider).await;
    }

    let ctx = InvestigationContext {
        project_id: task.project_id.clone(),
        project_path: project_path.to_string(),
        db_path: crate::db::default_db_path().to_string_lossy().to_string(),
    };

    let kit = investigation_kit(ctx.clone(), InvestigationAccess::ReadOnly);
    let base_preamble = build_investigation_preamble(&ctx, &kit.catalog).await;
    let preamble = build_content_review_investigation_preamble(&base_preamble);

    let prompt = build_content_review_investigation_prompt(task);

    log::info!(
        "[content_review_investigate] running RO tool agent (project={})",
        task.project_id
    );

    let agent_response = match run_tool_equipped_agent(&backend, kit.tools, &preamble, &prompt).await
    {
        Ok(text) => text,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!(
                "Content review investigation agent failed: {e}"
            ));
        }
    };

    // Typed extraction only — no prose unwrap_or fallback on this path.
    let extract_prompt = format!(
        "Map the following content-review investigation analysis into the \
         InvestigationFindings schema. Use only evidence present in the analysis; \
         do not invent findings.\n\n\
         Analysis:\n{agent_response}"
    );
    let extract_preamble = "You extract structured InvestigationFindings from investigation \
        analysis. Always use the submit tool. severity must be one of: critical, warning, info. \
        fix_type must be one of: auto_fixable, developer_actionable, hybrid, informational. \
        proposed_tasks.params must be a JSON object (use {} when empty).";

    let findings = match crate::rig::extraction::extract_with_backend::<InvestigationFindings>(
        &backend,
        &extract_prompt,
        Some(extract_preamble),
        Some("direct"),
        None,
    )
    .await
    {
        Ok(f) => f,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!(
                "Failed to extract InvestigationFindings (typed only; no prose fallback): {e}"
            ));
        }
    };

    let findings_str = match serde_json::to_string_pretty(&findings) {
        Ok(s) => s + "\n",
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!(
                "Failed to serialize InvestigationFindings: {e}"
            ));
        }
    };

    // Optional automation-dir snapshot; primary contract is step output → artifact.
    // Intentionally does NOT write recommendations.json.
    let paths = ProjectPaths::from_path(project_path);
    let findings_path = paths.automation_dir.join("investigation_findings.json");
    if let Err(e) = std::fs::create_dir_all(&paths.automation_dir) {
        log::warn!(
            "[content_review_investigate] could not create automation dir: {e}"
        );
    } else if let Err(e) = std::fs::write(&findings_path, &findings_str) {
        log::warn!(
            "[content_review_investigate] failed to write investigation_findings.json: {e}"
        );
    } else {
        log::info!(
            "[content_review_investigate] wrote investigation_findings.json \
             ({} findings, {} proposed_tasks)",
            findings.findings.len(),
            findings.proposed_tasks.len()
        );
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Investigation complete: {} findings, {} proposed tasks",
            findings.findings.len(),
            findings.proposed_tasks.len()
        ),
        output: Some(findings_str),
    }
}

fn build_content_review_investigation_preamble(base_preamble: &str) -> String {
    format!(
        "{base_preamble}\n\n\
        ---\n\n\
        Content review framing:\n\
        You are investigating why this project's content underperforms in search \
        and on-site quality. Focus on root causes, not per-article copy rewrites.\n\
        Consider: indexing/coverage gaps, CTR (titles/meta/snippets), cannibalization \
        and near-duplicates, internal linking, structural title/template bugs, \
        audit health failures, and freshness.\n\
        Read-only tools only — you cannot create tasks, enqueue work, or run mutators. \
        Propose follow-up task types in proposed_tasks; do not try to execute them.\n\
        Limit yourself to at most 20 tool calls total."
    )
}

fn build_content_review_investigation_prompt(task: &Task) -> String {
    let title = task.title.as_deref().unwrap_or("Content review");
    let description = task.description.as_deref().unwrap_or("");
    format!(
        "Run a content performance investigation for this project.\n\n\
         Task: {title}\n\
         {description}\n\n\
         Instructions:\n\
         1. Use the available tools to gather evidence. Call tools as needed — do not guess.\n\
         2. For each finding, cite which tool produced the evidence.\n\
         3. Be specific: include file paths, article slugs, counts, and snippets where relevant.\n\
         4. For actionable issues, set fix_type to auto_fixable, developer_actionable, \
            hybrid, or informational.\n\
         5. Propose downstream task types (e.g. ctr_audit, cannibalization_audit, \
            indexing_health_campaign, content_cleanup, fix_content_article) when justified, \
            with a clear reason and params object.\n\
         6. Limit yourself to at most 20 tool calls total.\n\
         7. End with a clear summary of root causes and prioritised findings."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::exec::investigate::backend_supports_tool_calling;
    use crate::rig::provider::LlmBackend;

    #[test]
    fn tool_calling_gate_matches_supported_backends() {
        assert!(backend_supports_tool_calling(&LlmBackend::KimiBridge {
            base_url: "http://localhost".into(),
            model: "kimi".into(),
        }));
        assert!(backend_supports_tool_calling(&LlmBackend::Claude {
            api_key: "k".into(),
            model: "claude".into(),
        }));
        assert!(backend_supports_tool_calling(&LlmBackend::OpenAi {
            api_key: "k".into(),
            model: "gpt".into(),
        }));
        assert!(backend_supports_tool_calling(&LlmBackend::Ollama {
            base_url: "http://localhost".into(),
            model: "llama".into(),
        }));
        assert!(!backend_supports_tool_calling(&LlmBackend::KimiCli {
            work_dir: ".".into(),
        }));
        assert!(!backend_supports_tool_calling(&LlmBackend::KimiDirect));
    }

    #[test]
    fn content_review_preamble_mentions_ro_and_domains() {
        let p = build_content_review_investigation_preamble("base catalog");
        assert!(p.contains("Read-only"));
        assert!(p.contains("cannibalization"));
        assert!(p.contains("20 tool calls"));
        assert!(p.contains("base catalog"));
    }
}
