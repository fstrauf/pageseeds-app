//! Strict Kimi wire-format adapter.
//!
//! Kimi's API (via the local ACP bridge) is OpenAI-compatible but validates
//! `content` fields as plain strings for system messages. Rig's OpenAI provider
//! serializes system message content as `[{"type":"text","text":"..."}]`
//! because `Message::System` uses `OneOrMany<SystemContent>` without the
//! flattening serializer that user messages have.
//!
//! This adapter bypasses Rig's native serialization for Kimi and produces
//! requests where every `content` field is a strict `String`.

use reqwest;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Request types (strict string content)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
struct ChatRequest {
    model: String,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize, Clone)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Debug, Serialize, Clone)]
struct RequestMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Clone)]
struct ToolDefinition {
    #[serde(rename = "type")]
    tool_type: String,
    function: FunctionDefinition,
}

#[derive(Debug, Serialize, Clone)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// tool_choice is serialized as a plain string ("auto", "none", "required")
// or as an object when forcing a specific function. We keep it simple here.

// ─────────────────────────────────────────────────────────────────────────────
// Response types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ChatResponse {
    id: String,
    choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Choice {
    message: ResponseMessage,
    finish_reason: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ResponseMessage {
    role: String,
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: ToolFunction,
}

#[derive(Debug, Deserialize, Clone)]
struct ToolFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a Kimi chat completion.
#[derive(Debug, Clone)]
pub struct KimiChatResult {
    pub content: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

/// Run a simple prompt through Kimi bridge.
///
/// System preambles are sent as user messages to avoid the array-content
/// validation issue.
pub async fn run_prompt(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
    backend: Option<&str>,
) -> Result<KimiChatResult, String> {
    let mut messages = Vec::new();
    if let Some(preamble) = preamble {
        messages.push(RequestMessage {
            role: "user".to_string(),
            content: format!("[System instructions]\n\n{}", preamble),
        });
    }
    messages.push(RequestMessage {
        role: "user".to_string(),
        content: prompt.to_string(),
    });

    let request = ChatRequest {
        model: model.to_string(),
        messages,
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        response_format: None,
    };

    let response = send_request(base_url, request, backend).await?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or("No choices in Kimi response")?;

    let content = choice.message.content.unwrap_or_default();
    let (pt, ct) = response
        .usage
        .map(|u| (Some(u.prompt_tokens), Some(u.completion_tokens)))
        .unwrap_or((None, None));

    Ok(KimiChatResult {
        content,
        prompt_tokens: pt,
        completion_tokens: ct,
    })
}

/// Extract structured data from Kimi.
///
/// First tries tool-calling (preferred for ACP mode). If the bridge is running
/// in direct mode and rejects tool calls, transparently falls back to JSON mode
/// (`response_format: { type: "json_object" }`) and parses the response content.
///
/// Retries up to 2 times on parse failures.
pub async fn extract_structured<T>(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
    backend: Option<&str>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    let schema_value = schema_to_parameters::<T>()?;

    let tool = ToolDefinition {
        tool_type: "function".to_string(),
        function: FunctionDefinition {
            name: "submit".to_string(),
            description: "Submit the extracted structured data.".to_string(),
            parameters: schema_value.clone(),
        },
    };

    let default_preamble = "Extract structured data from the provided text. \
         Always use the submit tool to return your answer. \
         Fill out every field and do not omit any required information.";

    let full_prompt = if let Some(preamble) = preamble {
        format!("{}\n\n{}", preamble, prompt)
    } else {
        prompt.to_string()
    };

    let user_content = format!("{}\n\n{}", default_preamble, full_prompt);

    let tool_request = ChatRequest {
        model: model.to_string(),
        messages: vec![RequestMessage {
            role: "user".to_string(),
            content: user_content,
        }],
        temperature: None,
        max_tokens: None,
        tools: Some(vec![tool]),
        tool_choice: Some("auto".to_string()),
        response_format: None,
    };

    // Attempt tool-calling first.
    let mut last_error = String::new();
    match send_request(base_url, tool_request.clone(), backend).await {
        Ok(response) => {
            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or("No choices in Kimi response")?;

            // Prefer tool call.
            if let Some(tool_call) = choice
                .message
                .tool_calls
                .into_iter()
                .find(|tc| tc.function.name == "submit")
            {
                match serde_json::from_str::<T>(&tool_call.function.arguments) {
                    Ok(value) => return Ok(value),
                    Err(e) => {
                        last_error = format!(
                            "Tool call parse error: {} | raw: {}",
                            e,
                            tool_call.function.arguments
                        );
                        log::warn!("[kimi::extract_structured] {}", last_error);
                    }
                }
            }

            // Fallback within same response: parse content as JSON.
            if let Some(content) = choice.message.content {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    match serde_json::from_str::<T>(trimmed) {
                        Ok(value) => return Ok(value),
                        Err(e) => {
                            last_error = format!(
                                "Content parse error: {} | raw: {}",
                                e, trimmed
                            );
                            log::warn!("[kimi::extract_structured] {}", last_error);
                        }
                    }
                }
            }
        }
        Err(e) if is_tools_not_supported(&e) => {
            log::info!(
                "[kimi::extract_structured] Tool calls not supported (direct mode). \
                 Falling back to JSON mode."
            );
            return extract_structured_json_mode::<T>(
                base_url,
                model,
                prompt,
                preamble,
                &schema_value,
                backend,
            )
            .await;
        }
        Err(e) => return Err(e),
    }

    // If we reach here, the tool request returned OK but parsing failed.
    // Retry up to 2 more times with the same tool request.
    for attempt in 1..3 {
        let response = send_request(base_url, tool_request.clone(), backend).await?;
        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or("No choices in Kimi response")?;

        if let Some(tool_call) = choice
            .message
            .tool_calls
            .into_iter()
            .find(|tc| tc.function.name == "submit")
        {
            match serde_json::from_str::<T>(&tool_call.function.arguments) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    last_error = format!(
                        "Tool call parse error (attempt {}): {} | raw: {}",
                        attempt + 1,
                        e,
                        tool_call.function.arguments
                    );
                    log::warn!("[kimi::extract_structured] {}", last_error);
                    continue;
                }
            }
        }

        if let Some(content) = choice.message.content {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                match serde_json::from_str::<T>(trimmed) {
                    Ok(value) => return Ok(value),
                    Err(e) => {
                        last_error = format!(
                            "Content parse error (attempt {}): {} | raw: {}",
                            attempt + 1,
                            e,
                            trimmed
                        );
                        log::warn!("[kimi::extract_structured] {}", last_error);
                        continue;
                    }
                }
            }
        }

        last_error = format!(
            "Attempt {}: Response contained neither tool calls nor parseable content",
            attempt + 1
        );
        log::warn!("[kimi::extract_structured] {}", last_error);
    }

    Err(format!(
        "Kimi structured extraction failed after 3 attempts. Last error: {}",
        last_error
    ))
}

