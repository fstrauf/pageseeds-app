//! Structured output extraction with provider-safe tool schemas.
//!
//! All structured-extract paths sanitize schemars output via
//! [`crate::rig::schema_sanitize::schemars_tool_parameters`] before attaching
//! it as tool/function parameters or injecting it into a JSON-mode prompt.
//!
//! Native Claude / OpenAI / Ollama **do not** use rig's raw `Extractor<T>` —
//! that serializes unsanitized schemas and triggers `invalid_function_parameters`
//! (e.g. `CtrFixPatch` with nested `Option<Struct>`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rig::provider::{resolve_backend, LlmBackend};
use crate::rig::schema_sanitize::schemars_tool_parameters;

/// Classify and format a structured-extraction failure for operator-facing logs.
///
/// - Labels `invalid_function_parameters` / `Invalid schema for function` as
///   `provider_schema_error`.
/// - Mentions `KimiDirect` guidance **only** when `backend_label` is `KimiDirect`.
pub fn format_extract_error(backend_label: &str, err: &str) -> String {
    let is_schema = err.contains("invalid_function_parameters")
        || err.contains("Invalid schema for function")
        || err.contains("provider_schema_error");

    if is_schema {
        format!(
            "backend={} provider_schema_error: {}. \
             Tool/function parameters must pass schema_sanitize; \
             unsanitized schemars Option/nested forms are rejected by providers.",
            backend_label, err
        )
    } else if backend_label == "KimiDirect" {
        format!(
            "backend={}: {}. \
             Structured extraction is not supported with KimiDirect (CLI fallback). \
             Switch to Kimi bridge, Claude, OpenAI, or Ollama.",
            backend_label, err
        )
    } else {
        format!("backend={}: {}", backend_label, err)
    }
}

/// Extract structured data from an LLM prompt.
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
    let preamble_str = preamble.unwrap_or(default_preamble);

    match backend {
        LlmBackend::KimiCli { work_dir } => {
            // Native CLI provider uses JSON-mode extraction: sanitized schema is
            // injected into the prompt, the model responds with raw JSON.
            let schema_value = schemars_tool_parameters::<T>()?;

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
            // Bridge path already sanitizes via schema_sanitize.
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
            crate::rig::openai_compatible_extract::extract_claude::<T>(
                api_key,
                model,
                prompt,
                preamble_str,
                max_tokens,
            )
            .await
        }
        LlmBackend::OpenAi { api_key, model } => {
            crate::rig::openai_compatible_extract::extract_openai::<T>(
                api_key,
                model,
                prompt,
                preamble_str,
                max_tokens,
            )
            .await
        }
        LlmBackend::GrokCli { work_dir } => {
            // Same JSON-mode extraction as Kimi CLI (sanitized schema in prompt).
            let schema_value = schemars_tool_parameters::<T>()?;

            crate::rig::grok_cli::extract_structured::<T>(
                prompt,
                preamble,
                &schema_value,
                work_dir,
            )
            .await
        }
        LlmBackend::Ollama { base_url, model } => {
            crate::rig::openai_compatible_extract::extract_ollama::<T>(
                base_url,
                model,
                prompt,
                preamble_str,
                max_tokens,
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
    use crate::models::ctr::CtrFixPatch;
    use crate::rig::schema_sanitize::schemars_tool_parameters;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct TestOutput {
        pub name: String,
        pub count: i32,
    }

    #[tokio::test]
    async fn test_extract_structured_rejects_unknown_provider() {
        let result =
            extract_structured::<TestOutput>("unknown_provider", "test", None, None, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown provider"));
    }

    #[tokio::test]
    async fn test_extract_with_backend_rejects_kimi_direct() {
        // Test the backend directly — avoids env-var sensitivity from resolve_backend.
        let backend = LlmBackend::KimiDirect;
        let result: Result<TestOutput, String> =
            extract_with_backend(&backend, "test", None, None, None).await;
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

    #[test]
    fn schemars_tool_parameters_ctr_fix_patch_has_no_hostile_forms() {
        let params = schemars_tool_parameters::<CtrFixPatch>().unwrap();
        let s = params.to_string();
        assert!(!s.contains("\"anyOf\""), "sanitized still has anyOf: {}", s);
        assert!(!s.contains("\"$ref\""), "sanitized still has $ref: {}", s);
        assert!(!s.contains("\"$defs\""), "sanitized still has $defs: {}", s);
        assert_eq!(
            params.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "CtrFixPatch tool parameters must be type object"
        );
    }

    #[test]
    fn format_extract_error_classifies_invalid_function_parameters() {
        let msg = format_extract_error(
            "OpenAi",
            "CompletionError: HttpError: 400 \"Invalid schema for function 'submit'\" code: invalid_function_parameters",
        );
        assert!(
            msg.contains("provider_schema_error"),
            "expected provider_schema_error label: {}",
            msg
        );
        assert!(msg.contains("backend=OpenAi"), "expected backend label: {}", msg);
        assert!(
            !msg.contains("KimiDirect"),
            "must not mention KimiDirect for OpenAi backend: {}",
            msg
        );
    }

    #[test]
    fn format_extract_error_mentions_kimi_direct_only_for_that_backend() {
        let msg = format_extract_error("KimiDirect", "not supported");
        assert!(msg.contains("KimiDirect"));
        assert!(msg.contains("not supported") || msg.contains("CLI fallback"));

        let other = format_extract_error("Claude", "timeout");
        assert!(!other.contains("KimiDirect") || other.starts_with("backend=Claude"));
        assert!(!other.contains("CLI fallback"));
        assert!(other.contains("backend=Claude"));
    }

    #[tokio::test]
    async fn test_extract_with_backend_mocked() {
        let mock_server = MockServer::start().await;

        // Kimi bridge extract path posts to /v1/chat/completions with tools.
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
