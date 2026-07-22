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

/// Current calendar year in UTC.
pub fn current_calendar_year() -> i32 {
    Utc::now().year()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn current_calendar_year_is_reasonable() {
        let y = current_calendar_year();
        assert!(y >= 2024 && y <= 2100, "unexpected year {}", y);
    }
}
