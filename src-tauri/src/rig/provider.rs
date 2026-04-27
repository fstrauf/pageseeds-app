//! Provider abstraction — maps PageSeeds provider names to rig-core clients.
//!
//! Supported backends:
//! - `kimi`    → tries bridge (localhost:8080), falls back to direct CLI
//! - `claude`  → Anthropic API via rig native provider
//! - `openai`  → OpenAI API via rig native provider
//! - `ollama`  → local Ollama via rig native provider
//!
//! The bridge path uses `rig::providers::openai` with a custom base URL because
//! the kimi-acp-openai-bridge exposes an OpenAI-compatible `/v1/chat/completions`
//! endpoint. The direct CLI path keeps the existing `agent-wrapper` subprocess.

use std::time::Duration;

use rig::client::CompletionClient;
use rig::completion::Prompt;

/// How to reach a given LLM.
#[derive(Debug, Clone)]
pub enum LlmBackend {
    /// Kimi via the local ACP bridge (OpenAI-compatible endpoint).
    /// Uses `rig::providers::openai` with custom base URL.
    KimiBridge {
        base_url: String,
        model: String,
    },
    /// Kimi via direct CLI subprocess (legacy fallback).
    KimiDirect,
    /// Claude via Anthropic API (native rig provider).
    Claude {
        api_key: String,
        model: String,
    },
    /// OpenAI via native API.
    OpenAi {
        api_key: String,
        model: String,
    },
    /// Ollama via OpenAI-compatible endpoint.
    Ollama {
        base_url: String,
        model: String,
    },
}

/// Result of an agent run.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub content: String,
    /// Token usage when available (rig HTTP providers). `None` for direct CLI.
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

impl AgentResponse {
    pub fn from_content(content: String) -> Self {
        Self {
            content,
            prompt_tokens: None,
            completion_tokens: None,
        }
    }

