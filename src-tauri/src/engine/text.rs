/// Text utilities shared across engine modules.
///
/// Purpose: avoid UTF-8 panics from byte slicing like `s[..300]`.
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Returns a valid UTF-8 prefix with at most `max_chars` Unicode scalar values.
///
/// This never panics, even when the string contains multi-byte characters.
pub fn char_prefix(s: &str, max_chars: usize) -> &str {
    if let Some((idx, _)) = s.char_indices().nth(max_chars) {
        &s[..idx]
    } else {
        s
    }
}

/// Extract JSON from raw LLM text.
///
/// Attempts (in order):
/// 1. Parse the entire text as JSON
/// 2. Extract JSON from a fenced code block (```json ... ```)
/// 3. Find a bare JSON object/array in the text
///
/// Extracts the first JSON object or array found in a string.
/// Used as a fallback when structured output is returned as raw text.
pub fn extract_json(text: &str) -> Option<Value> {
    // 1. Whole text is JSON
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }

    // 2. Fenced code block
    for pat in ["```json\n", "```json\r\n", "```JSON\n", "```\n"] {
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

    // 3. Bare JSON object/array
    let trimmed = text.trim();
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let Some(start) = trimmed.find(open) {
            if let Some(end) = trimmed.rfind(close) {
                if end > start {
                    let candidate = &trimmed[start..=end];
                    if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                        return Some(v);
                    }
                }
            }
        }
    }

    None
}

/// Typed JSON extraction: attempts to parse the extracted JSON into a specific type.
///
/// This is a thin wrapper around `extract_json` that adds type safety. Prefer this
/// over manual `serde_json::from_str` when parsing agent output.
pub fn extract_json_as<T: DeserializeOwned>(text: &str) -> Option<T> {
    let value = extract_json(text)?;
    serde_json::from_value(value).ok()
}

