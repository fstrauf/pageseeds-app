//! Keyword winnability classifier.
//!
//! Scores whether a keyword is worth targeting based on SERP features (AI
//! Overview presence, competitor authority) and keyword metrics (difficulty,
//! intent). This prevents the research pipeline from proposing keywords that
//! are structurally unwinnable — the root cause of the "dead weight" article
//! problem (25+ indexed articles with zero impressions).

use crate::seo::keywords::SerpFeaturesResult;
use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinnabilityBucket {
    /// Winnable: low AIO risk, competitors are beatable.
    Target,
    /// Winnable only with a proprietary angle (original data, tools, real
    /// experience). Generic educational content will not compete.
    Differentiate,
    /// Unwinnable: AIO-dominated or authority gap too large.
    /// Do not create an article for this keyword.
    Avoid,
}

impl WinnabilityBucket {
    pub fn as_str(&self) -> &'static str {
        match self {
            WinnabilityBucket::Target => "target",
            WinnabilityBucket::Differentiate => "differentiate",
            WinnabilityBucket::Avoid => "avoid",
        }
    }
}

impl std::fmt::Display for WinnabilityBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinnabilityAssessment {
    pub keyword: String,
    pub bucket: WinnabilityBucket,
    pub ai_overview_present: bool,
    pub featured_snippet_present: bool,
    /// Authority domains found in the top-5 organic results.
    pub authority_competitors: Vec<String>,
    pub risk_score: u32,
    pub reason: String,
}

// ─── Authority domains ───────────────────────────────────────────────────────

/// High-authority domains that are extremely difficult to outrank for
/// informational finance/investing content. When 2+ of these occupy the
/// top-5, a small or niche site cannot realistically compete with generic
/// educational content.
///
/// TODO: make this configurable per project (different niches have different
/// authority competitors). For now, covers the daystoexpiry.com use case.
const AUTHORITY_DOMAINS: &[&str] = &[
    "investopedia.com",
    "wikipedia.org",
    "tastylive.com",
    "nasdaq.com",
    "marketwatch.com",
    "bloomberg.com",
    "morningstar.com",
    "forbes.com",
    "nerdwallet.com",
    "bankrate.com",
    "thebalancemoney.com",
    "thebalance.com",
    "money.com",
    "businessinsider.com",
    "cnbc.com",
    "reuters.com",
    "finance.yahoo.com",
    "wsj.com",
    "warriortrading.com",
    "tdameritrade.com",
    "fidelity.com",
    "schwab.com",
    "etrade.com",
];

fn is_authority_domain(domain: &str) -> bool {
    let lower = domain.to_lowercase();
    AUTHORITY_DOMAINS.iter().any(|a| lower == *a || lower.ends_with(&format!(".{}", a)))
}

// ─── Scoring ─────────────────────────────────────────────────────────────────

/// Short-answer query shapes: definitional lookups an AI Overview can answer
/// completely inline, so the zero-click risk is real. Process/strategy queries
/// ("wash sale disallowed", "covered call strategy") imply a user with a
/// situation — they still click for depth, examples, and tools.
fn is_short_answer_query(keyword: &str) -> bool {
    let k = keyword.to_lowercase();
    k.starts_with("what is")
        || k.starts_with("what are")
        || k.starts_with("what's")
        || k.starts_with("define ")
        || k.starts_with("meaning of")
        || k.starts_with("definition of")
        || k.ends_with(" meaning")
        || k.ends_with(" definition")
        || k.contains(" vs ")
}

