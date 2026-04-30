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
    run_agent_with_backend(provider, prompt, project_path, None)
}

/// Run an agent with an optional backend preference for the Kimi bridge.
///
/// `backend_preference` should be `Some("direct")` for fast stateless queries
/// or `Some("acp")` for complex agentic tasks. When `None`, the global
/// `kimi_backend_mode` setting is used.
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
async fn run_rig_prompt(
    provider: &str,
    prompt: &str,
    backend_preference: Option<&str>,
) -> Result<String, RigError> {
    let backend = if provider == "kimi" && backend_preference.is_some() {
        // Caller explicitly wants bridge routing; bypass global mode setting.
        let bridge_url = std::env::var("KIMI_BRIDGE_URL")
            .unwrap_or_else(|_| "http://localhost:8080/v1".to_string());
        crate::rig::provider::LlmBackend::KimiBridge {
            base_url: bridge_url,
            model: crate::rig::provider::default_model_for_provider(provider),
        }
    } else {
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

        match crate::rig::provider::resolve_backend(provider, None, None, Some(&kimi_mode)).await {
            Ok(b) => b,
            Err(e) => return Err(RigError::Other(e)),
        }
    };

    match &backend {
        crate::rig::provider::LlmBackend::KimiDirect => {
            // Signal to the caller that we need CLI fallback.
            Err(RigError::FallbackToCli)
        }
        _ => {
            let response = crate::rig::provider::run_agent_with_backend(
                &backend, prompt, None, backend_preference,
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
