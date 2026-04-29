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
    KimiBridge { base_url: String, model: String },
    /// Kimi via direct CLI subprocess (legacy fallback).
    KimiDirect,
    /// Claude via Anthropic API (native rig provider).
    Claude { api_key: String, model: String },
    /// OpenAI via native API.
    OpenAi { api_key: String, model: String },
    /// Ollama via OpenAI-compatible endpoint.
    Ollama { base_url: String, model: String },
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
        LlmBackend::Claude { api_key, model } => run_claude(api_key, model, prompt, preamble).await,
        LlmBackend::OpenAi { api_key, model } => run_openai(api_key, model, prompt, preamble).await,
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
) -> Result<LlmBackend, String> {
    let model = default_model_for_provider(provider);
    let mode = kimi_backend_mode.unwrap_or("auto");

    match provider {
        "kimi" => {
            let bridge_url = bridge_url
                .map(|s| s.to_string())
                .or_else(|| std::env::var("KIMI_BRIDGE_URL").ok())
                .unwrap_or_else(|| "http://localhost:8080/v1".to_string());
            let bridge_url = bridge_url.as_str();
            match mode {
                "bridge" => Ok(LlmBackend::KimiBridge {
                    base_url: bridge_url.to_string(),
                    model,
                }),
                "direct" => Ok(LlmBackend::KimiDirect),
                _ => {
                    // "auto" or any other value → health check
                    if check_bridge_health(bridge_url).await {
                        Ok(LlmBackend::KimiBridge {
                            base_url: bridge_url.to_string(),
                            model,
                        })
                    } else {
                        Err(format!(
                            "Kimi bridge not available on {}. Start the bridge or set kimi_backend_mode to 'direct'.",
                            bridge_url
                        ))
                    }
                }
            }
        }
        "claude" => Ok(LlmBackend::Claude {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model,
        }),
        "openai" => Ok(LlmBackend::OpenAi {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model,
        }),
        "ollama" => Ok(LlmBackend::Ollama {
            base_url: bridge_url.unwrap_or("http://localhost:11434").to_string(),
            model,
        }),
        other => Err(format!(
            "Unknown provider '{}'. Valid providers: kimi, claude, openai, ollama",
            other
        )),
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
    let result = crate::rig::compat::kimi::run_prompt(base_url, model, prompt, preamble)
        .await
        .map_err(|e| format!("Kimi bridge prompt failed: {}", e))?;

    Ok(AgentResponse::with_usage(
        result.content,
        result.prompt_tokens.unwrap_or(0),
        result.completion_tokens.unwrap_or(0),
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
        Err(result
            .error
            .unwrap_or_else(|| "Unknown agent error".to_string()))
    }
}

async fn run_claude(
    api_key: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<AgentResponse, String> {
    let client = rig::providers::anthropic::Client::new(api_key)
        .map_err(|e| format!("Failed to build Claude client: {}", e))?;

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

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_default_model_for_provider() {
        assert_eq!(default_model_for_provider("kimi"), "kimi-k2.5");
        assert_eq!(default_model_for_provider("claude"), "claude-sonnet-4-6");
        assert_eq!(default_model_for_provider("openai"), "gpt-4o");
        assert_eq!(default_model_for_provider("ollama"), "llama3.2");
        assert_eq!(default_model_for_provider("unknown"), "kimi-k2.5");
    }

    #[tokio::test]
    async fn test_resolve_backend_known_providers() {
        // Claude
        let backend = resolve_backend("claude", None, None, None).await.unwrap();
        assert!(
            matches!(backend, LlmBackend::Claude { model, .. } if model == "claude-sonnet-4-6")
        );

        // OpenAI
        let backend = resolve_backend("openai", None, None, None).await.unwrap();
        assert!(matches!(backend, LlmBackend::OpenAi { model, .. } if model == "gpt-4o"));

        // Ollama
        let backend = resolve_backend("ollama", None, None, None).await.unwrap();
        assert!(matches!(backend, LlmBackend::Ollama { model, .. } if model == "llama3.2"));
    }

    #[tokio::test]
    async fn test_resolve_backend_unknown_provider_errors() {
        let err = resolve_backend("invalid_provider", None, None, None)
            .await
            .unwrap_err();
        assert!(err.contains("Unknown provider 'invalid_provider'"));
        assert!(err.contains("kimi"));
        assert!(err.contains("claude"));
        assert!(err.contains("openai"));
        assert!(err.contains("ollama"));
    }

    #[tokio::test]
    async fn test_resolve_backend_kimi_bridge_url_env_var() {
        // Set KIMI_BRIDGE_URL to a fake URL. With mode "bridge", it should use it directly.
        let old = std::env::var("KIMI_BRIDGE_URL").ok();
        std::env::set_var("KIMI_BRIDGE_URL", "http://fake-bridge:9999/v1");
        let backend = resolve_backend("kimi", None, None, Some("bridge"))
            .await
            .unwrap();
        assert!(
            matches!(backend, LlmBackend::KimiBridge { base_url, .. } if base_url == "http://fake-bridge:9999/v1")
        );
        if let Some(v) = old {
            std::env::set_var("KIMI_BRIDGE_URL", v);
        } else {
            std::env::remove_var("KIMI_BRIDGE_URL");
        }
    }

    #[test]
    fn test_agent_response_from_content() {
        let resp = AgentResponse::from_content("hello".to_string());
        assert_eq!(resp.content, "hello");
        assert_eq!(resp.prompt_tokens, None);
        assert_eq!(resp.completion_tokens, None);
    }

    #[test]
    fn test_agent_response_with_usage() {
        let resp = AgentResponse::with_usage("hello".to_string(), 10, 20);
        assert_eq!(resp.content, "hello");
        assert_eq!(resp.prompt_tokens, Some(10));
        assert_eq!(resp.completion_tokens, Some(20));
    }

    #[tokio::test]
    async fn test_run_agent_kimi_bridge_mocked() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1677652288,
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "Hello from mock bridge!"
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 7,
                    "total_tokens": 19
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = LlmBackend::KimiBridge {
            base_url: format!("{}/v1", mock_server.uri()),
            model: "test-model".to_string(),
        };

        let result = run_agent(&backend, "Say hello", None).await.unwrap();
        assert_eq!(result.content, "Hello from mock bridge!");
        assert_eq!(result.prompt_tokens, Some(12));
        assert_eq!(result.completion_tokens, Some(7));
    }
}
