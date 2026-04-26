/// Normalizer — parses raw agent text output into structured JSON artifacts.
///
/// Extraction strategy (attempted in order):
/// 1. Fenced code block: ```json ... ``` (most reliable — agents produce these)
/// 2. Bare JSON object or array at the start/end of the text
/// 3. First line that is valid JSON
/// 4. None — returns raw text only

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── Output type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedArtifact {
    pub raw_output: String,
    /// Structured data extracted from the raw output, if any.
    pub json_artifact: Option<Value>,
    /// How the JSON was extracted (or "none").
    pub extraction_method: String,
    pub success: bool,
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn normalize_agent_output(raw: &str) -> NormalizedArtifact {
    // 1. JSON fenced code block
    if let Some(json) = extract_fenced_json(raw) {
        return NormalizedArtifact {
            raw_output: raw.to_string(),
            json_artifact: Some(json),
            extraction_method: "json_block".to_string(),
            success: true,
        };
    }

    // 2. Bare JSON at the outer level of the text
    if let Some(json) = extract_bare_json(raw) {
        return NormalizedArtifact {
            raw_output: raw.to_string(),
            json_artifact: Some(json),
            extraction_method: "bare_json".to_string(),
            success: true,
        };
    }

    // 3. First line that parses as JSON
    if let Some(json) = extract_first_json_line(raw) {
        return NormalizedArtifact {
            raw_output: raw.to_string(),
            json_artifact: Some(json),
            extraction_method: "json_line".to_string(),
            success: true,
        };
    }

    // 4. No JSON found
    NormalizedArtifact {
        raw_output: raw.to_string(),
        json_artifact: None,
        extraction_method: "none".to_string(),
        success: false,
    }
}

// ─── Extraction helpers ───────────────────────────────────────────────────────

/// Extract JSON from the first ```json ... ``` (or ``` ... ```) fenced block.
fn extract_fenced_json(text: &str) -> Option<Value> {
    // Match ```json or ``` followed by optional whitespace/newline
    let patterns = ["```json\n", "```json\r\n", "```JSON\n", "```\n"];
    for pat in &patterns {
        if let Some(start) = text.find(pat) {
            let after_open = start + pat.len();
            let rest = &text[after_open..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Try to parse a JSON object `{...}` or array `[...]` from the full text
/// (after stripping surrounding whitespace and prose).
fn extract_bare_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();

    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return serde_json::from_str::<Value>(trimmed).ok();
    }

    // Try to find the first `{` or `[` and last matching `}` or `]`
    for (start_ch, end_ch) in [pair('{', '}'), pair('[', ']')] {
        if let Some(start_idx) = trimmed.find(start_ch) {
            // Walk from the end to find the last closing bracket
            if let Some(end_idx) = trimmed.rfind(end_ch) {
                if end_idx > start_idx {
                    let candidate = &trimmed[start_idx..=end_idx];
                    if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

// Small helper to make the tuple pattern readable.
#[inline]
fn pair(a: char, b: char) -> (char, char) {
    (a, b)
}

/// Try each line of the text as a complete JSON value.
fn extract_first_json_line(text: &str) -> Option<Value> {
    for line in text.lines() {
        let t = line.trim();
        if t.len() < 2 {
            continue;
        }
        if t.starts_with('{') || t.starts_with('[') {
            if let Ok(v) = serde_json::from_str::<Value>(t) {
                return Some(v);
            }
        }
    }
    None
}
