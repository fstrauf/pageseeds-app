//! OpenAI-compatible (and Anthropic) structured extraction with sanitized schemas.
//!
//! Rig's native `Extractor<T>` serializes raw schemars output as tool parameters.
//! Providers reject that for types like `CtrFixPatch` (`anyOf` + `$ref` on
//! `Option<Struct>` → `invalid_function_parameters`).
//!
//! This module talks HTTP directly with:
//! - OpenAI-shaped tools: `tools[].function.parameters = schemars_tool_parameters::<T>()`
//! - Anthropic tools: `tools[].input_schema` (same sanitized object)
//! - JSON-mode fallback when tools are unsupported
//!
//! Shared request/response shapes follow `compat/kimi.rs` patterns (string
//! message content, fence stripping, retries).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rig::schema_sanitize::schemars_tool_parameters;

// ─────────────────────────────────────────────────────────────────────────────
// Shared OpenAI-compatible wire types
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
    tool_choice: Option<Value>,
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
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Deserialize, Clone)]
struct ToolCall {
    function: ToolFunction,
}

#[derive(Debug, Deserialize, Clone)]
struct ToolFunction {
    name: String,
    arguments: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Anthropic Messages API wire types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
struct AnthropicRequest {
    model: String,
    max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public extract entry points
// ─────────────────────────────────────────────────────────────────────────────

const DEFAULT_PREAMBLE: &str = "Extract structured data from the provided text. \
    Always use the submit tool to return your answer. \
    Fill out every field and do not omit any required information.";

const SUBMIT_TOOL_DESC: &str = "Submit the extracted structured data.";

/// OpenAI Chat Completions structured extract with sanitized tool parameters.
pub async fn extract_openai<T>(
    api_key: &str,
    model: &str,
    prompt: &str,
    preamble: &str,
    max_tokens: Option<u64>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    extract_openai_compatible(
        "https://api.openai.com/v1",
        Some(api_key),
        model,
        prompt,
        preamble,
        max_tokens,
        "OpenAI",
    )
    .await
}

/// Ollama OpenAI-compatible structured extract (`{base}/v1/chat/completions`).
pub async fn extract_ollama<T>(
    base_url: &str,
    model: &str,
    prompt: &str,
    preamble: &str,
    max_tokens: Option<u64>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    extract_openai_compatible(
        base_url,
        None,
        model,
        prompt,
        preamble,
        max_tokens,
        "Ollama",
    )
    .await
}

/// Anthropic Messages API structured extract with sanitized `input_schema`.
pub async fn extract_claude<T>(
    api_key: &str,
    model: &str,
    prompt: &str,
    preamble: &str,
    max_tokens: Option<u64>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    let schema_value = schemars_tool_parameters::<T>()?;
    let system = if preamble.is_empty() {
        DEFAULT_PREAMBLE.to_string()
    } else {
        format!("{}\n\n{}", DEFAULT_PREAMBLE, preamble)
    };

    let tool = AnthropicTool {
        name: "submit".to_string(),
        description: SUBMIT_TOOL_DESC.to_string(),
        input_schema: schema_value.clone(),
    };

    let request = AnthropicRequest {
        model: model.to_string(),
        max_tokens: max_tokens.unwrap_or(8192),
        system: Some(system),
        messages: vec![RequestMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
        tools: Some(vec![tool]),
        tool_choice: Some(serde_json::json!({
            "type": "tool",
            "name": "submit"
        })),
    };

    let mut last_error = String::new();
    match send_anthropic_request(api_key, request.clone()).await {
        Ok(response) => {
            if let Some(value) = parse_anthropic_submit::<T>(&response, &mut last_error) {
                return Ok(value);
            }
        }
        Err(e) if is_tools_not_supported(&e) || is_schema_error(&e) => {
            // Fall through to JSON mode for tools-unsupported / blocked tools.
            log::warn!(
                "[openai_compatible_extract] Claude tools path failed ({}); trying JSON mode",
                e
            );
            return extract_json_mode_anthropic::<T>(
                api_key,
                model,
                prompt,
                preamble,
                &schema_value,
                max_tokens,
            )
            .await;
        }
        Err(e) => return Err(e),
    }

    // Retry tool path twice more on parse failures.
    for attempt in 1..3 {
        let response = send_anthropic_request(api_key, request.clone()).await?;
        if let Some(value) = parse_anthropic_submit::<T>(&response, &mut last_error) {
            return Ok(value);
        }
        log::warn!(
            "[openai_compatible_extract] Claude parse retry {} failed: {}",
            attempt + 1,
            last_error
        );
    }

    // Final fallback: JSON in prompt.
    log::info!(
        "[openai_compatible_extract] Claude tool extract exhausted retries; falling back to JSON mode. Last: {}",
        last_error
    );
    extract_json_mode_anthropic::<T>(api_key, model, prompt, preamble, &schema_value, max_tokens)
        .await
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenAI-compatible implementation
// ─────────────────────────────────────────────────────────────────────────────

async fn extract_openai_compatible<T>(
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
    preamble: &str,
    max_tokens: Option<u64>,
    label: &str,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    let schema_value = schemars_tool_parameters::<T>()?;

    let tool = ToolDefinition {
        tool_type: "function".to_string(),
        function: FunctionDefinition {
            name: "submit".to_string(),
            description: SUBMIT_TOOL_DESC.to_string(),
            parameters: schema_value.clone(),
        },
    };

    let user_content = if preamble.is_empty() {
        format!("{}\n\n{}", DEFAULT_PREAMBLE, prompt)
    } else {
        format!("{}\n\n{}\n\n{}", DEFAULT_PREAMBLE, preamble, prompt)
    };

    let tool_request = ChatRequest {
        model: model.to_string(),
        messages: vec![RequestMessage {
            role: "user".to_string(),
            content: user_content,
        }],
        temperature: None,
        max_tokens,
        tools: Some(vec![tool]),
        // Force submit when supported; "auto" is a safe fallback if forced form is rejected.
        tool_choice: Some(serde_json::json!({
            "type": "function",
            "function": { "name": "submit" }
        })),
        response_format: None,
    };

    let mut last_error = String::new();
    match send_openai_request(base_url, api_key, tool_request.clone(), label).await {
        Ok(response) => {
            if let Some(value) = parse_openai_response::<T>(&response, &mut last_error) {
                return Ok(value);
            }
        }
        Err(e) if is_tools_not_supported(&e) => {
            log::info!(
                "[{} extract] Tool calls not supported. Falling back to JSON mode.",
                label
            );
            return extract_json_mode_openai::<T>(
                base_url,
                api_key,
                model,
                prompt,
                preamble,
                &schema_value,
                max_tokens,
                label,
            )
            .await;
        }
        Err(e) => return Err(e),
    }

    for attempt in 1..3 {
        let response = send_openai_request(base_url, api_key, tool_request.clone(), label).await?;
        if let Some(value) = parse_openai_response::<T>(&response, &mut last_error) {
            return Ok(value);
        }
        log::warn!(
            "[{} extract] Parse retry {} failed: {}",
            label,
            attempt + 1,
            last_error
        );
    }

    log::info!(
        "[{} extract] Tool extract exhausted retries; falling back to JSON mode. Last: {}",
        label,
        last_error
    );
    extract_json_mode_openai::<T>(
        base_url,
        api_key,
        model,
        prompt,
        preamble,
        &schema_value,
        max_tokens,
        label,
    )
    .await
}

async fn extract_json_mode_openai<T>(
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
    preamble: &str,
    schema: &Value,
    max_tokens: Option<u64>,
    label: &str,
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

    let user_content = if preamble.is_empty() {
        format!("{}\n\n{}", json_preamble, prompt)
    } else {
        format!("{}\n\n{}\n\n{}", json_preamble, preamble, prompt)
    };

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![RequestMessage {
            role: "user".to_string(),
            content: user_content,
        }],
        temperature: Some(0.1),
        max_tokens,
        tools: None,
        tool_choice: None,
        response_format: Some(ResponseFormat {
            format_type: "json_object".to_string(),
        }),
    };

    let mut last_error = String::new();
    for attempt in 0..3 {
        let response = match send_openai_request(base_url, api_key, request.clone(), label).await {
            Ok(r) => r,
            Err(e) if attempt == 0 && e.contains("response_format") => {
                // Some Ollama models reject response_format — retry without it.
                let mut plain = request.clone();
                plain.response_format = None;
                send_openai_request(base_url, api_key, plain, label).await?
            }
            Err(e) => return Err(e),
        };

        if let Some(content) = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
        {
            let cleaned = strip_json_fences(content.trim());
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
                        log::warn!("[{} extract] {}", label, last_error);
                        continue;
                    }
                }
            }
        }
        last_error = format!(
            "JSON mode attempt {}: no parseable content",
            attempt + 1
        );
        log::warn!("[{} extract] {}", label, last_error);
    }

