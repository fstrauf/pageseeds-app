//! Native Kimi CLI provider — spawns `kimi -p` directly via tokio::process.
//!
//! Drop-in replacement for the Python HTTP bridge (`kimi-acp-openai-bridge`).
//!
//! Tested against `kimi-code` v0.23.0. Key CLI facts:
//! - `-p <prompt>` runs non-interactively and exits (no `--print` flag exists)
//! - `--output-format stream-json` emits JSONL (one JSON object per line)
//! - Tool calls are auto-approved in `-p` mode (no `--yolo` needed; in fact
//!   `--yolo` cannot be combined with `-p`)
//! - There is no `--work-dir` flag; the process CWD is the workspace root.
//!   We set it via `Command::current_dir()`.
//! - There is no `--final-message-only` or `--no-thinking` flag.
//!
//! JSONL output format (each line is one JSON object):
//! ```jsonl
//! {"role":"assistant","tool_calls":[{"type":"function","id":"...","function":{...}}]}
//! {"role":"tool","tool_call_id":"...","content":"..."}
//! {"role":"assistant","content":"final response text"}
//! {"role":"meta","type":"session.resume_hint","session_id":"...","content":"..."}
//! ```
//! We extract `content` from `assistant` role lines and ignore everything else.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::rig::provider::AgentResponse;

/// Default Kimi binary name (resolved via PATH).
const KIMI_BINARY: &str = "kimi";

/// Timeout for all Kimi CLI calls (seconds).
///
/// Previously split into STATELESS (300s) and CONTENT (600s) based on the
/// `"direct"`/`"acp"` backend_preference — a transport-routing parameter from
/// the bridge era that was never the right signal for timeout selection.
/// A single generous ceiling eliminates the entire class of "wrong timeout"
/// bugs: no task type can fail because someone forgot to label it `"acp"`.
///
/// 600s is conservative — production evidence shows content generation at
/// 160-170s, fix tasks at 5-9 min, and stateless analysis at 10-60s. If a
/// Kimi CLI call hasn't produced output in 10 minutes, something is genuinely
/// stuck.
const TIMEOUT_SECS: u64 = 600;