/// Extract structured data using JSON mode (no tool calls).
///
/// Used as a fallback when the Kimi bridge is running in direct mode.
/// The JSON schema is injected into the prompt; `response_format` forces
/// the model to emit valid JSON.
async fn extract_structured_json_mode<T>(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: Option<&str>,
    schema: &serde_json::Value,
    backend: Option<&str>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    let schema_str = serde_json::to_string_pretty(schema)
        .map_err(|e| format!("Failed to serialize schema: {}", e))?;

    let json_preamble = format!(
        "You are a structured data extraction assistant. \
         Respond ONLY with a valid JSON object that conforms to the following schema. \
         Do not include markdown code fences, explanations, or any text outside the JSON.\n\n\
         Schema:\n{}\n\n\
         Your response must be a single JSON object. Fill out every field.",
        schema_str
    );

    let full_prompt = if let Some(preamble) = preamble {
        format!("{}\n\n{}", preamble, prompt)
    } else {
        prompt.to_string()
    };

    let user_content = format!("{}\n\n{}", json_preamble, full_prompt);

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![RequestMessage {
            role: "user".to_string(),
            content: user_content,
        }],
        temperature: Some(0.1),
        max_tokens: None,
        tools: None,
        tool_choice: None,
        response_format: Some(ResponseFormat {
            format_type: "json_object".to_string(),
        }),
    };

    let mut last_error = String::new();
    for attempt in 0..3 {
        let response = send_request(base_url, request.clone(), backend).await?;
        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or("No choices in Kimi response")?;

        if let Some(content) = choice.message.content {
            let trimmed = content.trim();
            // Strip markdown fences if the model included them despite instructions.
            let cleaned = strip_json_fences(trimmed);
            if !cleaned.is_empty() {
                match serde_json::from_str::<T>(cleaned) {
                    Ok(value) => return Ok(value),
                    Err(e) => {
                        last_error = format!(
                            "JSON mode parse error (attempt {}): {} | raw: {}",
                            attempt + 1,
                            e,
                            cleaned
                        );
                        log::warn!("[kimi::extract_structured_json_mode] {}", last_error);
                        continue;
                    }
                }
            }
        }

        last_error = format!(
            "JSON mode attempt {}: Response contained no parseable content",
            attempt + 1
        );
        log::warn!("[kimi::extract_structured_json_mode] {}", last_error);
    }

    Err(format!(
        "Kimi JSON-mode structured extraction failed after 3 attempts. Last error: {}",
        last_error
    ))
}

