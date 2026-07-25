//! Fact rail: calendar years in SEO copy must match the current year.
//!
//! Stale or future years in titles/meta are a common agent hallucination
//! (e.g. "Best Stocks 2024" written in 2026). These helpers extract 20xx
//! years and gate recommended/patched strings so only the current calendar
//! year is allowed when any year is present.

use chrono::{Datelike, Utc};
use regex::Regex;
use std::sync::OnceLock;

fn year_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(20\d{2})\b").expect("year regex"))
}

/// True when `token` is a pure 20xx calendar year (exactly four digits, 20xx).
///
/// Used by keyword matching to treat years as optional, and shared so token
/// detection and body extraction agree on what counts as a year.
pub fn is_calendar_year_token(token: &str) -> bool {
    token.len() == 4
        && token.as_bytes()[0] == b'2'
        && token.as_bytes()[1] == b'0'
        && token.as_bytes()[2].is_ascii_digit()
        && token.as_bytes()[3].is_ascii_digit()
}

/// Extract all 20xx calendar years from `text` (order of appearance, duplicates kept).
pub fn extract_years(text: &str) -> Vec<i32> {
    year_re()
        .captures_iter(text)
        .filter_map(|c| c.get(1)?.as_str().parse::<i32>().ok())
        .collect()
}

/// True when `text` has no 20xx years, or every extracted year equals `current_year`.
pub fn years_ok(text: &str, current_year: i32) -> bool {
    let years = extract_years(text);
    years.is_empty() || years.iter().all(|&y| y == current_year)
}

/// Error when `text` contains any 20xx year that is not `current_year`.
///
/// Shared by content-fix and CTR patch validators for title/description.
/// Returns `None` when years are ok (absent or all equal to current year).
pub fn non_current_year_error(field: &str, text: &str, current_year: i32) -> Option<String> {
    if years_ok(text, current_year) {
        None
    } else {
        Some(format!(
            "{field} contains year not equal to current calendar year"
        ))
    }
}

/// Current calendar year in UTC.
pub fn current_calendar_year() -> i32 {
    Utc::now().year()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_calendar_year_token_accepts_pure_20xx() {
        assert!(is_calendar_year_token("2024"));
        assert!(is_calendar_year_token("2026"));
        assert!(!is_calendar_year_token("1999"));
        assert!(!is_calendar_year_token("202"));
        assert!(!is_calendar_year_token("20250"));
        assert!(!is_calendar_year_token("year"));
        assert!(!is_calendar_year_token("202a"));
    }

    #[test]
    fn extract_years_finds_20xx_word_boundaries() {
        assert_eq!(extract_years("Best stocks 2025 and 2026"), vec![2025, 2026]);
        assert_eq!(extract_years("no years here"), Vec::<i32>::new());
        // 19xx and non-word-boundary digits are ignored
        assert_eq!(extract_years("1999 and x2025y and 20250"), Vec::<i32>::new());
        assert_eq!(extract_years("guide-2024-edition"), vec![2024]);
    }

    #[test]
    fn years_ok_empty_or_all_current() {
        assert!(years_ok("Best stocks for wheel strategy", 2026));
        assert!(years_ok("Best stocks 2026", 2026));
        assert!(years_ok("2026 guide to 2026 markets", 2026));
        assert!(!years_ok("Best stocks 2025", 2026));
        assert!(!years_ok("2025 and 2026 picks", 2026));
        assert!(!years_ok("2024 review", 2026));
    }

    #[test]
    fn non_current_year_error_names_field() {
        assert!(non_current_year_error("title", "No year", 2026).is_none());
        assert!(non_current_year_error("title", "Best 2026", 2026).is_none());
        let err = non_current_year_error("title", "Best 2025", 2026).unwrap();
        assert!(err.contains("title"));
        assert!(err.contains("year not equal to current calendar year"));
        let err = non_current_year_error("description", "2025-2026 guide", 2026).unwrap();
        assert!(err.starts_with("description"));
    }

    #[test]
    fn current_calendar_year_is_reasonable() {
        let y = current_calendar_year();
        assert!(y >= 2024 && y <= 2100, "unexpected year {}", y);
    }
}
