//! Provider abstraction — maps PageSeeds provider names to rig-core clients.
//!
//! Supported backends:
//! - `kimi`    → three modes controlled by `kimi_backend_mode`:
//!   - `"cli"`    → native `tokio::process` calling `kimi --print` directly (no HTTP, no Python)
//!   - `"bridge"` → OpenAI-compatible bridge at localhost:8080 (Python/FastAPI)
//!   - `"auto"`   → health check bridge; fall back to direct CLI
//!   - `"direct"` → legacy agent-wrapper subprocess
//! - `claude`  → Anthropic API via rig native provider
//! - `openai`  → OpenAI API via rig native provider
//! - `ollama`  → local Ollama via rig native provider

use rig::client::CompletionClient;
use rig::completion::Prompt;

/// How to reach a given LLM.
#[derive(Debug, Clone)]
pub enum LlmBackend {
    /// Kimi via the native CLI provider — spawns `kimi --print` directly.
    /// No Python bridge, no HTTP layer. Drop-in replacement for `KimiBridge`.
    KimiCli {
        /// Working directory for the Kimi session (the project repo root).
        /// Passed as `--work-dir` so the agent's file tools operate in-scope.
        work_dir: String,
    },
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

/// Provider-name-based file-IO capability check — the single source of truth
/// for whether a provider can read/write files in the project repo itself.
///
/// Agentic CLI / ACP providers (Kimi in any mode) run an agent with file tools
/// in the repo, so a `write_article` prompt can create the MDX file on disk
/// itself. The native rig HTTP providers (Claude / OpenAI / Ollama) are pure
/// prompt→text completions — the executor must persist any returned content
/// to disk itself.
///
/// The executor only carries the configured provider name (backend resolution
/// happens inside the agent layer and may depend on health checks), hence the
/// string-based check. Unknown providers default to file-IO-capable so the
/// executor does not write agent output over a backend it does not know.
pub fn provider_supports_file_io(provider: &str) -> bool {
    !matches!(provider, "claude" | "openai" | "ollama")
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

/// Run a single prompt with an optional backend preference for the Kimi bridge.
///
/// `backend_preference` is passed as the `X-Kimi-Backend` HTTP header when using
/// the bridge, allowing per-request routing between `direct` (fast, stateless)
/// and `acp` (complex, persistent session) modes.
pub async fn run_agent_with_backend(
    backend: &LlmBackend,
    prompt: &str,
    preamble: Option<&str>,
    backend_preference: Option<&str>,
    workdir: Option<&str>,
) -> Result<AgentResponse, String> {
    match backend {
        LlmBackend::KimiCli { work_dir } => {
            // Prefer the caller's workdir (from the executor, = project repo root)
            // over the variant's placeholder (set at resolution time).
            let effective_workdir = workdir.unwrap_or(work_dir.as_str());
            run_kimi_cli(prompt, preamble, backend_preference, effective_workdir).await
        }
        LlmBackend::KimiBridge { base_url, model } => {
            run_kimi_bridge(base_url, model, prompt, preamble, backend_preference, workdir).await
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
///
/// Kept as a public compatibility wrapper even if no current caller uses it
/// directly — external consumers or future UI health checks may call it.
#[allow(dead_code)]
///
/// This is a thin compatibility wrapper around the typed health check.
pub async fn check_bridge_health(base_url: &str) -> bool {
    crate::rig::kimi_bridge::get_kimi_bridge_health(base_url)
        .await
        .map(|h| h.kimi_available)
        .unwrap_or(false)
}

/// Resolve a provider string + settings into a concrete backend.
///
/// For `"kimi"` the behaviour depends on `kimi_backend_mode`:
/// - `"cli"`   → native `kimi --print` subprocess (no bridge, no HTTP)
/// - `"bridge"`→ always use bridge (no health check)
/// - `"direct"`→ always use direct CLI (legacy agent-wrapper)
/// - `"auto"`  → health check bridge; if healthy → `KimiBridge`, otherwise fall back to `KimiDirect`
///
/// When `kimi_backend_mode` is `None`, the setting is read from the
/// `global_settings` SQLite table (the canonical source). This ensures all
/// callers — including direct `resolve_backend` users that don't go through
/// `engine::agent` — respect the user's configured backend mode.
pub async fn resolve_backend(
    provider: &str,
    bridge_url: Option<&str>,
    _api_key: Option<&str>,
    kimi_backend_mode: Option<&str>,
) -> Result<LlmBackend, String> {
    let model = default_model_for_provider(provider);

    // Read kimi_backend_mode from global settings if caller didn't specify.
    let owned_mode: String;
    let mode = match kimi_backend_mode {
        Some(m) => m,
        None => {
            owned_mode = match rusqlite::Connection::open(crate::db::default_db_path()) {
                Ok(conn) => crate::db::global_settings::get_kimi_backend_mode(&conn),
                Err(e) => {
                    log::warn!(
                        "[provider] Failed to open DB for kimi_backend_mode: {}. Using default ({}).",
                        e,
                        crate::db::global_settings::DEFAULT_KIMI_BACKEND_MODE,
                    );
                    crate::db::global_settings::DEFAULT_KIMI_BACKEND_MODE.to_string()
                }
            };
            &owned_mode
        }
    };

    match provider {
        "kimi" => {
            let bridge_url = bridge_url
                .map(|s| s.to_string())
                .or_else(|| std::env::var("KIMI_BRIDGE_URL").ok())
                .unwrap_or_else(|| "http://localhost:8080/v1".to_string());
            let bridge_url = bridge_url.as_str();
            match mode {
                "cli" => {
                    // Native CLI provider. The work_dir is resolved per-call by
                    // run_agent_with_backend (from the workdir parameter); here
                    // we use a placeholder that will be overridden.
                    if crate::rig::kimi_cli::is_kimi_available() {
                        log::info!(
                            "[provider] Kimi CLI available — using KimiCli (native subprocess)"
                        );
                        Ok(LlmBackend::KimiCli {
                            work_dir: std::env::current_dir()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|_| ".".to_string()),
                        })
                    } else {
                        Err(
                            "Kimi CLI binary 'kimi' not found on PATH. Install kimi or switch kimi_backend_mode to 'bridge' or 'auto'."
                                .to_string(),
                        )
                    }
                }
                "bridge" => Ok(LlmBackend::KimiBridge {
                    base_url: bridge_url.to_string(),
                    model,
                }),
                "direct" => Ok(LlmBackend::KimiDirect),
                _ => {
                    // "auto" or any other value → typed health check with fallback
                    match crate::rig::kimi_bridge::get_kimi_bridge_health(bridge_url).await {
                        Ok(health) => {
                            if health.kimi_available {
                                log::info!(
                                    "[provider] Kimi bridge healthy (version={}) — using KimiBridge",
                                    health.bridge_version
                                );
                                Ok(LlmBackend::KimiBridge {
                                    base_url: bridge_url.to_string(),
                                    model,
                                })
                            } else {
                                log::info!(
                                    "[provider] Kimi bridge reports kimi_available=false — falling back to KimiDirect"
                                );
                                Ok(LlmBackend::KimiDirect)
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "[provider] Kimi bridge health check failed ({}). Falling back to KimiDirect.",
                                e
                            );
                            Ok(LlmBackend::KimiDirect)
                        }
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

pub fn default_model_for_provider(provider: &str) -> String {
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

/// Run a prompt via the native Kimi CLI provider (`kimi -p`).
///
/// The `backend_preference` parameter is historical from the bridge era and
/// is now ignored — all Kimi CLI calls use the same 600s timeout. Kept in the
/// signature for API compatibility with `run_agent_with_backend`.
async fn run_kimi_cli(
    prompt: &str,
    preamble: Option<&str>,
    _backend_preference: Option<&str>,
    fallback_work_dir: &str,
) -> Result<AgentResponse, String> {
    let work_dir = if fallback_work_dir.is_empty() { "." } else { fallback_work_dir };

    crate::rig::kimi_cli::run_prompt(prompt, preamble, work_dir)
        .await
        .map_err(|e| format!("Kimi CLI prompt failed: {}", e))
}

async fn run_kimi_bridge(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
    backend_preference: Option<&str>,
    workdir: Option<&str>,
) -> Result<AgentResponse, String> {
    let result =
        crate::rig::compat::kimi::run_prompt(base_url, model, prompt, preamble, backend_preference, workdir)
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
        // Mutates process-global env — serialize against other env-mutating tests.
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
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
    fn test_provider_supports_file_io() {
        assert!(provider_supports_file_io("kimi"));
        assert!(!provider_supports_file_io("claude"));
        assert!(!provider_supports_file_io("openai"));
        assert!(!provider_supports_file_io("ollama"));
        // Unknown providers default to file-IO-capable so the executor does
        // not write agent output over a backend it does not know.
        assert!(provider_supports_file_io("unknown"));
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

        let result = run_agent_with_backend(&backend, "Say hello", None, None, None)
            .await
            .unwrap();
        assert_eq!(result.content, "Hello from mock bridge!");
        assert_eq!(result.prompt_tokens, Some(12));
        assert_eq!(result.completion_tokens, Some(7));
    }

    #[tokio::test]
    async fn test_resolve_backend_auto_healthy() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "healthy",
                "kimi_available": true,
                "bridge_version": "1.2.3",
                "models": ["kimi-k2.5"],
                "backends": {
                    "direct": {"available": true, "tool_calls": false, "json_mode": true, "file_io": false},
                    "acp": {"available": true, "tool_calls": true, "json_mode": true, "file_io": true}
                },
                "limits": {
                    "max_prompt_bytes_direct": 100000,
                    "max_prompt_bytes_acp": 100000,
                    "max_concurrent_requests": 4
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = resolve_backend(
            "kimi",
            Some(&format!("{}/v1", mock_server.uri())),
            None,
            Some("auto"),
        )
        .await
        .unwrap();
        assert!(matches!(backend, LlmBackend::KimiBridge { .. }));
    }

    #[tokio::test]
    async fn test_resolve_backend_auto_unhealthy() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "degraded",
                "kimi_available": false,
                "bridge_version": "1.0.0",
                "models": [],
                "backends": {},
                "limits": {
                    "max_prompt_bytes_direct": 100000,
                    "max_prompt_bytes_acp": 100000,
                    "max_concurrent_requests": 2
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = resolve_backend(
            "kimi",
            Some(&format!("{}/v1", mock_server.uri())),
            None,
            Some("auto"),
        )
        .await
        .unwrap();
        assert!(matches!(backend, LlmBackend::KimiDirect));
    }

    #[tokio::test]
    async fn test_resolve_backend_auto_unreachable() {
        // Use a port that is extremely unlikely to be open.
        let backend = resolve_backend("kimi", Some("http://127.0.0.1:1/v1"), None, Some("auto"))
            .await
            .unwrap();
        assert!(matches!(backend, LlmBackend::KimiDirect));
    }
}
