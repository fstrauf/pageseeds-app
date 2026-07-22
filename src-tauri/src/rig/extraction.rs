//! Structured output extraction using rig's `Extractor<T>`.
//!
//! This module provides `extract_structured<T>`, a type-safe way to get
//! structured JSON output from an LLM using rig's `Extractor<T>`.
//!
//! Instead of parsing raw LLM text with heuristics (fenced blocks, bare JSON,
//! first line), we send the target schema to the model and require it to call
//! a `submit` tool with structured arguments. Rig handles retries,
//! deserialization, and error reporting.

use rig::client::CompletionClient;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rig::provider::{resolve_backend, LlmBackend};

/// Shared extractor path for Claude / OpenAI / Grok (and Ollama after client build).
/// Kimi bridge/CLI stay on their own extraction implementations.
async fn extract_with_native_client<C, T>(
    client: C,
    model: &str,
    prompt: &str,
    preamble: &str,
) -> Result<T, String>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let extractor = client
        .extractor::<T>(model)
        .preamble(preamble)
        .build();
    extractor.extract(prompt).await.map_err(|e| e.to_string())
}

/// Extract structured data from an LLM prompt using rig's `Extractor<T>`.
///
/// # Type Parameters
/// * `T` - The target type. Must implement `JsonSchema`, `Deserialize`, and `Serialize`.
///
/// # Arguments
/// * `provider_name` - The LLM provider (`"kimi"`, `"claude"`, `"openai"`, `"grok"`, `"ollama"`)
/// * `prompt` - The user prompt / extraction instruction
/// * `preamble` - Optional system preamble (added to extractor's built-in preamble)
/// * `backend_preference` - Kimi bridge routing: `Some("direct")` for fast stateless
///   extraction (recommended for analysis, recommendations, audits), or `Some("acp")`
///   for project-aware extraction that may need persistent session / file I/O.
///   Pass `None` to let the bridge decide its default.
///
/// # Errors
/// Returns `Err(String)` on:
/// - Backend resolution failure
/// - LLM completion errors
/// - Deserialization failures after all retries
/// - Unsupported backend (e.g. `KimiDirect` CLI fallback)
///
/// # Example
/// ```ignore
/// #[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// struct SeedExtractionOutput {
///     themes: Vec<String>,
/// }
///
/// let result = extract_structured::<SeedExtractionOutput>(
///     "kimi",
///     "Extract 3 research themes from this brief: ...",
///     Some("You are a keyword research assistant."),
///     Some("direct"),
/// ).await?;
/// ```
pub async fn extract_structured<T>(
    provider_name: &str,
    prompt: &str,
    preamble: Option<&str>,
    backend_preference: Option<&str>,
    max_tokens: Option<u64>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let backend = match resolve_backend(provider_name, None, None, None).await {
        Ok(b) => b,
        Err(e) => return Err(e),
    };

    match &backend {
        LlmBackend::KimiDirect => {
            return Err(
                "Structured extraction is not supported with KimiDirect (CLI fallback). \
                 Please ensure the Kimi bridge is running or use another provider."
                    .to_string(),
            );
        }
        _ => {}
    }

    extract_with_backend(&backend, prompt, preamble, backend_preference, max_tokens).await
}

