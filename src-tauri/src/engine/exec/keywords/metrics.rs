//! Volume estimation and SERP metric helpers.

use super::*;

pub(crate) fn estimate_volume(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    // Ahrefs free tools often return enum-like labels instead of numeric ranges.
    match s {
        "MoreThanTenThousand" => return Some(10000),
        "MoreThanOneThousand" => return Some(1000),
        "MoreThanOneHundred" => return Some(100),
        "LessThanOneHundred" => return Some(50),
        _ => {}
    }

    let mut raw_chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == ',' {
            current.push(ch);
        } else if !current.is_empty() {
            raw_chunks.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        raw_chunks.push(current);
    }

    let nums: Vec<i64> = raw_chunks
        .into_iter()
        .map(|c| c.replace(',', ""))
        .filter_map(|p| p.parse::<i64>().ok())
        .collect();

    match nums.as_slice() {
        [] => None,
        [single] => Some(*single),
        [a, b, ..] => Some((a + b) / 2),
    }
}

pub(crate) fn best_serp_metric(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values.flatten().fold(None, |acc, v| match acc {
        Some(current) if current >= v => Some(current),
        _ => Some(v),
    })
}

/// A keyword candidate discovered from a seed theme.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) keyword: String,
    pub(crate) source_theme: String,
    pub(crate) is_question: bool,
    pub(crate) volume: Option<i64>,
    pub(crate) kd: Option<f64>,
    pub(crate) intent: Option<String>,
    /// Coverage-gap score (0-100) assigned by `filter_by_coverage_gap`;
    /// `None` when no coverage analysis was available for the project.
    pub(crate) gap_score: Option<f64>,
}
