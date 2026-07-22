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

/// Significant tokens of a keyword: alphanumeric, length >= 2, not a stopword,
/// not a pure 20xx year (years in stored keywords are optional for presence).
fn significant_tokens(normalized_keyword: &str) -> Vec<String> {
    normalized_keyword
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| {
            t.len() >= 2
                && !STOPWORDS.contains(t)
                && !crate::content::year_policy::is_calendar_year_token(t)
        })
        .map(String::from)
        .collect()
}

/// True when `haystack_lower` (already lowercased) covers the keyword.
///
/// Matching order:
/// 1. Verbatim phrase on the quote-stripped, whitespace-collapsed keyword.
/// 2. **Alternative phrases** — when the stored keyword is multiple quoted
///    segments (`"iron condor" "close at 50% profit"`) or an explicit
///    comparison (`"cash-secured put" vs "naked put"`), accept if **any**
///    phrase is present (verbatim or, for long phrases, via tokens).
/// 3. For long single phrases (4+ significant tokens), accept all significant
///    tokens being present anywhere. Short keywords stay strict: scattered
///    tokens are not evidence of targeting a 2–3 word phrase.
pub fn keyword_present(haystack_lower: &str, keyword: &str) -> bool {
    let normalized = normalize_keyword(keyword);
    if normalized.is_empty() {
        return false;
    }
    if haystack_lower.contains(&normalized) {
        return true;
    }

    // Multi-phrase / comparison keywords: any alternative is enough.
    // Requiring every token from every phrase (e.g. both "iron condor" *and*
    // "close at 50% profit") made well-optimized intros fail hard-fail the
    // whole content/CTR patch.
    if let Some(phrases) = alternative_phrases(keyword) {
        if phrases
            .iter()
            .any(|phrase| phrase_present(haystack_lower, phrase))
        {
            return true;
        }
    }

    let tokens = significant_tokens(&normalized);
    tokens.len() >= 4 && tokens.iter().all(|t| token_match_count(haystack_lower, t) > 0)
}

/// Match a single phrase (already a candidate alternative) against haystack.
fn phrase_present(haystack_lower: &str, phrase: &str) -> bool {
    let normalized = normalize_keyword(phrase);
    if normalized.is_empty() {
        return false;
    }
    if haystack_lower.contains(&normalized) {
        return true;
    }
    let tokens = significant_tokens(&normalized);
    // Allow token fallback for medium phrases too (3+), since alternatives are
    // themselves intended targets, not junk concatenations.
    tokens.len() >= 3 && tokens.iter().all(|t| token_match_count(haystack_lower, t) > 0)
}