    Err(format!(
        "{} JSON-mode structured extraction failed after 3 attempts. Last error: {}",
        label, last_error
    ))
}

async fn extract_json_mode_anthropic<T>(
    api_key: &str,
    model: &str,
    prompt: &str,
    preamble: &str,
    schema: &Value,
    max_tokens: Option<u64>,
) -> Result<T, String>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Send + Sync,
{
    let schema_str = serde_json::to_string_pretty(schema)
        .map_err(|e| format!("Failed to serialize schema: {}", e))?;

    let system = format!(
        "You are a structured data extraction assistant. \
         Respond ONLY with a valid JSON object that conforms to the following schema. \
         Do not include markdown code fences, explanations, or any text outside the JSON.\n\n\
         Schema:\n{}\n\n\
         Your response must be a single JSON object. Fill out every field.{}",
        schema_str,
        if preamble.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", preamble)
        }
    );

    let request = AnthropicRequest {
        model: model.to_string(),
        max_tokens: max_tokens.unwrap_or(8192),
        system: Some(system),
        messages: vec![RequestMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
        tools: None,
        tool_choice: None,
    };

    let mut last_error = String::new();
    for attempt in 0..3 {
        let response = send_anthropic_request(api_key, request.clone()).await?;
        let text = response
            .content
            .iter()
            .filter(|b| b.block_type == "text")
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        let cleaned = strip_json_fences(text.trim());
        if !cleaned.is_empty() {
            match serde_json::from_str::<T>(cleaned) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    last_error = format!(
                        "Claude JSON mode parse error (attempt {}): {} | raw: {}",
                        attempt + 1,
                        e,
                        cleaned
                    );
                    log::warn!("[openai_compatible_extract] {}", last_error);
                    continue;
                }
            }
        }
        last_error = format!(
            "Claude JSON mode attempt {}: no parseable content",
            attempt + 1
        );
    }

    Err(format!(
        "Claude JSON-mode structured extraction failed after 3 attempts. Last error: {}",
        last_error
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP + parse helpers
// ─────────────────────────────────────────────────────────────────────────────

fn openai_chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") || base.contains("/v1/") {
        format!("{}/chat/completions", base)
    } else {
        // Ollama default is http://localhost:11434 — OpenAI compat lives under /v1.
        format!("{}/v1/chat/completions", base)
    }
}

