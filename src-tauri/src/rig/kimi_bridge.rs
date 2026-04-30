//! Typed models and helpers for the Kimi ACP OpenAI bridge.
//!
//! This module provides structured deserialization for the bridge `/health`
//! endpoint and for structured error bodies returned by the bridge.

use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// Data models
// ─────────────────────────────────────────────────────────────────────────────

/// Full health response from the Kimi bridge `/health` endpoint.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct KimiBridgeHealth {
    #[serde(default)]
    pub status: String,
    pub kimi_available: bool,
    #[serde(default)]
    pub bridge_version: String,
    pub kimi_cli_version: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub backends: HashMap<String, KimiBackendCapabilities>,
    #[serde(default)]
    pub limits: KimiBridgeLimits,
}

/// Capability flags for a single bridge backend (e.g. `direct` or `acp`).
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct KimiBackendCapabilities {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub tool_calls: bool,
    #[serde(default)]
    pub json_mode: bool,
    #[serde(default)]
    pub file_io: bool,
}

/// Hard limits advertised by the bridge.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct KimiBridgeLimits {
    #[serde(default)]
    pub max_prompt_bytes_direct: usize,
    #[serde(default)]
    pub max_prompt_bytes_acp: usize,
    #[serde(default)]
    pub max_concurrent_requests: usize,
}

/// Structured error returned by the bridge in the `error` field.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct KimiBridgeError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    pub backend: Option<String>,
    pub request_id: Option<String>,
    pub phase: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// Wrapper around the bridge error body (the bridge nests the error object).
#[derive(Debug, Clone, Deserialize, PartialEq)]
struct BridgeErrorEnvelope {
    error: KimiBridgeError,
}

// ─────────────────────────────────────────────────────────────────────────────
// Health check
// ─────────────────────────────────────────────────────────────────────────────

/// Fetch and parse the Kimi bridge `/health` endpoint.
///
/// Uses a 2-second timeout to avoid blocking the executor.
pub async fn get_kimi_bridge_health(base_url: &str) -> Result<KimiBridgeHealth, String> {
    let health_url = base_url.trim_end_matches("/v1").to_string() + "/health";
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to build HTTP client for bridge health: {}", e))?;

    let resp = client
        .get(&health_url)
        .send()
        .await
        .map_err(|e| format!("Kimi bridge health request failed: {}", e))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Kimi bridge health body read failed: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "Kimi bridge health returned HTTP {}: {}",
            status, body
        ));
    }

    let health: KimiBridgeHealth = serde_json::from_str(&body).map_err(|e| {
        format!(
            "Kimi bridge health response was not valid JSON ({}). Body: {}",
            e, body
        )
    })?;

    log::info!(
        "[kimi_bridge] health — version={}, kimi_available={}, backends={:?}, limits={:?}",
        health.bridge_version,
        health.kimi_available,
        health.backends.keys().collect::<Vec<_>>(),
        health.limits,
    );

    Ok(health)
}

// ─────────────────────────────────────────────────────────────────────────────
// Error parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt to parse a structured Kimi bridge error from an HTTP response body.
///
/// Returns `None` if the body is not a recognised bridge error envelope
/// (e.g. plain HTML from a reverse proxy).
pub fn parse_bridge_error(body: &str) -> Option<KimiBridgeError> {
    let envelope: BridgeErrorEnvelope = serde_json::from_str(body).ok()?;
    Some(envelope.error)
}

/// Build a user-facing error message from a parsed bridge error.
///
/// The message includes the `request_id` when available and gives an
/// actionable hint for known error codes.
pub fn format_bridge_error(err: &KimiBridgeError) -> String {
    let req_id = err
        .request_id
        .as_deref()
        .unwrap_or("unknown");
    let backend = err
        .backend
        .as_deref()
        .unwrap_or("unknown");

    let hint = match err.code.as_str() {
        "prompt_too_large" => {
            "Split or trim the prompt before retrying."
        }
        "tools_not_supported" => {
            "Restart the bridge with --backend acp (or set KIMI_BACKEND=acp) and try again."
        }
        "kimi_not_found" | "bridge_unhealthy" | "backend_unavailable" => {
            "Check bridge setup and ensure Kimi CLI is installed and reachable."
        }
        _ => {
            if err.retryable {
                "This error may be transient — retrying is recommended."
            } else {
                "Review the error details and retry with different parameters."
            }
        }
    };

    format!(
        "Kimi bridge request {} failed: {} for {} backend. {} {}",
        req_id, err.code, backend, err.message, hint
    )
}

