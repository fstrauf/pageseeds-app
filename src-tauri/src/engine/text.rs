/// Text utilities shared across engine modules.
///
/// Purpose: avoid UTF-8 panics from byte slicing like `s[..300]`.

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

#[cfg(test)]
mod tests {
    use super::char_prefix;

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
        // Previously this class of content could panic when sliced by bytes.
        let p = char_prefix(s, 40);
        assert!(p.len() <= s.len());
        assert!(s.starts_with(p));
    }
}