async fn send_openai_request(
    base_url: &str,
    api_key: Option<&str>,
    request: ChatRequest,
    label: &str,
) -> Result<ChatResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let url = openai_chat_completions_url(base_url);
    let mut req = client.post(&url).json(&request);
    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    } else {
        // Ollama accepts a dummy key; some reverse proxies require the header.
        req = req.header("Authorization", "Bearer ollama");
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("{} request failed: {}", label, e))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("{} response body error: {}", label, e))?;

    if !status.is_success() {
        return Err(format!("{} HTTP {}: {}", label, status.as_u16(), body));
    }

    serde_json::from_str::<ChatResponse>(&body).map_err(|e| {
        format!(
            "{} response parse error: {} | body: {}",
            label,
            e,
            truncate(&body, 500)
        )
    })
}

async fn send_anthropic_request(
    api_key: &str,
    request: AnthropicRequest,
) -> Result<AnthropicResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Claude request failed: {}", e))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Claude response body error: {}", e))?;

    if !status.is_success() {
        return Err(format!("Claude HTTP {}: {}", status.as_u16(), body));
    }

    serde_json::from_str::<AnthropicResponse>(&body).map_err(|e| {
        format!(
            "Claude response parse error: {} | body: {}",
            e,
            truncate(&body, 500)
        )
    })
}

fn parse_openai_response<T>(response: &ChatResponse, last_error: &mut String) -> Option<T>
where
    T: for<'a> Deserialize<'a>,
{
    let choice = response.choices.first()?;

    if let Some(tool_call) = choice
        .message
        .tool_calls
        .iter()
        .find(|tc| tc.function.name == "submit")
    {
        match serde_json::from_str::<T>(&tool_call.function.arguments) {
            Ok(value) => return Some(value),
            Err(e) => {
                *last_error = format!(
                    "Tool call parse error: {} | raw: {}",
                    e, tool_call.function.arguments
                );
            }
        }
    }

    if let Some(content) = &choice.message.content {
        let cleaned = strip_json_fences(content.trim());
        if !cleaned.is_empty() {
            match serde_json::from_str::<T>(cleaned) {
                Ok(value) => return Some(value),
                Err(e) => {
                    *last_error = format!("Content parse error: {} | raw: {}", e, cleaned);
                }
            }
        }
    }

    if last_error.is_empty() {
        *last_error =
            "Response contained neither submit tool_calls nor parseable content".to_string();
    }
    None
}