/// Assess a keyword's winnability based on SERP features and keyword metrics.
///
/// Scoring philosophy:
/// - AIO presence is weighted by query shape and intent (single AIO branch):
///   +2 for short-answer queries (genuine zero-click risk),
///   +2 for explicit informational intent (AIO-heavy educational SERPs),
///   +1 otherwise for commercial/transactional (AIO summarizes but users still
///   click for depth; AIO citation is also an attainable target).
/// - Authority domains (capped +2): counted when AIO is present (any KD), or
///   when AIO is absent and KD >= 30. Without AIO and KD < 30, authority is
///   ignored — KD has already priced in weak SERP slots.
/// - Buckets: 0–1 Target, 2–3 Differentiate, ≥4 Avoid.
///
/// # Arguments
/// * `keyword` - The keyword being assessed.
/// * `serp` - SERP feature data (AIO, snippets, competitor domains).
/// * `kd` - Keyword difficulty score (0-100), if available.
/// * `intent` - Search intent ("informational", "commercial", "transactional").
///   Explicit `informational` raises AIO risk to +2; empty/None is not treated
///   as informational.
pub fn assess(
    keyword: &str,
    serp: &SerpFeaturesResult,
    kd: Option<f64>,
    intent: Option<&str>,
) -> WinnabilityAssessment {
    let mut risk_score: u32 = 0;
    let mut reasons: Vec<&str> = Vec::new();

    let is_informational = intent
        .map(|i| i.eq_ignore_ascii_case("informational"))
        .unwrap_or(false);

    // AI Overview — single branch: short-answer > informational > default.
    if serp.ai_overview_present {
        if is_short_answer_query(keyword) {
            risk_score += 2;
            reasons.push("AI Overview present (short-answer query)");
        } else if is_informational {
            risk_score += 2;
            reasons.push("AI Overview present (informational query)");
        } else {
            risk_score += 1;
            reasons.push("AI Overview present");
        }
    }

    // Featured snippet — position 0 already captured.
    if serp.featured_snippet_present {
        risk_score += 1;
        reasons.push("featured snippet present");
    }

    // Authority competitors in top 5. When AIO is present, always count
    // (authority compounds zero-click risk). When AIO is absent, only count
    // if KD says the SERP is strong (KD >= 30).
    let kd_strong = kd.map(|v| v >= 30.0).unwrap_or(true);
    let count_authority = serp.ai_overview_present || kd_strong;
    let authority_competitors: Vec<String> = serp
        .organic_results
        .iter()
        .filter(|r| r.position <= 5)
        .filter(|r| is_authority_domain(&r.domain))
        .map(|r| r.domain.clone())
        .collect();
    if count_authority {
        let auth_count = authority_competitors.len().min(2) as u32;
        risk_score += auth_count;
        if auth_count > 0 {
            reasons.push(if auth_count == 1 {
                "1 authority domain in top 5"
            } else {
                "2+ authority domains in top 5"
            });
        }
    }

    // Note: no direct KD risk contribution. Final selection pre-filters
    // candidates to KD <= 30, so a high-KD branch could never fire; KD only
    // gates the authority-domain signal when AIO is absent.

    let bucket = match risk_score {
        0..=1 => WinnabilityBucket::Target,
        2..=3 => WinnabilityBucket::Differentiate,
        _ => WinnabilityBucket::Avoid,
    };

    let reason = if reasons.is_empty() {
        "Low risk: no AIO, no dominant authority competitors, winnable KD."
            .to_string()
    } else {
        reasons.join("; ") + "."
    };

    WinnabilityAssessment {
        keyword: keyword.to_string(),
        bucket,
        ai_overview_present: serp.ai_overview_present,
        featured_snippet_present: serp.featured_snippet_present,
        authority_competitors,
        risk_score,
        reason,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seo::keywords::{SerpFeaturesResult, SerpOrganicResult};

    fn make_serp(
        aio: bool,
        snippet: bool,
        paa: bool,
        domains: &[&str],
    ) -> SerpFeaturesResult {
        SerpFeaturesResult {
            keyword: "test".to_string(),
            ai_overview_present: aio,
            featured_snippet_present: snippet,
            people_also_ask_present: paa,
            organic_results: domains
                .iter()
                .enumerate()
                .map(|(i, d)| SerpOrganicResult {
                    domain: d.to_string(),
                    url: format!("https://{}/page", d),
                    title: format!("Result {}", i),
                    position: (i + 1) as i64,
                })
                .collect(),
        }
    }

    #[test]
    fn target_when_no_risk_factors() {
        let serp = make_serp(false, false, false, &["smallblog.com", "nicissite.com"]);
        let result = assess("best stocks wheel strategy", &serp, Some(25.0), Some("commercial"));
        assert_eq!(result.bucket, WinnabilityBucket::Target);
        assert_eq!(result.risk_score, 0);
    }

    #[test]
    fn differentiate_when_moderate_risk() {
        let serp = make_serp(true, false, false, &["investopedia.com", "smallblog.com"]);
        let result = assess("what is a covered call", &serp, Some(30.0), Some("informational"));
        // Short-answer AIO (+2) + 1 authority (KD >= 30, so counted) (+1) = 3 → Differentiate
        assert_eq!(result.risk_score, 3);
        assert_eq!(result.bucket, WinnabilityBucket::Differentiate);
    }

    #[test]
    fn differentiate_with_single_authority_and_no_aio() {
        let serp = make_serp(false, false, false, &["investopedia.com", "smallblog.com"]);
        let result = assess("iron condor strategy", &serp, Some(30.0), Some("informational"));
        // 1 authority (+1) = 1 → Target
        assert_eq!(result.bucket, WinnabilityBucket::Target);
    }

    #[test]
    fn low_kd_without_aio_ignores_authority_domains() {
        // Without AIO, KD 18 says the SERP has open slots despite Investopedia
        // ranking — authority is already priced into KD and must not be
        // double-counted.
        let serp = make_serp(
            false,
            false,
            false,
            &["investopedia.com", "tastylive.com", "smallblog.com"],
        );
        let result = assess("wash sale disallowed", &serp, Some(18.0), Some("informational"));
        // No AIO + authority ignored (KD < 30) = 0 → Target
        assert_eq!(result.risk_score, 0);
        assert_eq!(result.bucket, WinnabilityBucket::Target);
    }

    #[test]
    fn low_kd_with_aio_counts_authority_informational_is_avoid() {
        // With AIO present, authority domains count even when KD < 30.
        // Informational AIO (+2) + 2 authority (+2) = 4 → Avoid
        let serp = make_serp(
            true,
            false,
            false,
            &["investopedia.com", "tastylive.com", "smallblog.com"],
        );
        let result = assess("wash sale disallowed", &serp, Some(18.0), Some("informational"));
        assert!(result.risk_score >= 4);
        assert_eq!(result.bucket, WinnabilityBucket::Avoid);
        assert_eq!(result.risk_score, 4);
    }

    #[test]
    fn informational_aio_alone_is_differentiate() {
        // Pure informational + AIO, no authority, no snippet → risk 2 Differentiate
        // (not Target — AIO-heavy educational queries need a proprietary angle).
        let serp = make_serp(true, false, false, &["smallblog.com", "nicissite.com"]);
        let result = assess(
            "theta decay explained",
            &serp,
            Some(20.0),
            Some("informational"),
        );
        assert_eq!(result.risk_score, 2);
        assert_eq!(result.bucket, WinnabilityBucket::Differentiate);
        assert!(result.reason.contains("informational query"));
    }

    #[test]
    fn pure_informational_aio_plus_two_authority_is_avoid() {
        // Golden: pure informational + AIO + 2 authority → Avoid, including KD < 30
        let serp = make_serp(
            true,
            false,
            false,
            &["investopedia.com", "wikipedia.org", "smallblog.com"],
        );
        let result = assess(
            "covered call tax treatment",
            &serp,
            Some(22.0),
            Some("informational"),
        );
        // Informational AIO (+2) + 2 authority (+2) = 4 → Avoid
        assert_eq!(result.risk_score, 4);
        assert_eq!(result.bucket, WinnabilityBucket::Avoid);
    }

    #[test]
    fn short_answer_query_detection() {
        assert!(is_short_answer_query("what is iv crush"));
        assert!(is_short_answer_query("iv crush meaning"));
        assert!(is_short_answer_query("strike price vs exercise price"));
        assert!(is_short_answer_query("theta decay definition"));
        assert!(!is_short_answer_query("wash sale disallowed"));
        assert!(!is_short_answer_query("covered call strategy"));
        assert!(!is_short_answer_query("how to roll a covered call"));
    }

    #[test]
    fn avoid_when_aio_plus_two_authority_plus_informational() {
        let serp = make_serp(
            true,
            true,
            false,
            &["investopedia.com", "tastylive.com", "smallblog.com"],
        );
        let result = assess(
            "what is theta decay",
            &serp,
            Some(45.0),
            Some("informational"),
        );
        // Short-answer AIO (+2, not doubled with informational) + snippet (+1)
        // + 2 authority (KD >= 30) (+2) = 5 → Avoid
        assert_eq!(result.bucket, WinnabilityBucket::Avoid);
        assert!(result.risk_score >= 5);
        assert_eq!(result.risk_score, 5);
        assert!(result.reason.contains("short-answer query"));
        assert!(!result.reason.contains("informational query"));
    }

    #[test]
    fn authority_domain_detection_handles_subdomains() {
        assert!(is_authority_domain("investopedia.com"));
        assert!(is_authority_domain("www.investopedia.com"));
        assert!(is_authority_domain("sub.investopedia.com"));
        assert!(!is_authority_domain("notinvestopedia.com"));
        assert!(!is_authority_domain("investopedia.com.evil.com"));
    }

    #[test]
    fn non_short_answer_aio_alone_is_target() {
        // Commercial intent + AIO on a tool query with a weak SERP: users still
        // click for depth, and the AIO citation itself is winnable.
        let serp = make_serp(true, false, false, &["smallblog.com", "nicissite.com"]);
        let result = assess(
            "best covered call screener",
            &serp,
            Some(25.0),
            Some("commercial"),
        );
        // Commercial non-short-answer AIO (+1) = 1 → Target
        assert_eq!(result.risk_score, 1);
        assert_eq!(result.bucket, WinnabilityBucket::Target);
    }

    #[test]
    fn empty_intent_with_aio_is_not_informational() {
        // Empty/None intent must not be treated as informational — AIO is +1 only.
        let serp = make_serp(true, false, false, &["smallblog.com"]);
        let result = assess("options assignment process", &serp, Some(20.0), None);
        assert_eq!(result.risk_score, 1);
        assert_eq!(result.bucket, WinnabilityBucket::Target);
        assert!(!result.reason.contains("informational query"));
    }
}