/// Determine whether a bridge error should be retried.
///
/// Non-retryable codes are treated as hard failures:
/// - `prompt_too_large`
/// - `tools_not_supported`
/// - `kimi_not_found`
/// - `bridge_unhealthy`
/// - `backend_unavailable`
pub fn is_bridge_error_retryable(err: &KimiBridgeError) -> bool {
    match err.code.as_str() {
        "prompt_too_large"
        | "tools_not_supported"
        | "kimi_not_found"
        | "bridge_unhealthy"
        | "backend_unavailable" => false,
        _ => err.retryable,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rich_health() {
        let json = r#"{
            "status": "healthy",
            "kimi_available": true,
            "bridge_version": "1.2.3",
            "kimi_cli_version": "0.9.1",
            "models": ["kimi-k2.5"],
            "backends": {
                "direct": {"available": true, "tool_calls": false, "json_mode": true, "file_io": false},
                "acp": {"available": true, "tool_calls": true, "json_mode": true, "file_io": true}
            },
            "limits": {
                "max_prompt_bytes_direct": 20000,
                "max_prompt_bytes_acp": 20000,
                "max_concurrent_requests": 4
            }
        }"#;

        let health: KimiBridgeHealth = serde_json::from_str(json).unwrap();
        assert_eq!(health.status, "healthy");
        assert!(health.kimi_available);
        assert_eq!(health.bridge_version, "1.2.3");
        assert_eq!(health.kimi_cli_version, Some("0.9.1".to_string()));
        assert_eq!(health.models, vec!["kimi-k2.5"]);
        assert_eq!(health.backends.len(), 2);
        assert_eq!(health.limits.max_prompt_bytes_direct, 20000);
        assert_eq!(health.limits.max_prompt_bytes_acp, 20000);
        assert_eq!(health.limits.max_concurrent_requests, 4);
    }

    #[test]
    fn test_parse_health_minimal() {
        let json = r#"{
            "status": "ok",
            "kimi_available": false,
            "bridge_version": "1.0.0",
            "models": [],
            "backends": {},
            "limits": {
                "max_prompt_bytes_direct": 20000,
                "max_prompt_bytes_acp": 20000,
                "max_concurrent_requests": 2
            }
        }"#;

        let health: KimiBridgeHealth = serde_json::from_str(json).unwrap();
        assert!(!health.kimi_available);
        assert_eq!(health.kimi_cli_version, None);
        assert!(health.backends.is_empty());
    }

    #[test]
    fn test_parse_prompt_too_large() {
        let json = r#"{"error": {"code": "prompt_too_large", "message": "Prompt exceeds 20000 bytes", "retryable": false, "backend": "direct", "request_id": "req-123", "phase": "validation"}}"#;
        let err = parse_bridge_error(json).unwrap();
        assert_eq!(err.code, "prompt_too_large");
        assert!(!err.retryable);
        assert_eq!(err.request_id, Some("req-123".to_string()));
        assert!(!is_bridge_error_retryable(&err));
    }

    #[test]
    fn test_parse_tools_not_supported() {
        let json = r#"{"error": {"code": "tools_not_supported", "message": "Direct mode does not support tool calls", "retryable": false, "backend": "direct", "request_id": "req-456"}}"#;
        let err = parse_bridge_error(json).unwrap();
        assert_eq!(err.code, "tools_not_supported");
        assert!(!is_bridge_error_retryable(&err));
    }

    #[test]
    fn test_parse_backend_empty_response() {
        let json = r#"{"error": {"code": "backend_empty_response", "message": "Empty response from Kimi", "retryable": true, "request_id": "req-789"}}"#;
        let err = parse_bridge_error(json).unwrap();
        assert_eq!(err.code, "backend_empty_response");
        assert!(is_bridge_error_retryable(&err));
    }

    #[test]
    fn test_parse_retryable_error() {
        let json = r#"{"error": {"code": "rate_limit_exceeded", "message": "Too many requests", "retryable": true}}"#;
        let err = parse_bridge_error(json).unwrap();
        assert!(is_bridge_error_retryable(&err));
    }

    #[test]
    fn test_parse_non_bridge_body_returns_none() {
        assert!(parse_bridge_error("<html>nginx error</html>").is_none());
        assert!(parse_bridge_error("plain text error").is_none());
        assert!(parse_bridge_error("{}").is_none());
    }

    #[test]
    fn test_format_bridge_error_includes_request_id() {
        let err = KimiBridgeError {
            code: "prompt_too_large".to_string(),
            message: "too big".to_string(),
            retryable: false,
            backend: Some("direct".to_string()),
            request_id: Some("chatcmpl-abc".to_string()),
            phase: None,
            details: None,
        };
        let msg = format_bridge_error(&err);
        assert!(msg.contains("chatcmpl-abc"));
        assert!(msg.contains("prompt_too_large"));
        assert!(msg.contains("direct"));
        assert!(msg.contains("Split or trim"));
    }

    #[test]
    fn test_is_bridge_error_retryable_hard_codes() {
        for code in ["prompt_too_large", "tools_not_supported", "kimi_not_found", "bridge_unhealthy", "backend_unavailable"] {
            let err = KimiBridgeError {
                code: code.to_string(),
                message: "x".to_string(),
                retryable: true, // even if the flag says retryable, hard list wins
                backend: None,
                request_id: None,
                phase: None,
                details: None,
            };
            assert!(!is_bridge_error_retryable(&err), "{} should not be retryable", code);
        }
    }
}
