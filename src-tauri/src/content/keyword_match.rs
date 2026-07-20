//! Canonical target-keyword matching for SEO checks.
//!
//! Stored `target_keyword` values are messy: some contain literal quote
//! characters (`"theta decay accelerates near expiration"`), some are long
//! multi-token phrases that never occur verbatim in prose (`best stocks for
//! wheel strategy 2025 2026`). Plain `contains()` checks against those raw
//! strings produce false failures on well-optimized pages. These helpers
//! normalize the keyword and provide tolerant matching — use them for every
//! keyword-presence or keyword-density check instead of raw `str::contains`
//! / `str::matches`.

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "at", "to", "of", "for", "in", "on", "and", "or", "with", "how", "what",
    "is", "are", "vs", "your", "you", "my", "our", "we", "it", "its", "by", "as", "be", "do",
    "does", "can", "will", "that", "this",
];

/// Normalize a stored target keyword for matching: strip literal quote
/// characters, lowercase, collapse whitespace.
pub fn normalize_keyword(kw: &str) -> String {
    kw.replace(['"', '\''], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Significant tokens of a keyword: alphanumeric, length >= 2, not a stopword.
fn significant_tokens(normalized_keyword: &str) -> Vec<String> {
    normalized_keyword
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2 && !STOPWORDS.contains(t))
        .map(String::from)
        .collect()
}

/// True when `haystack_lower` (already lowercased) covers the keyword.
///
/// Verbatim phrase match on the normalized keyword first. For long keywords
/// (4+ significant tokens — full sentences or multi-phrase strings that
/// realistically never occur verbatim), accept all significant tokens being
/// present anywhere in the haystack instead. Short keywords stay strict:
/// scattered tokens are not evidence of targeting a 2–3 word phrase.
pub fn keyword_present(haystack_lower: &str, keyword: &str) -> bool {
    let normalized = normalize_keyword(keyword);
    if normalized.is_empty() {
        return false;
    }
    if haystack_lower.contains(&normalized) {
        return true;
    }
    let tokens = significant_tokens(&normalized);
    tokens.len() >= 4 && tokens.iter().all(|t| token_match_count(haystack_lower, t) > 0)
}

/// Occurrence count of the keyword in `text_lower` (already lowercased), for
/// density calculations. Verbatim phrase count when the phrase occurs; for
/// multi-token keywords with no verbatim occurrence, the average per-token
/// occurrence count — an approximation of how densely the keyword's
/// vocabulary is used.
pub fn keyword_occurrences(text_lower: &str, keyword: &str) -> usize {
    let normalized = normalize_keyword(keyword);
    if normalized.is_empty() {
        return 0;
    }
    let phrase_count = text_lower.matches(&normalized).count();
    if phrase_count > 0 {
        return phrase_count;
    }
    let tokens = significant_tokens(&normalized);
    if tokens.len() < 2 {
        return 0;
    }
    let total: usize = tokens
        .iter()
        .map(|t| token_match_count(text_lower, t))
        .sum();
    total / tokens.len()
}

/// Whole-word occurrence count of a single token (substring matching would
/// count "50" inside "500" or "put" inside "computer").
fn token_match_count(text_lower: &str, token: &str) -> usize {
    let pattern = format!(r"\b{}\b", regex::escape(token));
    regex::Regex::new(&pattern)
        .map(|re| re.find_iter(text_lower).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_quotes_and_collapses_whitespace() {
        assert_eq!(
            normalize_keyword("\"theta decay accelerates near expiration\""),
            "theta decay accelerates near expiration"
        );
        assert_eq!(
            normalize_keyword("\"iron condor\" \"close at 50% profit\""),
            "iron condor close at 50% profit"
        );
        assert_eq!(normalize_keyword("  Wheel   Strategy "), "wheel strategy");
    }

    #[test]
    fn present_verbatim_after_quote_stripping() {
        let title = "theta decay accelerates near expiration: a dte guide";
        assert!(keyword_present(
            title,
            "\"theta decay accelerates near expiration\""
        ));
    }

    #[test]
    fn present_long_phrase_via_tokens() {
        let title = "best stocks for the wheel strategy: 2025-2026 screening";
        assert!(keyword_present(title, "best stocks for wheel strategy 2025 2026"));
    }

    #[test]
    fn absent_when_tokens_missing() {
        let title = "best stocks for the wheel strategy: 2025-2026 screening";
        assert!(!keyword_present(title, "best stocks for iron condors 2025 2026"));
    }

    #[test]
    fn short_keywords_stay_strict() {
        let text = "iron prices rose; the condor is a large bird";
        assert!(!keyword_present(text, "iron condor"));
        let text = "trade the iron condor for income";
        assert!(keyword_present(text, "iron condor"));
    }

    #[test]
    fn occurrences_verbatim_phrase() {
        let text = "theta decay is real. theta decay matters.";
        assert_eq!(keyword_occurrences(text, "theta decay"), 2);
    }

    #[test]
    fn occurrences_token_average_fallback() {
        // "theta" x2, "decay" x2, "accelerates" x0, "expiration" x0 -> avg 1
        let text = "theta decay and theta decay again";
        assert_eq!(keyword_occurrences(text, "theta decay accelerates expiration"), 1);
    }

    #[test]
    fn occurrences_no_false_substring_matches() {
        // "50" inside "500" and "put" inside "computer" must not count;
        // the standalone "put" alone averages to 0 occurrences.
        let text = "the computer put 500 units";
        assert_eq!(keyword_occurrences(text, "50 put"), 0);
    }

    #[test]
    fn empty_keyword_never_matches() {
        assert!(!keyword_present("anything", ""));
        assert_eq!(keyword_occurrences("anything", ""), 0);
    }
}
