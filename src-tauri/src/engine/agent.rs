/// Agent invocation — detect available agent CLIs and run non-interactive prompts.
///
/// This module is a compatibility layer. The actual LLM execution now lives in
/// `crate::rig::provider`. This file maintains the original `run_agent` sync
/// interface so that existing step executors do not need to change.
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub use agent_wrapper::detect_agents_cached as detect_agents;

/// Information about an available agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub binary: String,
    pub available: bool,
    pub version: Option<String>,
}

/// Agent status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub available_agents: Vec<AgentInfo>,
    pub configured_provider: String,
    /// Token usage from the most recent agent run, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

/// Token usage for a single agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Check which agent CLIs are available on PATH.
pub fn detect_agents_sync(configured_provider: &str) -> AgentStatus {
    let agents = agent_wrapper::detect_agents_cached(false);

    let available_agents = agents
        .into_iter()
        .map(|a| AgentInfo {
            name: a.name,
            binary: a.binary,
            available: a.available,
            version: a.version,
        })
        .collect();

    AgentStatus {
        available_agents,
        configured_provider: configured_provider.to_string(),
        token_usage: None,
    }
}

// ─── Token usage side-channel ────────────────────────────────────────────────

static LAST_TOKENS: Mutex<(Option<u64>, Option<u64>)> = Mutex::new((None, None));

/// Retrieve and clear the token usage from the most recent rig-backed agent run.
///
/// Returns `(prompt_tokens, completion_tokens)`. Call this after `run_agent`
/// to capture usage for persistence.
pub fn take_last_tokens() -> (Option<u64>, Option<u64>) {
    let mut guard = LAST_TOKENS.lock().unwrap();
    let result = *guard;
    *guard = (None, None);
    result
}

fn set_last_tokens(prompt: Option<u64>, completion: Option<u64>) {
    let mut guard = LAST_TOKENS.lock().unwrap();
    *guard = (prompt, completion);
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Run an agent prompt through the standard agent pipeline.
///
/// This is the single entry point for all agent calls in the codebase.
/// Bypass callers should use this (or `run_agent_with_skill`) instead of
/// constructing prompts and calling `run_agent` directly with inline prompts.
///
/// **Backend routing:** This function defaults to `direct` mode for the Kimi
/// bridge, which is appropriate for stateless analysis and short generation
/// tasks. Content-writing tasks that need ACP (file I/O, persistent session)
/// should call `run_agent_with_backend(..., Some("acp"))` explicitly.
pub fn run_agent(provider: &str, prompt: &str, project_path: &Path) -> Result<String, String> {
    run_agent_with_backend(provider, prompt, project_path, Some("direct"))
}

/// Load a skill, assemble a prompt, and run it through the agent pipeline.
///
/// Standardizes the pattern repeated across 13 exec modules:
/// 1. Load skill from project repo or app defaults
/// 2. Build prompt from skill content + context + output contract
/// 3. Call the agent
/// 4. Return raw output (caller handles JSON extraction and domain logic)
///
/// `output_contract` is appended as a "## Output Contract" section instructing
/// the agent to return JSON matching a specific schema.
pub fn run_agent_with_skill(
    skill_name: &str,
    repo_root: &Path,
    context: &str,
    agent_provider: &str,
    output_contract: &str,
) -> Result<String, String> {
    let skill = crate::engine::skills::load_skill(repo_root, skill_name).ok_or_else(|| {
        format!(
            "Skill '{}' not found in .github/skills/ or app defaults",
            skill_name
        )
    })?;

    let prompt = format!(
        "{}\n\n---\n\n## Context\n\n{}\n\n## Output Contract\n\n{}\n\n\
         CRITICAL: Return ONLY a single JSON object matching the Output Contract above. \
         Do not include markdown prose, summaries, tables, or explanations outside the JSON. \
         Do not write files. Output the JSON directly in your response.",
        skill.content,
        context,
        output_contract,
    );

    run_agent(agent_provider, &prompt, repo_root)
}

/// Run an agent with an optional backend preference for the Kimi bridge.
///
/// `backend_preference` should be `Some("direct")` for fast stateless queries
/// or `Some("acp")` for complex agentic tasks. It is passed as the
/// `X-Kimi-Backend` header when the resolved backend is `KimiBridge`.
///
/// The global `kimi_backend_mode` setting always controls whether the bridge
/// or direct CLI is used; `backend_preference` does **not** override it.
pub fn run_agent_with_backend(
    provider: &str,
    prompt: &str,
    project_path: &Path,
    backend_preference: Option<&str>,
) -> Result<String, String> {
    // Attempt to use a rig backend first.
    match try_rig_backend_with_preference(provider, prompt, backend_preference) {
        Ok(content) => return Ok(content),
        Err(RigError::FallbackToCli) => {
            // Fall through to direct CLI below.
        }
        Err(RigError::Other(msg)) => return Err(msg),
    }

    // Fallback: direct CLI subprocess.
    let result = agent_wrapper::run_agent(provider, prompt, project_path)
        .map_err(|e| format!("Agent wrapper error: {}", e))?;

    if result.success {
        Ok(result.raw_output)
    } else {
        Err(result
            .error
            .unwrap_or_else(|| "Unknown agent error".to_string()))
    }
}

enum RigError {
    /// Rig could not handle this request — fall back to CLI.
    FallbackToCli,
    /// Rig failed with an error — propagate it.
    Other(String),
}

fn try_rig_backend_with_preference(
    provider: &str,
    prompt: &str,
    backend_preference: Option<&str>,
) -> Result<String, RigError> {
    // Spawn a dedicated thread with its own runtime to avoid all block_on issues:
    // - called from an async task on a worker thread
    // - called from a spawn_blocking thread (block_in_place panics there)
    // - called from a current_thread runtime
    let provider = provider.to_string();
    let prompt = prompt.to_string();
    let backend_preference = backend_preference.map(|s| s.to_string());

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[agent] Failed to create tokio runtime: {}", e);
                return Err(RigError::FallbackToCli);
            }
        };
        let pref = backend_preference.as_deref();
        rt.block_on(run_rig_prompt(&provider, &prompt, pref))
    })
    .join()
    .unwrap_or_else(|e| {
        log::error!("[agent] Rig thread panicked: {:?}", e);
        Err(RigError::Other("Agent thread panicked".to_string()))
    })
}

