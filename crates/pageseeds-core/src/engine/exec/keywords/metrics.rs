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
    /// Cost per click in USD (DataForSEO keyword_info); `None` when the
    /// provider does not return CPC. Drives commercial-value ranking for
    /// landing page candidates.
    pub(crate) cpc: Option<f64>,
    /// Coverage-gap score (0-100) assigned by `filter_by_coverage_gap`;
    /// `None` when no coverage analysis was available for the project.
    pub(crate) gap_score: Option<f64>,
}

/// Minimum known monthly search volume to keep a candidate.
///
/// Unknown volume (`None`) is **kept** — only known volume strictly below this
/// threshold is dropped. See #263.
pub(crate) const MIN_VOLUME: i64 = 50;

/// Aggregate counters from [`filter_candidates_by_volume`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VolumeFilterStats {
    /// Known volume strictly below [`MIN_VOLUME`].
    pub volume_dropped: usize,
    /// `volume == None` kept (not treated as below threshold).
    pub volume_unknown_kept: usize,
}

/// Keep candidates with `volume >= MIN_VOLUME` **or** `volume == None`.
/// Drop only known low volume (`Some(v) if v < MIN_VOLUME`).
pub(crate) fn filter_candidates_by_volume(
    candidates: Vec<Candidate>,
) -> (Vec<Candidate>, VolumeFilterStats) {
    let mut kept = Vec::with_capacity(candidates.len());
    let mut stats = VolumeFilterStats::default();
    for c in candidates {
        match c.volume {
            Some(v) if v >= MIN_VOLUME => kept.push(c),
            Some(_) => stats.volume_dropped += 1,
            None => {
                stats.volume_unknown_kept += 1;
                kept.push(c);
            }
        }
    }
    (kept, stats)
}