/// Check whether an error indicates tool calls are not supported.
fn is_tools_not_supported(error: &str) -> bool {
    error.contains("tools_not_supported")
        || error.contains("does not support native tool calls")
        || error.contains("does not support structured extraction")
        || error.contains("tool_calls")
        || error.contains("Tool calling")
}

/// Strip markdown JSON code fences from a string.
fn strip_json_fences(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.starts_with("```json") {
        trimmed
            .strip_prefix("```json")
            .and_then(|s| s.strip_suffix("```"))
            .map(|s| s.trim())
            .unwrap_or(trimmed)
    } else if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```")
            .and_then(|s| s.strip_suffix("```"))
            .map(|s| s.trim())
            .unwrap_or(trimmed)
    } else {
        trimmed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a schemars schema into function parameters.
///
/// The full JSON Schema object is passed through as-is, including `$defs` for
/// nested types. Stripping `$defs` would leave dangling `$ref` pointers.
fn schema_to_parameters<T: JsonSchema>() -> Result<serde_json::Value, String> {
    let schema = schemars::schema_for!(T);
    serde_json::to_value(&schema).map_err(|e| format!("Failed to serialize JSON schema: {}", e))
}

async fn send_request(
    base_url: &str,
    request: ChatRequest,
    backend: Option<&str>,
) -> Result<ChatResponse, String> {
    // Use a 180-second timeout so we fail faster than the bridge's own 300s timeout.
    // This lets us retry promptly instead of waiting 5 minutes for a 500.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    log::debug!(
        "[kimi::send_request] POST {} with body: {}",
        url,
        serde_json::to_string_pretty(&request).unwrap_or_default()
    );

    let mut last_error = String::new();
    for attempt in 1..=3 {
        let start = std::time::Instant::now();
        log::info!(
            "[kimi::send_request] >>> START POST {} (attempt {}/3)",
            url,
            attempt
        );

        let mut req = client
            .post(&url)
            .header("Authorization", "Bearer dummy")
            .json(&request);

        if let Some(b) = backend {
            req = req.header("X-Kimi-Backend", b);
        }

        let resp = match req.send().await
        {
            Ok(r) => r,
            Err(e) if e.is_timeout() => {
                log::warn!(
                    "[kimi::send_request] Attempt {}/3 timed out after {:?}: {}",
                    attempt,
                    start.elapsed(),
                    e
                );
                last_error = format!("Kimi request timed out: {}", e);
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_secs(2_u64.pow(attempt))).await;
                    continue;
                }
                return Err(last_error);
            }
            Err(e) => {
                log::error!(
                    "[kimi::send_request] Request failed after {:?}: {}",
                    start.elapsed(),
                    e
                );
                return Err(format!("Kimi request failed: {}", e));
            }
        };

        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            log::error!(
                "[kimi::send_request] Body read failed after {:?}: {}",
                start.elapsed(),
                e
            );
            e.to_string()
        })?;

        let elapsed = start.elapsed();

        if !status.is_success() {
            log::error!(
                "[kimi::send_request] <<< END ERROR status={} duration={:?} body={}",
                status,
                elapsed,
                body
            );

            // Try structured bridge error first.
            if let Some(bridge_err) = crate::rig::kimi_bridge::parse_bridge_error(&body) {
                let formatted = crate::rig::kimi_bridge::format_bridge_error(&bridge_err);
                log::error!("[kimi::send_request] Structured bridge error: {:?}", bridge_err);

                if !crate::rig::kimi_bridge::is_bridge_error_retryable(&bridge_err) {
                    return Err(formatted);
                }

                // Retryable bridge errors: only retry on useful HTTP statuses.
                let should_retry = status.as_u16() == 429 || status.as_u16() == 503 || status.as_u16() == 504;
                if should_retry && attempt < 3 {
                    let backoff = 2_u64.pow(attempt);
                    log::info!(
                        "[kimi::send_request] Retrying in {}s due to retryable bridge error {}",
                        backoff,
                        bridge_err.code
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    continue;
                }

                return Err(formatted);
            }

            // Fallback: opaque body detection for backward compatibility.
            if body.contains("tools_not_supported")
                || body.contains("does not support native tool calls")
            {
                return Err(
                    "Kimi bridge is running in direct mode, which does not support structured extraction (native tool calls). \
                     Restart the bridge with --backend acp (or set KIMI_BACKEND=acp) and try again."
                        .to_string(),
                );
            }

            last_error = format!("Kimi API error {}: {}", status, body);

            // Retry on 5xx (server / bridge errors) and 429 (rate limit).
            let should_retry = status.is_server_error() || status.as_u16() == 429;
            if should_retry && attempt < 3 {
                let backoff = 2_u64.pow(attempt);
                log::info!(
                    "[kimi::send_request] Retrying in {}s due to status {}",
                    backoff,
                    status
                );
                tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                continue;
            }

            return Err(last_error);
        }

        let parsed: ChatResponse = serde_json::from_str(&body).map_err(|e| {
            log::error!(
                "[kimi::send_request] Parse error after {:?}: {}",
                elapsed,
                e
            );
            format!("Kimi response parse error: {} | body: {}", e, body)
        })?;

        log::info!(
            "[kimi::send_request] <<< END OK request_id={} duration={:?} prompt_tokens={:?} completion_tokens={:?}",
            parsed.id,
            elapsed,
            parsed.usage.as_ref().map(|u| u.prompt_tokens),
            parsed.usage.as_ref().map(|u| u.completion_tokens)
        );

        return Ok(parsed);
    }

    Err(format!(
        "Kimi request failed after 3 attempts. Last error: {}",
        last_error
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use wiremock::matchers::{method, path};
    use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct TestOutput {
        pub name: String,
        pub count: i32,
    }

    #[tokio::test]
    async fn test_run_prompt_success() {
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
                            "content": "Hello from Kimi!"
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            })))
            .mount(&mock_server)
            .await;

        let result = run_prompt(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Say hello",
            Some("You are a helpful assistant."),
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.content, "Hello from Kimi!");
        assert_eq!(result.prompt_tokens, Some(10));
        assert_eq!(result.completion_tokens, Some(5));
    }

    #[tokio::test]
    async fn test_run_prompt_no_preamble() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "No preamble received"
                        },
                        "finish_reason": "stop"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let result = run_prompt(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Test prompt",
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.content, "No preamble received");
        assert_eq!(result.prompt_tokens, None);
    }

    #[tokio::test]
    async fn test_extract_structured_via_tool_call() {
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

        let result: TestOutput = extract_structured(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Extract name and count.",
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.name, "mocked-name");
        assert_eq!(result.count, 42);
    }

    #[tokio::test]
    async fn test_extract_structured_fallback_to_content() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "{\"name\":\"content-name\",\"count\":99}"
                        },
                        "finish_reason": "stop"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let result: TestOutput = extract_structured(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Extract name and count.",
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.name, "content-name");
        assert_eq!(result.count, 99);
    }

    #[tokio::test]
    async fn test_extract_structured_api_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(422).set_body_string(
                r#"{"detail":[{"type":"string_type","loc":["body","messages",0,"content"],"msg":"Input should be a valid string"}]}"#
            ))
            .mount(&mock_server)
            .await;

        let result: Result<TestOutput, String> = extract_structured(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Extract name and count.",
            None,
            None,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("422"));
    }

    #[test]
    fn test_request_message_serialization_is_string() {
        let msg = RequestMessage {
            role: "system".to_string(),
            content: "You are helpful.".to_string(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "system");
        assert!(json["content"].is_string());
        assert_eq!(json["content"], "You are helpful.");
    }

    #[test]
    fn test_chat_request_has_no_array_content() {
        let req = ChatRequest {
            model: "test".to_string(),
            messages: vec![
                RequestMessage {
                    role: "system".to_string(),
                    content: "sys".to_string(),
                },
                RequestMessage {
                    role: "user".to_string(),
                    content: "user".to_string(),
                },
            ],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            response_format: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        let messages = json["messages"].as_array().unwrap();
        for msg in messages {
            assert!(
                msg["content"].is_string(),
                "Kimi message content must be string, got: {:?}",
                msg["content"]
            );
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct NestedItem {
        pub label: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct NestedOutput {
        pub items: Vec<NestedItem>,
        pub total: i32,
    }

    #[test]
    fn test_schema_to_parameters_preserves_defs_for_nested_types() {
        let params = schema_to_parameters::<NestedOutput>().unwrap();
        let obj = params.as_object().unwrap();

        // The schema must contain $defs because NestedItem is referenced via $ref.
        assert!(
            obj.contains_key("$defs"),
            "Schema for nested type must contain $defs, got: {}",
            serde_json::to_string_pretty(&params).unwrap()
        );

        // Verify no dangling $ref: every $ref must point into $defs.
        let defs = obj.get("$defs").unwrap().as_object().unwrap();
        let refs = collect_refs(&params);
        for r in refs {
            let stripped = r.trim_start_matches("#/$defs/");
            assert!(
                defs.contains_key(stripped),
                "Dangling $ref: {} (not found in $defs keys: {:?})",
                r,
                defs.keys().collect::<Vec<_>>()
            );
        }
    }

    /// Custom wiremock matcher that validates the Kimi extraction request body.
    struct ValidKimiExtractionRequest;

    impl Match for ValidKimiExtractionRequest {
        fn matches(&self, request: &Request) -> bool {
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).unwrap_or(serde_json::Value::Null);

            // 1. Every message content must be a string.
            if let Some(messages) = body["messages"].as_array() {
                for msg in messages {
                    if !msg["content"].is_string() {
                        return false;
                    }
                }
            } else {
                return false;
            }

            // 2. tool_choice must be the plain string "auto", not {"type":"auto"}.
            if let Some(tc) = body.get("tool_choice") {
                if tc != "auto" {
                    return false;
                }
            } else {
                return false;
            }

            // 3. The tool schema must not contain dangling $ref pointers.
            if let Some(tools) = body["tools"].as_array() {
                if let Some(first_tool) = tools.first() {
                    if let Some(params) = first_tool["function"]["parameters"].as_object() {
                        if let Some(defs) = params.get("$defs").and_then(|v| v.as_object()) {
                            let refs = collect_refs(&serde_json::json!(params));
                            for r in refs {
                                let stripped = r.trim_start_matches("#/$defs/");
                                if !defs.contains_key(stripped) {
                                    return false;
                                }
                            }
                        }
                    }
                }
            }

            true
        }
    }

    #[tokio::test]
    async fn test_extract_structured_request_body_shape() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(ValidKimiExtractionRequest)
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-validated",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [
                                {
                                    "id": "call_validate",
                                    "type": "function",
                                    "function": {
                                        "name": "submit",
                                        "arguments": "{\"items\":[{\"label\":\"a\"}],\"total\":1}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let result: NestedOutput = extract_structured(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Extract items and total.",
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].label, "a");
        assert_eq!(result.total, 1);
    }

    #[tokio::test]
    async fn test_extract_structured_fallback_to_json_mode_on_tools_not_supported() {
        let mock_server = MockServer::start().await;

        // First request (tool-calling) fails with direct-mode error.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "detail": {
                    "error": {
                        "message": "Kimi direct backend does not support native tool calls. Use tool_choice='none' or backend='acp'."
                    }
                }
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // Second request (JSON mode) succeeds.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-json",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "{\"name\":\"json-mode-name\",\"count\":77}"
                        },
                        "finish_reason": "stop"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let result: TestOutput = extract_structured(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Extract name and count.",
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.name, "json-mode-name");
        assert_eq!(result.count, 77);
    }

    #[tokio::test]
    async fn test_extract_structured_json_mode_strips_markdown_fences() {
        let mock_server = MockServer::start().await;

        // Tool request fails.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"detail":{"error":{"message":"tools_not_supported"}}}"#
            ))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // JSON mode returns fenced JSON.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-fenced",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "```json\n{\"name\":\"fenced-name\",\"count\":88}\n```"
                        },
                        "finish_reason": "stop"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let result: TestOutput = extract_structured(
            &format!("{}/v1", mock_server.uri()),
            "test-model",
            "Extract name and count.",
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.name, "fenced-name");
        assert_eq!(result.count, 88);
    }

    #[test]
    fn test_is_tools_not_supported_detects_variants() {
        assert!(is_tools_not_supported("tools_not_supported"));
        assert!(is_tools_not_supported("does not support native tool calls"));
        assert!(is_tools_not_supported("tool_calls not allowed"));
        assert!(!is_tools_not_supported("rate limit exceeded"));
        assert!(!is_tools_not_supported(""));
    }

    #[test]
    fn test_strip_json_fences() {
        assert_eq!(
            strip_json_fences("```json\n{\"a\":1}\n```"),
            "{\"a\":1}"
        );
        assert_eq!(
            strip_json_fences("```\n{\"a\":1}\n```"),
            "{\"a\":1}"
        );
        assert_eq!(
            strip_json_fences("{\"a\":1}"),
            "{\"a\":1}"
        );
    }

    /// Recursively collect all `$ref` values from a JSON value.
    fn collect_refs(value: &serde_json::Value) -> Vec<String> {
        let mut refs = Vec::new();
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == "$ref" {
                        if let Some(s) = v.as_str() {
                            refs.push(s.to_string());
                        }
                    }
                    refs.extend(collect_refs(v));
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    refs.extend(collect_refs(v));
                }
            }
            _ => {}
        }
        refs
    }
}