    pub fn with_usage(content: String, prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            content,
            prompt_tokens: Some(prompt_tokens),
            completion_tokens: Some(completion_tokens),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Run a single prompt through the configured backend.
///
/// This is the primary replacement for `engine/agent.rs::run_agent`.
pub async fn run_agent(
    backend: &LlmBackend,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<AgentResponse, String> {
    match backend {
        LlmBackend::KimiBridge { base_url, model } => {
            run_kimi_bridge(base_url, model, prompt, preamble).await
        }
        LlmBackend::KimiDirect => run_kimi_direct(prompt, preamble),
        LlmBackend::Claude { api_key, model } => {
            run_claude(api_key, model, prompt, preamble).await
        }
        LlmBackend::OpenAi { api_key, model } => {
            run_openai(api_key, model, prompt, preamble).await
        }
        LlmBackend::Ollama { base_url, model } => {
            run_ollama(base_url, model, prompt, preamble).await
        }
    }
}

/// Detect whether the Kimi bridge is healthy on the given URL.
pub async fn check_bridge_health(base_url: &str) -> bool {
    let health_url = base_url.trim_end_matches("/v1").to_string() + "/health";
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(&health_url).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("kimi_available")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Resolve a provider string + settings into a concrete backend.
///
/// For `"kimi"` the behaviour depends on `kimi_backend_mode`:
/// - `"auto"`  → health check bridge, fall back to direct CLI if down
/// - `"bridge"`→ always use bridge (no health check)
/// - `"direct"`→ always use direct CLI
pub async fn resolve_backend(
    provider: &str,
    bridge_url: Option<&str>,
    _api_key: Option<&str>,
    kimi_backend_mode: Option<&str>,
) -> LlmBackend {
    let model = default_model_for_provider(provider);
    let mode = kimi_backend_mode.unwrap_or("auto");

    match provider {
        "kimi" => {
            let bridge_url = bridge_url.unwrap_or("http://localhost:8080/v1");
            match mode {
                "bridge" => LlmBackend::KimiBridge {
                    base_url: bridge_url.to_string(),
                    model,
                },
                "direct" => LlmBackend::KimiDirect,
                _ => {
                    // "auto" or any other value → health check
                    if check_bridge_health(bridge_url).await {
                        LlmBackend::KimiBridge {
                            base_url: bridge_url.to_string(),
                            model,
                        }
                    } else {
                        log::info!("[rig::provider] Bridge not available on {}, falling back to direct CLI", bridge_url);
                        LlmBackend::KimiDirect
                    }
                }
            }
        }
        "claude" => LlmBackend::Claude {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model,
        },
        "openai" => LlmBackend::OpenAi {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model,
        },
        "ollama" => LlmBackend::Ollama {
            base_url: bridge_url.unwrap_or("http://localhost:11434").to_string(),
            model,
        },
        other => {
            log::warn!("[rig::provider] Unknown provider '{}', falling back to Kimi direct", other);
            LlmBackend::KimiDirect
        }
    }
}

fn default_model_for_provider(provider: &str) -> String {
    match provider {
        "kimi" => "kimi-k2.5".to_string(),
        "claude" => "claude-sonnet-4-6".to_string(),
        "openai" => "gpt-4o".to_string(),
        "ollama" => "llama3.2".to_string(),
        _ => "kimi-k2.5".to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend implementations
// ─────────────────────────────────────────────────────────────────────────────

async fn run_kimi_bridge(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<AgentResponse, String> {
    let client = rig::providers::openai::Client::builder()
        .base_url(base_url)
        .api_key("dummy")
        .build()
        .map_err(|e| format!("Failed to build bridge client: {}", e))?
        .completions_api();

    let agent = client
        .agent(model)
        .preamble(preamble.unwrap_or("You are a helpful assistant."))
        .build();

    let resp = agent
        .prompt(prompt)
        .extended_details()
        .await
        .map_err(|e| format!("Bridge prompt failed: {}", e))?;

    Ok(AgentResponse::with_usage(
        resp.output,
        resp.usage.input_tokens,
        resp.usage.output_tokens,
    ))
}

fn run_kimi_direct(prompt: &str, _preamble: Option<&str>) -> Result<AgentResponse, String> {
    // The agent-wrapper crate does not support preamble separation;
    // it receives the full prompt as a single string.
    let result = agent_wrapper::run_agent("kimi", prompt, std::path::Path::new("."))
        .map_err(|e| format!("Agent wrapper error: {}", e))?;

    if result.success {
        Ok(AgentResponse::from_content(result.raw_output))
    } else {
        Err(result.error.unwrap_or_else(|| "Unknown agent error".to_string()))
    }
}

async fn run_claude(
    api_key: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<AgentResponse, String> {
    let client =
        rig::providers::anthropic::Client::new(api_key).map_err(|e| format!("Failed to build Claude client: {}", e))?;

    let agent = client
        .agent(model)
        .preamble(preamble.unwrap_or("You are a helpful assistant."))
        .build();

    let resp = agent
        .prompt(prompt)
        .extended_details()
        .await
        .map_err(|e| format!("Claude prompt failed: {}", e))?;

    Ok(AgentResponse::with_usage(
        resp.output,
        resp.usage.input_tokens,
        resp.usage.output_tokens,
    ))
}

async fn run_openai(
    api_key: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<AgentResponse, String> {
    let client = rig::providers::openai::Client::new(api_key)
        .map_err(|e| format!("Failed to build OpenAI client: {}", e))?;

    let agent = client
        .agent(model)
        .preamble(preamble.unwrap_or("You are a helpful assistant."))
        .build();

    let resp = agent
        .prompt(prompt)
        .extended_details()
        .await
        .map_err(|e| format!("OpenAI prompt failed: {}", e))?;

    Ok(AgentResponse::with_usage(
        resp.output,
        resp.usage.input_tokens,
        resp.usage.output_tokens,
    ))
}

async fn run_ollama(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<AgentResponse, String> {
    use rig::client::Nothing;

    let client = rig::providers::ollama::Client::builder()
        .api_key(Nothing)
        .base_url(base_url)
        .build()
        .map_err(|e| format!("Failed to build Ollama client: {}", e))?;

    let agent = client
        .agent(model)
        .preamble(preamble.unwrap_or("You are a helpful assistant."))
        .build();

    let resp = agent
        .prompt(prompt)
        .extended_details()
        .await
        .map_err(|e| format!("Ollama prompt failed: {}", e))?;

    Ok(AgentResponse::with_usage(
        resp.output,
        resp.usage.input_tokens,
        resp.usage.output_tokens,
    ))
}