fn parse_anthropic_submit<T>(response: &AnthropicResponse, last_error: &mut String) -> Option<T>
where
    T: for<'a> Deserialize<'a>,
{
    for block in &response.content {
        if block.block_type == "tool_use" && block.name.as_deref() == Some("submit") {
            if let Some(input) = &block.input {
                match serde_json::from_value::<T>(input.clone()) {
                    Ok(value) => return Some(value),
                    Err(e) => {
                        *last_error = format!(
                            "Claude tool_use parse error: {} | raw: {}",
                            e, input
                        );
                    }
                }
            }
        }
    }

    // Fallback: text block may contain JSON.
    let text = response
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = strip_json_fences(text.trim());
    if !cleaned.is_empty() {
        match serde_json::from_str::<T>(cleaned) {
            Ok(value) => return Some(value),
            Err(e) => {
                *last_error = format!("Claude text parse error: {} | raw: {}", e, cleaned);
            }
        }
    }

    if last_error.is_empty() {
        *last_error =
            "Claude response contained neither submit tool_use nor parseable text".to_string();
    }
    None
}

fn is_tools_not_supported(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("tools_not_supported")
        || lower.contains("does not support native tool")
        || lower.contains("does not support tools")
        || lower.contains("tool use is not supported")
        || lower.contains("tool_choice")
        || (lower.contains("tool") && lower.contains("not support"))
}

fn is_schema_error(error: &str) -> bool {
    error.contains("invalid_function_parameters")
        || error.contains("Invalid schema for function")
        || error.contains("input_schema")
}

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

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ctr::CtrFixPatch;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};
    use wiremock::matchers::body_partial_json;

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct TestOutput {
        pub name: String,
        pub count: i32,
    }

    /// Matcher: tools[0].function.parameters has no $ref / anyOf / $defs.
    struct SanitizedToolParameters;

    impl wiremock::Match for SanitizedToolParameters {
        fn matches(&self, request: &Request) -> bool {
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            let params = body
                .pointer("/tools/0/function/parameters")
                .cloned()
                .unwrap_or(Value::Null);
            let s = params.to_string();
            !s.contains("\"$ref\"")
                && !s.contains("\"anyOf\"")
                && !s.contains("\"$defs\"")
                && params.get("type").and_then(|t| t.as_str()) == Some("object")
        }
    }

    #[tokio::test]
    async fn openai_compatible_extract_sends_sanitized_tool_params() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(SanitizedToolParameters)
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "submit",
                                "arguments": "{\"name\":\"ok\",\"count\":7}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            })))
            .mount(&mock_server)
            .await;

        let result: TestOutput = extract_openai_compatible(
            &mock_server.uri(),
            Some("test-key"),
            "test-model",
            "extract",
            "",
            None,
            "MockOpenAI",
        )
        .await
        .unwrap();

        assert_eq!(result.name, "ok");
        assert_eq!(result.count, 7);
    }

    #[tokio::test]
    async fn openai_compatible_ctr_fix_patch_params_have_no_ref() {
        // Pure schema check via the same helper the wire path uses.
        let params = schemars_tool_parameters::<CtrFixPatch>().unwrap();
        let s = params.to_string();
        assert!(!s.contains("\"$ref\""), "params still has $ref: {}", s);
        assert!(!s.contains("\"anyOf\""), "params still has anyOf: {}", s);
        assert!(!s.contains("\"$defs\""), "params still has $defs: {}", s);
        assert_eq!(params.get("type").and_then(|t| t.as_str()), Some("object"));
    }

    #[tokio::test]
    async fn openai_json_mode_fallback_on_tools_not_supported() {
        let mock_server = MockServer::start().await;

        // First request (with tools) fails as tools-not-supported.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(serde_json::json!({
                "tools": [{"type": "function"}]
            })))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"error":{"message":"does not support tools","code":"tools_not_supported"}}"#,
            ))
            .mount(&mock_server)
            .await;

        // JSON mode (no tools) succeeds.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-json",
                "object": "chat.completion",
                "created": 1,
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "{\"name\":\"from-json\",\"count\":3}"
                    },
                    "finish_reason": "stop"
                }]
            })))
            .mount(&mock_server)
            .await;

        let result: TestOutput = extract_openai_compatible(
            &mock_server.uri(),
            Some("key"),
            "test-model",
            "extract",
            "",
            None,
            "MockOpenAI",
        )
        .await
        .unwrap();

        assert_eq!(result.name, "from-json");
        assert_eq!(result.count, 3);
    }

    #[test]
    fn openai_chat_completions_url_handles_v1_and_bare() {
        assert_eq!(
            openai_chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url("http://localhost:11434"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url("http://localhost:11434/v1/"),
            "http://localhost:11434/v1/chat/completions"
        );
    }
}