/// Maximum structured-extraction retry attempts.
const MAX_EXTRACTION_RETRIES: usize = 3;

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Check whether the `kimi` binary is available on PATH.
pub fn is_kimi_available() -> bool {
    std::process::Command::new(KIMI_BINARY)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a prompt through `kimi -p` and return the concatenated assistant content.
///
/// Flattens `preamble + prompt` into a single string because prompt mode is
/// single-turn (the model receives the full context as one user message).
///
/// # Errors
///
/// Returns `Err(String)` when:
/// - The `kimi` binary is not on PATH.
/// - The process times out ([`TIMEOUT_SECS`]).
/// - Kimi exits with a non-zero code.
/// - The output is empty.
/// - The step limit is reached.
pub async fn run_prompt(
    prompt: &str,
    preamble: Option<&str>,
    work_dir: &str,
) -> Result<AgentResponse, String> {
    let full_prompt = build_prompt_text(preamble, prompt);

    log::info!(
        "[kimi_cli] prompt_bytes={} timeout={}s work_dir={}",
        full_prompt.len(),
        TIMEOUT_SECS,
        work_dir,
    );

    let raw = run_kimi_subprocess(&full_prompt, work_dir, TIMEOUT_SECS).await?;

    let lower = raw.to_lowercase();
    if lower.contains("max number of steps reached") || lower.contains("max_steps_reached") {
        return Err("Kimi reached the maximum number of steps".to_string());
    }

    let content = raw.trim().to_string();
    if content.is_empty() {
        return Err("Kimi CLI returned empty output".to_string());
    }

    log::info!("[kimi_cli] response_chars={}", content.len());

    Ok(AgentResponse::from_content(content))
}

/// Extract structured JSON data from a prompt using `kimi -p`.
///
/// Injects the JSON schema into the prompt and instructs the model to respond
/// with valid JSON only (no markdown fences, no prose). This is equivalent to
/// the bridge's JSON-mode extraction path but without the HTTP round-trip.
///
/// Retries up to [`MAX_EXTRACTION_RETRIES`] times on parse failures.
pub async fn extract_structured<T>(
    prompt: &str,
    preamble: Option<&str>,
    schema: &serde_json::Value,
    work_dir: &str,
) -> Result<T, String>
where
    T: JsonSchema + DeserializeOwned + Send,
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

    let full_prompt = if let Some(p) = preamble {
        format!("{}\n\n{}", p, prompt)
    } else {
        prompt.to_string()
    };

    let mut last_error = String::new();
    for attempt in 0..MAX_EXTRACTION_RETRIES {
        let result = run_prompt(&full_prompt, Some(&json_preamble), work_dir).await?;

        let cleaned = strip_json_fences(&result.content);
        if cleaned.is_empty() {
            last_error = format!(
                "Attempt {}: response contained no parseable content",
                attempt + 1
            );
            log::warn!("[kimi_cli::extract_structured] {}", last_error);
            continue;
        }

        match serde_json::from_str::<T>(cleaned) {
            Ok(value) => {
                log::info!(
                    "[kimi_cli::extract_structured] parsed successfully on attempt {}",
                    attempt + 1
                );
                return Ok(value);
            }
            Err(e) => {
                let preview: String = cleaned.chars().take(500).collect();
                last_error = format!(
                    "JSON parse error (attempt {}): {} | raw: {}",
                    attempt + 1,
                    e,
                    preview
                );
                log::warn!("[kimi_cli::extract_structured] {}", last_error);
            }
        }
    }

    Err(format!(
        "Kimi CLI structured extraction failed after {} attempts. Last error: {}",
        MAX_EXTRACTION_RETRIES, last_error
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: subprocess execution
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn `kimi -p <prompt> --output-format stream-json` and parse the JSONL output.
///
/// The process CWD is set to `work_dir` so the agent's file tools operate
/// in-scope (there is no `--work-dir` CLI flag in kimi-code 0.23.0).
///
/// `kill_on_drop(true)` ensures the child is terminated if the future is
/// dropped (e.g. by a timeout or task cancellation).
async fn run_kimi_subprocess(
    prompt: &str,
    work_dir: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut cmd = Command::new(KIMI_BINARY);
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let start = std::time::Instant::now();
    log::info!(
        "[kimi_cli] spawning kimi -p (timeout={}s, prompt_bytes={}, work_dir={})",
        timeout_secs,
        prompt.len(),
        work_dir,
    );

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "Kimi CLI binary '{}' not found. Ensure kimi is installed and in PATH.",
                KIMI_BINARY
            ));
        }
        Err(e) => {
            return Err(format!("Failed to spawn kimi process: {}", e));
        }
    };

    let result = match timeout(
        Duration::from_secs(timeout_secs),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(format!("Kimi process I/O error: {}", e));
        }
        Err(_) => {
            return Err(format!(
                "Kimi CLI timed out after {}s",
                timeout_secs
            ));
        }
    };

    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    log::info!(
        "[kimi_cli] process finished exit_code={:?} duration={:?} stdout_lines={} stderr_chars={}",
        result.status.code(),
        elapsed,
        stdout.lines().count(),
        stderr.len(),
    );

    // On non-zero exit, check stderr for the error.
    if !result.status.success() {
        let err_preview: String = stderr.trim().chars().take(500).collect();
        match result.status.code() {
            Some(75) => {
                return Err(format!(
                    "Kimi CLI transient failure (exit 75, retryable): {}",
                    err_preview
                ));
            }
            Some(code) => {
                return Err(format!(
                    "Kimi CLI exited with code {}: {}",
                    code, err_preview
                ));
            }
            None => {
                return Err("Kimi CLI process was killed (signal)".to_string());
            }
        }
    }

    // Parse JSONL: extract assistant content, skip meta/tool lines.
    let content = parse_stream_json(&stdout);
    if content.is_empty() {
        // Fallback: if JSONL parsing yielded nothing (unexpected format change),
        // return the raw stdout so the caller sees what happened.
        let raw = stdout.trim();
        if raw.is_empty() {
            return Err(format!(
                "Kimi CLI produced no output. stderr: {}",
                stderr.trim()
            ));
        }
        log::warn!(
            "[kimi_cli] JSONL parse yielded empty content, returning raw stdout ({} bytes)",
            raw.len()
        );
        return Ok(raw.to_string());
    }

    Ok(content)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: JSONL parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Parse `stream-json` JSONL output and concatenate all `assistant` content.
///
/// Each line is a JSON object with a `role` field:
/// - `"assistant"` with `content` → append content (skip if content is null/absent,
///   which means the line only carries `tool_calls`)
/// - `"tool"` → skip (tool execution results)
/// - `"meta"` → skip (session hints, metadata)
fn parse_stream_json(output: &str) -> String {
    let mut content = String::new();

    for (i, line) in output.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(obj) => {
                let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "assistant" {
                    if let Some(text) = obj.get("content").and_then(|c| c.as_str()) {
                        if !text.is_empty() {
                            content.push_str(text);
                        }
                    }
                }
            }
            Err(e) => {
                // Log but don't fail — some lines might be non-JSON (e.g. progress output)
                let preview: String = line.chars().take(200).collect();
                log::debug!(
                    "[kimi_cli] skipping unparseable line {}: {} | {}",
                    i,
                    e,
                    preview
                );
            }
        }
    }

    content
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: text helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Flatten preamble + prompt into a single text block.
///
/// Prompt mode is single-turn: the model receives one user message. We prepend
/// the preamble as a system-instruction prefix so the model distinguishes it
/// from the user content.
fn build_prompt_text(preamble: Option<&str>, prompt: &str) -> String {
    match preamble {
        Some(p) if !p.is_empty() => format!("[System instructions]\n\n{}\n\n---\n\n{}", p, prompt),
        _ => prompt.to_string(),
    }
}