pub(crate) async fn extract_with_backend<T>(
    backend: &LlmBackend,
    prompt: &str,
    preamble: Option<&str>,
    backend_preference: Option<&str>,
    max_tokens: Option<u64>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let default_preamble = "Extract structured data from the provided text. \
        Always use the submit tool to return your answer. \
        Fill out every field and do not omit any required information.";

    match backend {
        LlmBackend::KimiCli { work_dir } => {
            // Native CLI provider uses JSON-mode extraction: schema is injected
            // into the prompt, the model responds with raw JSON, and we parse it.
            // No tool-calling round-trip (print mode emits text, not tool_calls).
            let schema = schemars::schema_for!(T);
            let schema_value = serde_json::to_value(&schema)
                .map_err(|e| format!("Failed to serialize JSON schema: {}", e))?;

            crate::rig::kimi_cli::extract_structured::<T>(
                prompt,
                preamble,
                &schema_value,
                work_dir,
            )
            .await
        }
        LlmBackend::KimiBridge { base_url, model } => {
            // `backend_preference` is passed as the X-Kimi-Backend header.
            // - "direct": 120s timeout, fast, stateless. Falls back to JSON mode
            //   if native tool calls are not supported.
            // - "acp": 300s timeout, project-aware, may queue behind concurrency limit.
            // - None: bridge default (usually direct).
            crate::rig::compat::kimi::extract_structured::<T>(
                base_url,
                model,
                prompt,
                preamble,
                backend_preference,
                max_tokens,
            )
            .await
        }
        LlmBackend::Claude { api_key, model } => {
            let client = rig::providers::anthropic::Client::new(api_key)
                .map_err(|e| format!("Failed to build Claude client: {}", e))?;
            extract_with_native_client(
                client,
                model,
                prompt,
                preamble.unwrap_or(default_preamble),
            )
            .await
        }
        LlmBackend::OpenAi { api_key, model } => {
            let client = rig::providers::openai::Client::new(api_key)
                .map_err(|e| format!("Failed to build OpenAI client: {}", e))?;
            extract_with_native_client(
                client,
                model,
                prompt,
                preamble.unwrap_or(default_preamble),
            )
            .await
        }
        LlmBackend::Grok { api_key, model } => {
            let client = rig::providers::xai::Client::new(api_key)
                .map_err(|e| format!("Failed to build Grok (xAI) client: {}", e))?;
            extract_with_native_client(
                client,
                model,
                prompt,
                preamble.unwrap_or(default_preamble),
            )
            .await
        }
        LlmBackend::Ollama { base_url, model } => {
            use rig::client::Nothing;
            let client = rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
                .map_err(|e| format!("Failed to build Ollama client: {}", e))?;
            // Construction differs (Nothing + base_url); extractor path is shared.
            extract_with_native_client(
                client,
                model,
                prompt,
                preamble.unwrap_or(default_preamble),
            )
            .await
        }
        LlmBackend::KimiDirect => {
            // Should never reach here because the caller checks this first.
            Err("KimiDirect does not support structured extraction".to_string())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct TestOutput {
        pub name: String,
        pub count: i32,
    }

    #[tokio::test]
    async fn test_extract_structured_rejects_unknown_provider() {
        let result = extract_structured::<TestOutput>("unknown_provider", "test", None, None, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown provider"));
    }

    #[tokio::test]
    async fn test_extract_with_backend_rejects_kimi_direct() {
        // Test the backend directly — avoids env-var sensitivity from resolve_backend.
        let backend = LlmBackend::KimiDirect;
        let result: Result<TestOutput, String> = extract_with_backend(&backend, "test", None, None, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("KimiDirect"));
        assert!(err.contains("does not support"));
    }

    #[test]
    fn test_extract_structured_type_requirements() {
        // Verify that TestOutput meets the trait bounds required by extract_structured.
        // This is a compile-time check; if it compiles, the traits are satisfied.
        fn _assert_bounds<T>()
        where
            T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
        {
        }
        _assert_bounds::<TestOutput>();
    }

    #[tokio::test]
    async fn test_extract_with_backend_mocked() {
        let mock_server = MockServer::start().await;

        // Rig's Extractor<T> sends a POST to /v1/chat/completions with a tool-calling
        // request and expects a response containing tool_calls.
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
                            "content": null,
                            "tool_calls": [
                                {
                                    "id": "call_abc123",
                                    "type": "function",
                                    "function": {
                                        "name": "submit",
                                        "arguments": "{\"name\":\"mocked-name\",\"count\":42}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ],
                "usage": {
                    "prompt_tokens": 25,
                    "completion_tokens": 15,
                    "total_tokens": 40
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = LlmBackend::KimiBridge {
            base_url: format!("{}/v1", mock_server.uri()),
            model: "test-model".to_string(),
        };

        let result: TestOutput = extract_with_backend(
            &backend,
            "Extract the name and count from this prompt.",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.name, "mocked-name");
        assert_eq!(result.count, 42);
    }
}
