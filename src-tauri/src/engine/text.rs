/// Text utilities shared across engine modules.
///
/// Purpose: avoid UTF-8 panics from byte slicing like `s[..300]`.

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
}
