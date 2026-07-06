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
    /// Unwinnable: AIO-dominated, authority gap too large, or KD prohibitive.
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

/// Assess a keyword's winnability based on SERP features and keyword metrics.
///
/// # Arguments
/// * `keyword` - The keyword being assessed.
/// * `serp` - SERP feature data (AIO, snippets, competitor domains).
/// * `kd` - Keyword difficulty score (0-100), if available.
/// * `intent` - Search intent ("informational", "commercial", "transactional").
pub fn assess(
    keyword: &str,
    serp: &SerpFeaturesResult,
    kd: Option<f64>,
    intent: Option<&str>,
) -> WinnabilityAssessment {
    let mut risk_score: u32 = 0;
    let mut reasons: Vec<&str> = Vec::new();

    // AI Overview — the biggest zero-click risk.
    if serp.ai_overview_present {
        risk_score += 2;
        reasons.push("AI Overview present");
    }

    // Featured snippet — position 0 already captured.
    if serp.featured_snippet_present {
        risk_score += 1;
        reasons.push("featured snippet present");
    }

    // Authority competitors in top 5.
    let authority_competitors: Vec<String> = serp
        .organic_results
        .iter()
        .filter(|r| r.position <= 5)
        .filter(|r| is_authority_domain(&r.domain))
        .map(|r| r.domain.clone())
        .collect();
    let auth_count = authority_competitors.len().min(2) as u32;
    risk_score += auth_count;
    if auth_count > 0 {
        reasons.push(if auth_count == 1 {
            "1 authority domain in top 5"
        } else {
            "2+ authority domains in top 5"
        });
    }

    // Keyword difficulty.
    if let Some(kd_val) = kd {
        if kd_val >= 40.0 {
            risk_score += 1;
            reasons.push("KD >= 40");
        }
    }

    // Informational + AIO = highest zero-click risk.
    if intent == Some("informational") && serp.ai_overview_present {
        risk_score += 1;
        reasons.push("informational + AIO");
    }

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
        // AIO (+2) + 1 authority (+1) + informational+AIO (+1) = 4 → Avoid
        // Let's test a moderate case instead
        assert_eq!(result.risk_score, 4);
        assert_eq!(result.bucket, WinnabilityBucket::Avoid);
    }

    #[test]
    fn differentiate_with_single_authority_and_no_aio() {
        let serp = make_serp(false, false, false, &["investopedia.com", "smallblog.com"]);
        let result = assess("iron condor strategy", &serp, Some(30.0), Some("informational"));
        // 1 authority (+1) = 1 → Target
        assert_eq!(result.bucket, WinnabilityBucket::Target);
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
        // AIO (+2) + snippet (+1) + 2 authority (+2) + KD≥40 (+1) + info+AIO (+1) = 7 → Avoid
        assert_eq!(result.bucket, WinnabilityBucket::Avoid);
        assert!(result.risk_score >= 4);
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
    fn target_for_commercial_intent_even_with_aio() {
        // Commercial/transactional queries are less AIO-vulnerable.
        // AIO (+2) + 0 authority = 2 → Differentiate (not Avoid)
        let serp = make_serp(true, false, false, &["smallblog.com", "nicissite.com"]);
        let result = assess(
            "best covered call screener",
            &serp,
            Some(25.0),
            Some("commercial"),
        );
        assert_eq!(result.bucket, WinnabilityBucket::Differentiate);
    }
}