/// Extract the raw JSON string from agent output without parsing.
///
/// Returns the first valid JSON substring found (fenced or bare), or None.
/// Useful when you need the raw string for further processing.
pub fn extract_json_string(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // 1. Whole text is JSON
    if serde_json::from_str::<Value>(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }

    // 2. Fenced code block
    for pat in ["```json\n", "```json\r\n", "```JSON\n", "```\n"] {
        if let Some(start) = text.find(pat) {
            let after_open = start + pat.len();
            let rest = &text[after_open..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                if serde_json::from_str::<Value>(candidate).is_ok() {
                    return Some(candidate.to_string());
                }
            }
        }
    }

    // 3. Bare JSON object/array
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let Some(start) = trimmed.find(open) {
            if let Some(end) = trimmed.rfind(close) {
                if end > start {
                    let candidate = &trimmed[start..=end];
                    if serde_json::from_str::<Value>(candidate).is_ok() {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Strip common LLM meta-commentary that leaks outside the requested markdown format.
///
/// Removes preambles like "Done.", "Here is the spec:", "It contains:", backtick-wrapped
/// file paths, and postambles like "Let me know if you need anything else."
///
/// Use this whenever an agentic step expects the LLM to output raw markdown but the model
/// instead wraps it in conversational commentary.
pub fn strip_agent_markdown_preambles(raw: &str) -> String {
    let mut cleaned = raw.trim().to_string();

    // Strip common preambles (case-insensitive, anchored to start)
    let preamble_patterns = [
        r"(?i)^\s*done\.\s*",
        r"(?i)^\s*ok\.\s*",
        r"(?i)^\s*alright\.\s*",
        r"(?i)^\s*here\s+is\s+(the\s+)?spec(ification)?[:\.]?\s*",
        r"(?i)^\s*here\s+is\s+(the\s+)?markdown\s+document[:\.]?\s*",
        r"(?i)^\s*here\s+is\s+(the\s+)?feature\s+spec(ification)?[:\.]?\s*",
        r"(?i)^\s*i[''']?ve\s+written\s+(the\s+)?spec[:\.]?\s*",
        r"(?i)^\s*the\s+spec\s+has\s+been\s+written\s+to[:\.]?\s*",
        r"(?i)^\s*the\s+specification\s+is\s+below[:\.]?\s*",
        r"(?i)^\s*below\s+is\s+(the\s+)?spec[:\.]?\s*",
        r"(?i)^\s*generating\s+(the\s+)?spec[:\.]?\s*",
        r"(?i)^.*written\s+to\s+`[^`]+`\s*",
        r"(?i)^.*saved\s+to\s+`[^`]+`\s*",
        // LLMs sometimes emit a summary block like:
        // "It contains:\n- 2 P0 code issues\n- 3 P1 content issues..."
        r"(?im)^\s*it\s+contains[:\.]?\s*$(?:\n^\s*[-*]\s+.*$)*",
    ];

    for pattern in &preamble_patterns {
        let re = regex::Regex::new(pattern).unwrap();
        cleaned = re.replace(&cleaned, "").to_string();
    }

    // Strip lines that are just a backtick-wrapped path (e.g. `docs/...md`)
    // These often appear after "written to:" summaries.
    cleaned = regex::Regex::new(r"(?m)^\s*`[^`]+`\s*$\n?")
        .unwrap()
        .replace_all(&cleaned, "")
        .to_string();

    // Strip common postambles (case-insensitive, anchored to end)
    let postamble_patterns = [
        r"(?i)\s*let\s+me\s+know\s+if\s+you\s+need\s+anything\s+else[\.!]?\s*$",
        r"(?i)\s*feel\s+free\s+to\s+ask\s+if\s+you\s+need\s+changes[\.!]?\s*$",
        r"(?i)\s*this\s+spec\s+is\s+ready\s+for\s+implementation[\.!]?\s*$",
        r"(?i)\s*the\s+spec\s+has\s+been\s+saved[\.!]?\s*$",
        r"(?i)\s*saved\s+to\s+`[^`]+`\s*$",
        r"(?i)\s*written\s+to\s+`[^`]+`\s*$",
    ];

    for pattern in &postamble_patterns {
        let re = regex::Regex::new(pattern).unwrap();
        cleaned = re.replace(&cleaned, "").to_string();
    }

    cleaned.trim().to_string()
}

/// Extract a markdown document from raw LLM output that may be buried in commentary.
///
/// First strips common agent preambles/postambles, then if the result does not start
/// with a `#` heading, searches for `expected_heading` (if provided) or any `#` heading.
/// Returns `None` if no markdown heading can be found.
///
/// Example:
/// ```
/// use pageseeds_lib::engine::text::extract_markdown_document;
///
/// let raw = "Done. Here is the spec:\n\n# SEO Feature Specification\n...";
/// let doc = extract_markdown_document(raw, Some("# SEO Feature Specification"));
/// assert!(doc.unwrap().starts_with("# SEO Feature Specification"));
/// ```
pub fn extract_markdown_document(raw: &str, expected_heading: Option<&str>) -> Option<String> {
    let cleaned = strip_agent_markdown_preambles(raw);
    let trimmed = cleaned.trim();

    if trimmed.starts_with('#') {
        return Some(trimmed.to_string());
    }

    // Try to find the expected heading first
    if let Some(heading) = expected_heading {
        if let Some(pos) = trimmed.find(heading) {
            return Some(trimmed[pos..].to_string());
        }
    }

    // Fall back to any markdown heading
    if let Some(pos) = trimmed.find("# ") {
        return Some(trimmed[pos..].to_string());
    }

    // If first line is not a heading, check if second line starts with #
    if let Some(pos) = trimmed.find('\n') {
        let after_first = &trimmed[pos + 1..].trim_start();
        if after_first.starts_with('#') {
            return Some(after_first.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_prefix_handles_ascii() {
        assert_eq!(char_prefix("abcdef", 3), "abc");
        assert_eq!(char_prefix("abc", 99), "abc");
    }

    #[test]
    fn char_prefix_handles_multibyte_boundary() {
        let s = "A└B";
        assert_eq!(char_prefix(s, 2), "A└");
        assert_eq!(char_prefix(s, 3), "A└B");
    }

    #[test]
    fn char_prefix_never_panics_with_box_drawing_logs() {
        let s = "● Read seo_content_brief.md\n │ path\n └ 1 line read";
        let p = char_prefix(s, 40);
        assert!(p.len() <= s.len());
        assert!(s.starts_with(p));
    }

    #[test]
    fn extract_json_bare_object() {
        let text = r#"{"key": "value"}"#;
        let result = extract_json(text).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn extract_json_fenced_block() {
        let text = "Some prose\n\n```json\n{\"key\": \"value\"}\n```\nMore prose";
        let result = extract_json(text).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn extract_json_no_json() {
        let text = "Just plain text with no JSON at all.";
        assert!(extract_json(text).is_none());
    }

    // ── strip_agent_markdown_preambles ─────────────────────────────────────────

    #[test]
    fn strip_preambles_removes_done_written_to() {
        let raw = "Done. The spec has been written to:\n\n`docs/SEO_FEATURE_SPEC.md`\n\nIt contains:\n- **2 P0 code issues**\n- **3 P1 content issues**\n";
        let cleaned = strip_agent_markdown_preambles(raw);
        assert!(!cleaned.contains("Done."));
        assert!(!cleaned.contains("written to"));
        assert!(!cleaned.contains("docs/SEO_FEATURE_SPEC"));
        assert!(!cleaned.contains("It contains:"));
    }

    #[test]
    fn strip_preambles_preserves_clean_markdown() {
        let raw = "# SEO Feature Specification\n\n## Executive Summary\n";
        let cleaned = strip_agent_markdown_preambles(raw);
        assert_eq!(cleaned, raw.trim());
    }

    #[test]
    fn strip_preambles_removes_postamble() {
        let raw = "# Spec\n\nLet me know if you need anything else.";
        let cleaned = strip_agent_markdown_preambles(raw);
        assert!(!cleaned.to_lowercase().contains("let me know"));
    }

    // ── extract_markdown_document ──────────────────────────────────────────────

    #[test]
    fn extract_document_finds_expected_heading() {
        let raw = "Done. Here is the spec:\n\n# SEO Feature Specification\n\n## P0\n";
        let doc = extract_markdown_document(raw, Some("# SEO Feature Specification"));
        assert!(doc.unwrap().starts_with("# SEO Feature Specification"));
    }

    #[test]
    fn extract_document_returns_none_for_no_heading() {
        let raw = "Just some plain text with no markdown at all.";
        assert!(extract_markdown_document(raw, None).is_none());
    }

    #[test]
    fn extract_document_falls_back_to_any_heading() {
        let raw = "Some intro\n# Any Heading\nContent here.";
        let doc = extract_markdown_document(raw, None);
        assert!(doc.unwrap().starts_with("# Any Heading"));
    }
}
