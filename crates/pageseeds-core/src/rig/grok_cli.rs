//! Native Grok CLI provider — spawns `grok -p` directly via tokio::process.
//!
//! Parallel to [`crate::rig::kimi_cli`]: agentic file tools run inside the CLI
//! process with CWD set to the project root.
//!
//! Tested against Grok Build CLI. Key facts:
//! - `-p` / `--single <prompt>` runs non-interactively and exits
//! - `--always-approve` auto-approves tool use (required for unattended runs)
//! - `--cwd <path>` sets working directory for file tools
//! - `--output-format plain` prints the final assistant text to stdout
//! - `--output-format streaming-json` emits JSONL (`type: text` / `thought` / `end`)
//!
//! We use **plain** by default for a stable single-block response.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::rig::provider::AgentResponse;

/// Default Grok binary name (resolved via PATH).
const GROK_BINARY: &str = "grok";

/// Timeout for all Grok CLI calls (seconds). Same ceiling as Kimi CLI.
const TIMEOUT_SECS: u64 = 600;

/// Maximum structured-extraction retry attempts.
const MAX_EXTRACTION_RETRIES: usize = 3;

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Check whether the `grok` binary is available on PATH.
pub fn is_grok_available() -> bool {
    std::process::Command::new(GROK_BINARY)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a prompt through `grok -p` and return the assistant text.
///
/// Flattens `preamble + prompt` into a single string (single-turn headless mode).
pub async fn run_prompt(
    prompt: &str,
    preamble: Option<&str>,
    work_dir: &str,
) -> Result<AgentResponse, String> {
    let full_prompt = build_prompt_text(preamble, prompt);

    log::info!(
        "[grok_cli] prompt_bytes={} timeout={}s work_dir={}",
        full_prompt.len(),
        TIMEOUT_SECS,
        work_dir,
    );

    let raw = run_grok_subprocess(&full_prompt, work_dir, TIMEOUT_SECS).await?;

    let content = raw.trim().to_string();
    if content.is_empty() {
        return Err("Grok CLI returned empty output".to_string());
    }

    log::info!("[grok_cli] response_chars={}", content.len());

    Ok(AgentResponse::from_content(content))
}

/// Extract structured JSON data from a prompt using `grok -p`.
///
/// Injects the JSON schema into the prompt and instructs the model to respond
/// with valid JSON only (same pattern as `kimi_cli::extract_structured`).
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
                "Grok CLI returned empty JSON body on attempt {}",
                attempt + 1
            );
            log::warn!("[grok_cli::extract_structured] {}", last_error);
            continue;
        }

        match serde_json::from_str::<T>(cleaned) {
            Ok(val) => {
                if attempt > 0 {
                    log::info!(
                        "[grok_cli::extract_structured] parsed successfully on attempt {}",
                        attempt + 1
                    );
                }
                return Ok(val);
            }
            Err(e) => {
                last_error = format!(
                    "Failed to parse Grok CLI JSON on attempt {}: {} | body_preview={}",
                    attempt + 1,
                    e,
                    cleaned.chars().take(200).collect::<String>()
                );
                log::warn!("[grok_cli::extract_structured] {}", last_error);
            }
        }
    }

    Err(format!(
        "Grok CLI structured extraction failed after {} attempts: {}",
        MAX_EXTRACTION_RETRIES, last_error
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Subprocess
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn `grok -p <prompt> --always-approve --output-format plain --cwd <work_dir>`.
async fn run_grok_subprocess(
    prompt: &str,
    work_dir: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut cmd = Command::new(GROK_BINARY);
    cmd.arg("-p")
        .arg(prompt)
        .arg("--always-approve")
        .arg("--output-format")
        .arg("plain")
        .arg("--cwd")
        .arg(work_dir)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let start = std::time::Instant::now();
    log::info!(
        "[grok_cli] spawning grok -p (timeout={}s, prompt_bytes={}, work_dir={})",
        timeout_secs,
        prompt.len(),
        work_dir,
    );

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "Grok CLI binary '{}' not found. Ensure grok is installed and in PATH.",
                GROK_BINARY
            ));
        }
        Err(e) => {
            return Err(format!("Failed to spawn grok process: {}", e));
        }
    };

    let result = match timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(format!("Grok process I/O error: {}", e));
        }
        Err(_) => {
            return Err(format!("Grok CLI timed out after {}s", timeout_secs));
        }
    };

    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    log::info!(
        "[grok_cli] process finished exit_code={:?} duration={:?} stdout_chars={} stderr_chars={}",
        result.status.code(),
        elapsed,
        stdout.len(),
        stderr.len(),
    );

    if !result.status.success() {
        let err_preview: String = stderr.trim().chars().take(500).collect();
        match result.status.code() {
            Some(code) => {
                return Err(format!(
                    "Grok CLI exited with code {}: {}",
                    code, err_preview
                ));
            }
            None => {
                return Err("Grok CLI process was killed (signal)".to_string());
            }
        }
    }

    let content = stdout.trim();
    if content.is_empty() {
        return Err(format!(
            "Grok CLI produced no output. stderr: {}",
            stderr.trim()
        ));
    }

    Ok(content.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Text helpers
// ─────────────────────────────────────────────────────────────────────────────

fn build_prompt_text(preamble: Option<&str>, prompt: &str) -> String {
    match preamble {
        Some(p) if !p.is_empty() => format!("[System instructions]\n\n{}\n\n---\n\n{}", p, prompt),
        _ => prompt.to_string(),
    }
}

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
    }

    #[test]
    fn test_build_prompt_text_without_preamble() {
        assert_eq!(build_prompt_text(None, "Hello"), "Hello");
    }

    #[test]
    fn test_strip_json_fences() {
        assert_eq!(strip_json_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_json_fences("{\"a\":1}"), "{\"a\":1}");
    }
}