/// Strip ```` ```json ```` or ```` ``` ```` fences from a model response.
fn strip_json_fences(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```")
            .map(|s| s.trim())
            .unwrap_or(trimmed)
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```")
            .map(|s| s.trim())
            .unwrap_or(trimmed)
    } else {
        trimmed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_text_with_preamble() {
        let result = build_prompt_text(Some("Be concise."), "Hello");
        assert!(result.contains("[System instructions]"));
        assert!(result.contains("Be concise."));
        assert!(result.contains("Hello"));
        assert!(result.contains("---"));
    }

    #[test]
    fn test_build_prompt_text_without_preamble() {
        let result = build_prompt_text(None, "Hello");
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_build_prompt_text_empty_preamble() {
        let result = build_prompt_text(Some(""), "Hello");
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_strip_json_fences_json_prefix() {
        assert_eq!(strip_json_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
    }

    #[test]
    fn test_strip_json_fences_bare_prefix() {
        assert_eq!(strip_json_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
    }

    #[test]
    fn test_strip_json_fences_no_fences() {
        assert_eq!(strip_json_fences("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn test_strip_json_fences_with_whitespace() {
        assert_eq!(strip_json_fences("  ```json\n{\"a\":1}\n```  "), "{\"a\":1}");
    }

    #[test]
    fn test_strip_json_fences_empty() {
        assert_eq!(strip_json_fences(""), "");
    }

    #[test]
    fn test_parse_stream_json_simple_response() {
        let jsonl = r#"{"role":"assistant","content":"Hello world."}
{"role":"meta","type":"session.resume_hint","session_id":"abc","content":"To resume: kimi -r abc"}"#;
        assert_eq!(parse_stream_json(jsonl), "Hello world.");
    }

    #[test]
    fn test_parse_stream_json_with_tool_calls() {
        let jsonl = r#"{"role":"assistant","tool_calls":[{"type":"function","id":"tc1","function":{"name":"Read","arguments":"{\"path\":\"test.md\"}"}}]}
{"role":"tool","tool_call_id":"tc1","content":"file contents here"}
{"role":"assistant","content":"The file contains: file contents here"}
{"role":"meta","type":"session.resume_hint","session_id":"def","content":"To resume: kimi -r def"}"#;
        assert_eq!(
            parse_stream_json(jsonl),
            "The file contains: file contents here"
        );
    }

    #[test]
    fn test_parse_stream_json_multi_chunk_assistant() {
        // If the model streams a long response across multiple assistant lines
        let jsonl = r#"{"role":"assistant","content":"Part 1. "}
{"role":"assistant","content":"Part 2."}
{"role":"meta","type":"session.resume_hint","content":"ignore"}"#;
        assert_eq!(parse_stream_json(jsonl), "Part 1. Part 2.");
    }

    #[test]
    fn test_parse_stream_json_skips_null_content() {
        // Tool-call-only assistant lines have content: null
        let jsonl = r#"{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"tc1","function":{"name":"Write","arguments":"{}"}}]}
{"role":"tool","tool_call_id":"tc1","content":"Wrote 10 bytes"}
{"role":"assistant","content":"Done writing the file."}"#;
        assert_eq!(parse_stream_json(jsonl), "Done writing the file.");
    }

    #[test]
    fn test_parse_stream_json_empty_input() {
        assert_eq!(parse_stream_json(""), "");
    }

    #[test]
    fn test_parse_stream_json_skips_blank_lines() {
        let jsonl = "\n{\"role\":\"assistant\",\"content\":\"Hi\"}\n\n";
        assert_eq!(parse_stream_json(jsonl), "Hi");
    }

    #[test]
    fn test_parse_stream_json_tolerates_non_json() {
        // Non-JSON lines (debug output, progress bars) are skipped
        let jsonl = r#"some progress text
{"role":"assistant","content":"Response"}
another non-json line"#;
        assert_eq!(parse_stream_json(jsonl), "Response");
    }

    #[test]
    fn test_timeout_is_generous() {
        assert!(TIMEOUT_SECS >= 600, "timeout must cover 5-9 min fix tasks");
    }

    #[test]
    fn test_max_extraction_retries() {
        assert_eq!(MAX_EXTRACTION_RETRIES, 3);
    }
}