/// Resolve backend and run prompt via rig.
///
/// `backend_preference` is treated as a *routing hint* (passed as the
/// `X-Kimi-Backend` header) rather than an override of the global
/// `kimi_backend_mode` setting. This ensures `"auto"` mode can still
/// fall back to the direct CLI when the bridge is down.
async fn run_rig_prompt(
    provider: &str,
    prompt: &str,
    backend_preference: Option<&str>,
) -> Result<String, RigError> {
    // Read kimi_backend_mode from global settings (fallback to "auto" if DB unreachable).
    let kimi_mode = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => crate::db::global_settings::get_kimi_backend_mode(&conn),
        Err(e) => {
            log::warn!(
                "[agent] Failed to open DB for kimi_backend_mode: {}. Using auto.",
                e
            );
            "auto".to_string()
        }
    };

    let backend =
        match crate::rig::provider::resolve_backend(provider, None, None, Some(&kimi_mode)).await {
            Ok(b) => b,
            Err(e) => return Err(RigError::Other(e)),
        };

    match &backend {
        crate::rig::provider::LlmBackend::KimiDirect => {
            // Signal to the caller that we need CLI fallback.
            Err(RigError::FallbackToCli)
        }
        crate::rig::provider::LlmBackend::KimiBridge { .. } => {
            // Pass backend_preference as the X-Kimi-Backend header.
            let response = crate::rig::provider::run_agent_with_backend(
                &backend,
                prompt,
                None,
                backend_preference,
            )
            .await
            .map_err(|e| RigError::Other(e))?;
            if let (Some(pt), Some(ct)) = (response.prompt_tokens, response.completion_tokens) {
                log::info!(
                    "[agent] tokens — prompt={}, completion={}, total={}",
                    pt,
                    ct,
                    pt + ct
                );
                set_last_tokens(Some(pt), Some(ct));
            }
            Ok(response.content)
        }
        _ => {
            // Non-Kimi providers — backend_preference is irrelevant.
            let response =
                crate::rig::provider::run_agent_with_backend(&backend, prompt, None, None)
                    .await
                    .map_err(|e| RigError::Other(e))?;
            if let (Some(pt), Some(ct)) = (response.prompt_tokens, response.completion_tokens) {
                log::info!(
                    "[agent] tokens — prompt={}, completion={}, total={}",
                    pt,
                    ct,
                    pt + ct
                );
                set_last_tokens(Some(pt), Some(ct));
            }
            Ok(response.content)
        }
    }
}

/// Async version of agent detection (uses cached results).
pub async fn detect_agents_async(configured_provider: &str) -> AgentStatus {
    // Run detection in blocking thread since agent-wrapper is sync
    let provider = configured_provider.to_string();
    tokio::task::spawn_blocking(move || detect_agents_sync(&provider))
        .await
        .unwrap_or_else(|_| AgentStatus {
            available_agents: vec![],
            configured_provider: configured_provider.to_string(),
            token_usage: None,
        })
}
