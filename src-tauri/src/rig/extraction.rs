//! Structured output extraction using rig's `Extractor<T>`.
//!
//! This module provides `extract_structured<T>`, a type-safe replacement for
//! the regex-based `normalize_agent_output` in `engine/normalizer.rs`.
//!
//! Instead of parsing raw LLM text with heuristics (fenced blocks, bare JSON,
//! first line), we send the target schema to the model and require it to call
//! a `submit` tool with structured arguments. Rig handles retries,
//! deserialization, and error reporting.

use rig::client::CompletionClient;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rig::provider::{resolve_backend, LlmBackend};

/// Extract structured data from an LLM prompt using rig's `Extractor<T>`.
///
/// # Type Parameters
/// * `T` - The target type. Must implement `JsonSchema`, `Deserialize`, and `Serialize`.
///
/// # Arguments
/// * `provider_name` - The LLM provider (`"kimi"`, `"claude"`, `"openai"`, `"ollama"`)
/// * `prompt` - The user prompt / extraction instruction
/// * `preamble` - Optional system preamble (added to extractor's built-in preamble)
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
/// ).await?;
/// ```
pub async fn extract_structured<T>(
    provider_name: &str,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let backend = resolve_backend(provider_name, None, None, None).await;

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

    extract_with_backend(&backend, prompt, preamble).await
}

async fn extract_with_backend<T>(
    backend: &LlmBackend,
    prompt: &str,
    preamble: Option<&str>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let default_preamble = "Extract structured data from the provided text. \
        Always use the submit tool to return your answer. \
        Fill out every field and do not omit any required information.";

    match backend {
        LlmBackend::KimiBridge { base_url, model } => {
            let client = rig::providers::openai::Client::builder()
                .base_url(base_url)
                .api_key("dummy")
                .build()
                .map_err(|e| format!("Failed to build OpenAI client for bridge: {}", e))?;
            let completion_client = client.completions_api();
            let extractor = completion_client
                .extractor::<T>(model)
                .preamble(preamble.unwrap_or(default_preamble))
                .build();
            extractor.extract(prompt).await.map_err(|e| e.to_string())
        }
        LlmBackend::Claude { api_key, model } => {
            let client = rig::providers::anthropic::Client::new(api_key)
                .map_err(|e| format!("Failed to build Claude client: {}", e))?;
            let extractor = client
                .extractor::<T>(model)
                .preamble(preamble.unwrap_or(default_preamble))
                .build();
            extractor.extract(prompt).await.map_err(|e| e.to_string())
        }
        LlmBackend::OpenAi { api_key, model } => {
            let client = rig::providers::openai::Client::new(api_key)
                .map_err(|e| format!("Failed to build OpenAI client: {}", e))?;
            let extractor = client
                .extractor::<T>(model)
                .preamble(preamble.unwrap_or(default_preamble))
                .build();
            extractor.extract(prompt).await.map_err(|e| e.to_string())
        }
        LlmBackend::Ollama { base_url, model } => {
            use rig::client::Nothing;
            let client = rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
                .map_err(|e| format!("Failed to build Ollama client: {}", e))?;
            let extractor = client
                .extractor::<T>(model)
                .preamble(preamble.unwrap_or(default_preamble))
                .build();
            extractor.extract(prompt).await.map_err(|e| e.to_string())
        }
        LlmBackend::KimiDirect => {
            // Should never reach here because the caller checks this first.
            Err("KimiDirect does not support structured extraction".to_string())
        }
    }
}
