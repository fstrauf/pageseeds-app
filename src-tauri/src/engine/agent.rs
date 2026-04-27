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

/// Run an agent with the given prompt and return the captured stdout.
///
/// This function now delegates to `crate::rig::provider` when a rig-compatible
/// backend is available (bridge, Claude, OpenAI, Ollama). It falls back to the
/// direct CLI subprocess (`agent-wrapper`) only for `KimiDirect` or when the
/// bridge health check fails.
///
/// The function is still **synchronous** to maintain backward compatibility
/// with all existing step executors. Internally it uses `block_on` when an
/// async rig backend is selected.
pub fn run_agent(provider: &str, prompt: &str, project_path: &Path) -> Result<String, String> {
    // Attempt to use a rig backend first.
    match try_rig_backend(provider, prompt) {
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
        Err(result.error.unwrap_or_else(|| "Unknown agent error".to_string()))
    }
}

enum RigError {
    /// Rig could not handle this request — fall back to CLI.
    FallbackToCli,
    /// Rig failed with an error — propagate it.
    Other(String),
}

/// Try to run the prompt through a rig async backend.
fn try_rig_backend(provider: &str, prompt: &str) -> Result<String, RigError> {
    // We need an async runtime to resolve the backend and call rig.
    // First try the current runtime handle (works inside spawn_blocking too).
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            // No runtime available — create a temporary one.
            let rt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[agent] Failed to create tokio runtime: {}", e);
                    return Err(RigError::FallbackToCli);
                }
            };
            return rt.block_on(run_rig_prompt(provider, prompt));
        }
    };

    // We have a runtime handle — block_on the async work.
    handle.block_on(run_rig_prompt(provider, prompt))
}

/// Resolve backend and run prompt via rig.
async fn run_rig_prompt(provider: &str, prompt: &str) -> Result<String, RigError> {
    let backend = crate::rig::provider::resolve_backend(provider, None, None, None).await;

    match &backend {
        crate::rig::provider::LlmBackend::KimiDirect => {
            // Signal to the caller that we need CLI fallback.
            Err(RigError::FallbackToCli)
        }
        _ => {
            let response = crate::rig::provider::run_agent(&backend, prompt, None)
                .await
                .map_err(|e| RigError::Other(e))?;
            if let (Some(pt), Some(ct)) = (response.prompt_tokens, response.completion_tokens) {
                log::info!(
                    "[agent] tokens — prompt={}, completion={}, total={}",
                    pt, ct, pt + ct
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
        })
}