/// Split messy stored keywords into alternative target phrases.
///
/// Returns `Some` only when there are 2+ real alternatives (quoted segments
/// or `vs`/`versus` comparisons). Single-phrase keywords return `None` so
/// the stricter single-phrase path applies.
fn alternative_phrases(keyword: &str) -> Option<Vec<String>> {
    let trimmed = keyword.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Prefer explicit quote-delimited segments when 2+ are present.
    let quoted: Vec<String> = {
        let mut out = Vec::new();
        let mut rest = trimmed;
        while let Some(start) = rest.find(['"', '\'']) {
            let quote = rest.as_bytes()[start] as char;
            rest = &rest[start + 1..];
            if let Some(end) = rest.find(quote) {
                let inner = rest[..end].trim();
                if !inner.is_empty() {
                    out.push(inner.to_string());
                }
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
        out
    };
    if quoted.len() >= 2 {
        return Some(quoted);
    }

    // Comparison form: `cash-secured put vs naked put` (with or without quotes).
    let lower = trimmed.to_lowercase();
    for sep in [" vs ", " versus "] {
        if let Some(idx) = lower.find(sep) {
            let left = trimmed[..idx].trim();
            let right = trimmed[idx + sep.len()..].trim();
            let left_n = normalize_keyword(left);
            let right_n = normalize_keyword(right);
            if !left_n.is_empty() && !right_n.is_empty() {
                return Some(vec![left.to_string(), right.to_string()]);
            }
        }
    }

    None
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

/// Maximum content words kept from a backfilled GSC query. Longer phrases
/// can never appear in a 55-char title, so fix-pipeline keyword validation
/// would be unsatisfiable (issue #74).
const BACKFILLED_KEYWORD_MAX_WORDS: usize = 5;

/// Normalize a GSC query into a titleable target keyword for backfill.
///
/// A query is not a keyword: real top-clicked queries are long natural-
/// language phrases, branded/navigational terms, or scraped Q&A junk
/// (`3. joelle wants … * 1 point 3 months 9 months`). Normalization:
/// lowercase + strip quotes (via `normalize_keyword`), reject quiz/PAQ junk
/// (leading enumeration like `3. `, points markers like `* 1 point`) and
/// queries containing any of the project's brand tokens, drop stopwords,
/// cap at 5 content words. Returns `None` when nothing titleable remains —
/// the caller then leaves `target_keyword` empty rather than storing junk.
pub fn normalize_backfilled_keyword(query: &str, brand_tokens: &[String]) -> Option<String> {
    let normalized = normalize_keyword(query);
    if normalized.is_empty() {
        return None;
    }

    // Scraped quiz / Q&A junk — nothing titleable survives, reject outright.
    let leading_enumeration = regex::Regex::new(r"^\d{1,2}[.)]\s").unwrap();
    let points_marker = regex::Regex::new(r"\*+\s*\d+\s*points?\b").unwrap();
    if leading_enumeration.is_match(&normalized) || points_marker.is_match(&normalized) {
        return None;
    }

    // Branded / navigational queries name the project — not targetable keywords.
    let brands: Vec<String> = brand_tokens.iter().map(|t| t.to_lowercase()).collect();
    if !brands.is_empty()
        && normalized
            .split(|c: char| !c.is_alphanumeric())
            .any(|token| !token.is_empty() && brands.iter().any(|b| b == token))
    {
        return None;
    }

    let content_words: Vec<String> = significant_tokens(&normalized)
        .into_iter()
        .take(BACKFILLED_KEYWORD_MAX_WORDS)
        .collect();
    if content_words.is_empty() {
        return None;
    }
    Some(content_words.join(" "))
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
    fn keyword_years_are_optional_for_presence() {
        // Stored keyword carries dual years; haystack has only one / different year
        // — core tokens still match (issue #112 rail B).
        assert!(keyword_present(
            "best stocks for the wheel strategy 2026",
            "best stocks for wheel strategy 2025 2026"
        ));
        assert!(keyword_present(
            "best stocks for the wheel strategy",
            "best stocks for wheel strategy 2025 2026"
        ));
        // Core mismatch still fails even when years would "match".
        assert!(!keyword_present(
            "best stocks for iron condors 2025 2026",
            "best stocks for wheel strategy 2025 2026"
        ));
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

    #[test]
    fn multi_quoted_phrases_match_any_alternative() {
        // Stored junk: two quoted intents glued together. Intro only covers
        // the primary phrase — must still pass (used to hard-fail whole patch).
        let intro = "an iron condor is a defined-risk options spread for income sellers";
        assert!(keyword_present(
            intro,
            "\"iron condor\" \"close at 50% profit\""
        ));
        // Secondary phrase alone also counts.
        let alt = "many traders close at 50% profit before expiration risk grows";
        assert!(keyword_present(
            alt,
            "\"iron condor\" \"close at 50% profit\""
        ));
        // Neither phrase / tokens → fail.
        assert!(!keyword_present(
            "wheel strategy on blue chip stocks",
            "\"iron condor\" \"close at 50% profit\""
        ));
    }

    #[test]
    fn vs_comparison_keyword_matches_either_side() {
        let desc = "learn when a cash-secured put beats a short stock position";
        assert!(keyword_present(
            desc,
            "\"cash-secured put\" vs \"naked put\""
        ));
        let desc2 = "a naked put has undefined risk if the underlying collapses";
        assert!(keyword_present(
            desc2,
            "\"cash-secured put\" vs \"naked put\""
        ));
        assert!(!keyword_present(
            "covered call income guide",
            "\"cash-secured put\" vs \"naked put\""
        ));
    }

    #[test]
    fn backfill_long_query_with_stopwords_capped_at_five_words() {
        let brand: Vec<String> = vec![];
        // 8-word natural-language query → stopwords dropped, capped at 5.
        assert_eq!(
            normalize_backfilled_keyword(
                "adding custom categories to google sheets budget template",
                &brand
            ),
            Some("adding custom categories google sheets".to_string())
        );
        // Quotes stripped before normalization.
        assert_eq!(
            normalize_backfilled_keyword("\"iron condor\" adjustments", &brand),
            Some("iron condor adjustments".to_string())
        );
    }

    #[test]
    fn backfill_quiz_question_query_returns_none() {
        let brand: Vec<String> = vec![];
        // Real scraped quiz question observed on the expense project.
        assert_eq!(
            normalize_backfilled_keyword(
                "3. joelle wants to have an emergency fund… * 1 point 3 months 9 months 24 months 30 months",
                &brand
            ),
            None
        );
        // Points marker alone also marks the query as junk.
        assert_eq!(
            normalize_backfilled_keyword("how many months of expenses * 1 point", &brand),
            None
        );
    }

    #[test]
    fn backfill_brand_query_returns_none() {
        let brand = vec!["expense".to_string(), "sorted".to_string()];
        assert_eq!(normalize_backfilled_keyword("expense sorted", &brand), None);
        assert_eq!(
            normalize_backfilled_keyword("expense tracker spreadsheet", &brand),
            None
        );
        // Non-brand queries survive.
        assert_eq!(
            normalize_backfilled_keyword("budget spreadsheet template", &brand),
            Some("budget spreadsheet template".to_string())
        );
    }

    #[test]
    fn backfill_stopword_only_query_returns_none() {
        let brand: Vec<String> = vec![];
        assert_eq!(normalize_backfilled_keyword("how to what is", &brand), None);
        assert_eq!(normalize_backfilled_keyword("", &brand), None);
    }
}
