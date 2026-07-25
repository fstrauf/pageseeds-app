//! Prototype: Deterministic plateau analysis for GSC data.
//!
//! This module computes structured plateau metrics that the agentic investigation
//! currently leaves to the LLM to infer. By making these deterministic, we get
//! reproducible, comparable diagnostics across investigations.
//!
//! Intended integration: add `PlateauAnalysisTool` to the investigation tool catalog
//! so agents can call it with one tool invocation instead of 5–7 separate calls
//! plus inference.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Input: raw GSC page-level metrics (from gsc_performance tool output)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
pub struct GscPageMetric {
    pub page: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Output: structured plateau diagnosis
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlateauAnalysis {
    /// Overall diagnosis synthesized from metrics
    pub diagnosis: PlateauDiagnosis,
    /// CTR performance vs position benchmarks
    pub ctr_anomaly: CtrAnomalyReport,
    /// How concentrated or distributed impressions are
    pub impression_distribution: ImpressionDistribution,
    /// Pages that get impressions but near-zero clicks
    pub zombie_pages: Vec<ZombiePage>,
    /// Position histogram (where do impressions actually show up?)
    pub position_histogram: PositionHistogram,
    /// Cohort breakdown by performance tier
    pub page_cohorts: PageCohorts,
    /// Whether the site looks "stuck" (flat distribution, no clear winners)
    pub stuck_signals: StuckSignals,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlateauDiagnosis {
    /// One of: "ctr_crisis", "winner_take_all", "flat_distribution",
    /// "position_suppressed", "zombie_dominated", "healthy_growth"
    pub primary_pattern: String,
    /// Human-readable summary
    pub summary: String,
    /// Confidence 0.0–1.0 based on signal strength
    pub confidence: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CtrAnomalyReport {
    /// Actual site-wide CTR (clicks / impressions)
    pub actual_ctr: f64,
    /// Expected CTR based on position-weighted industry benchmarks
    pub expected_ctr: f64,
    /// Ratio: actual / expected. < 0.5 is severe, < 1.0 is underperforming
    pub ctr_ratio: f64,
    /// Pages where CTR is < 50% of expected for their position bucket
    pub underperforming_pages: usize,
    /// Per-bucket breakdown
    pub bucket_breakdown: Vec<CtrBucket>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CtrBucket {
    pub position_range: String, // "1–2", "3–4", "5–7", "8–10", "11–20", "21+"
    pub page_count: usize,
    pub total_impressions: f64,
    pub actual_ctr: f64,
    pub expected_ctr: f64,
    pub ctr_gap: f64, // expected - actual
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImpressionDistribution {
    /// Total pages with >0 impressions
    pub active_pages: usize,
    /// % of impressions from top 10% of pages (high = winner-take-all)
    pub top_10pct_share: f64,
    /// % of impressions from top 1 page
    pub top_page_share: f64,
    /// Gini coefficient (0 = perfectly even, 1 = all impressions on one page)
    pub gini_coefficient: f64,
    /// Pages with < 100 impressions in the period (long tail)
    pub long_tail_pages: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ZombiePage {
    pub page: String,
    pub impressions: f64,
    pub clicks: f64,
    pub ctr: f64,
    pub position: f64,
    /// Estimated lost clicks if CTR matched position benchmark
    pub estimated_lost_clicks: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PositionHistogram {
    pub p1_3_share: f64,   // % of impressions in positions 1–3
    pub p4_6_share: f64,   // % of impressions in positions 4–6
    pub p7_10_share: f64,  // % of impressions in positions 7–10
    pub p11_20_share: f64, // % of impressions in positions 11–20
    pub p21_plus_share: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PageCohorts {
    /// Pages driving >80% of clicks (the "dependables")
    pub star_pages: Vec<CohortPage>,
    /// Pages with good impressions but poor CTR (optimization targets)
    pub opportunity_pages: Vec<CohortPage>,
    /// Pages with impressions but 0 clicks
    pub zero_click_pages: Vec<CohortPage>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CohortPage {
    pub page: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StuckSignals {
    /// True if no single page drives >15% of impressions (no clear winners)
    pub no_clear_winners: bool,
    /// True if >30% of impressions are on pages with CTR < 0.2%
    pub ctr_bleed: bool,
    /// True if >50% of impressions are in positions 7+ (suppressed rankings)
    pub position_suppressed: bool,
    /// True if top 10% of pages drive < 50% of impressions (flat distribution)
    pub flat_distribution: bool,
    /// True if >10% of active pages are zombie pages
    pub zombie_heavy: bool,
    /// Composite stuck score: count of true signals / total signals
    pub stuck_score: f64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Benchmark CTR by position (industry averages for informational content)
// ═══════════════════════════════════════════════════════════════════════════════

fn expected_ctr_for_position(position: f64) -> f64 {
    match position {
        p if p <= 2.0 => 0.08,
        p if p <= 4.0 => 0.04,
        p if p <= 7.0 => 0.015,
        p if p <= 10.0 => 0.008,
        p if p <= 20.0 => 0.003,
        _ => 0.001,
    }
}

fn position_bucket(position: f64) -> &'static str {
    match position {
        p if p <= 2.0 => "1–2",
        p if p <= 4.0 => "3–4",
        p if p <= 7.0 => "5–7",
        p if p <= 10.0 => "8–10",
        p if p <= 20.0 => "11–20",
        _ => "21+",
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Core analysis function
// ═══════════════════════════════════════════════════════════════════════════════

pub fn analyze_plateau(pages: &[GscPageMetric]) -> PlateauAnalysis {
    let active_pages: Vec<&GscPageMetric> = pages.iter().filter(|p| p.impressions > 0.0).collect();
    let total_impressions: f64 = active_pages.iter().map(|p| p.impressions).sum();
    let total_clicks: f64 = active_pages.iter().map(|p| p.clicks).sum();
    let actual_ctr = if total_impressions > 0.0 { total_clicks / total_impressions } else { 0.0 };

    // ── CTR anomaly ───────────────────────────────────────────────────────────
    let mut bucket_map: std::collections::HashMap<&'static str, Vec<&GscPageMetric>> =
        std::collections::HashMap::new();
    for p in &active_pages {
        bucket_map.entry(position_bucket(p.position)).or_default().push(p);
    }

    let mut bucket_breakdown: Vec<CtrBucket> = Vec::new();
    let mut expected_ctr_weighted_sum = 0.0;
    let mut underperforming_pages = 0usize;

    for (range, pages_in_bucket) in bucket_map {
        let bucket_impressions: f64 = pages_in_bucket.iter().map(|p| p.impressions).sum();
        let bucket_clicks: f64 = pages_in_bucket.iter().map(|p| p.clicks).sum();
        let bucket_actual_ctr = if bucket_impressions > 0.0 { bucket_clicks / bucket_impressions } else { 0.0 };
        // Use median position in bucket as benchmark anchor
        let median_pos = if let Some(mid) = pages_in_bucket.get(pages_in_bucket.len() / 2) {
            mid.position
        } else {
            10.0
        };
        let bucket_expected = expected_ctr_for_position(median_pos);
        expected_ctr_weighted_sum += bucket_expected * bucket_impressions;

        let local_under = pages_in_bucket
            .iter()
            .filter(|p| {
                let exp = expected_ctr_for_position(p.position);
                p.ctr < exp * 0.5 && p.impressions > 100.0
            })
            .count();
        underperforming_pages += local_under;

        bucket_breakdown.push(CtrBucket {
            position_range: range.to_string(),
            page_count: pages_in_bucket.len(),
            total_impressions: bucket_impressions,
            actual_ctr: bucket_actual_ctr,
            expected_ctr: bucket_expected,
            ctr_gap: bucket_expected - bucket_actual_ctr,
        });
    }

    bucket_breakdown.sort_by(|a, b| a.position_range.cmp(&b.position_range));

    let expected_ctr = if total_impressions > 0.0 {
        expected_ctr_weighted_sum / total_impressions
    } else {
        0.0
    };
    let ctr_ratio = if expected_ctr > 0.0 { actual_ctr / expected_ctr } else { 0.0 };

    // ── Impression distribution / Gini ────────────────────────────────────────
    let mut sorted_by_impressions: Vec<f64> = active_pages.iter().map(|p| p.impressions).collect();
    sorted_by_impressions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted_by_impressions.len() as f64;

    let gini = if n > 0.0 && total_impressions > 0.0 {
        let sum_weighted: f64 = sorted_by_impressions
            .iter()
            .enumerate()
            .map(|(i, v)| (2.0 * (i as f64 + 1.0) - n - 1.0) * v)
            .sum();
        sum_weighted / (n * total_impressions)
    } else {
        0.0
    };

    let top_10_count = ((n * 0.1).ceil() as usize).max(1);
    let top_10_impressions: f64 = sorted_by_impressions.iter().rev().take(top_10_count).sum();
    let top_10_share = if total_impressions > 0.0 { top_10_impressions / total_impressions } else { 0.0 };

    let top_page_impressions = sorted_by_impressions.last().copied().unwrap_or(0.0);
    let top_page_share = if total_impressions > 0.0 { top_page_impressions / total_impressions } else { 0.0 };

    let long_tail_pages = active_pages.iter().filter(|p| p.impressions < 100.0).count();

    // ── Zombie pages (impressions but no clicks, or CTR < 0.05%) ──────────────
    let mut zombies: Vec<ZombiePage> = active_pages
        .iter()
        .filter(|p| p.impressions > 500.0 && p.ctr < 0.0005) // 500+ impressions, < 0.05% CTR
        .map(|p| {
            let expected = expected_ctr_for_position(p.position);
            let lost = (p.impressions * expected) - p.clicks;
            ZombiePage {
                page: p.page.clone(),
                impressions: p.impressions,
                clicks: p.clicks,
                ctr: p.ctr,
                position: p.position,
                estimated_lost_clicks: lost.max(0.0),
            }
        })
        .collect();
    zombies.sort_by(|a, b| b.impressions.partial_cmp(&a.impressions).unwrap_or(std::cmp::Ordering::Equal));
    zombies.truncate(20);

    // ── Position histogram ────────────────────────────────────────────────────
    let p1_3: f64 = active_pages.iter().filter(|p| p.position <= 3.0).map(|p| p.impressions).sum();
    let p4_6: f64 = active_pages.iter().filter(|p| p.position > 3.0 && p.position <= 6.0).map(|p| p.impressions).sum();
    let p7_10: f64 = active_pages.iter().filter(|p| p.position > 6.0 && p.position <= 10.0).map(|p| p.impressions).sum();
    let p11_20: f64 = active_pages.iter().filter(|p| p.position > 10.0 && p.position <= 20.0).map(|p| p.impressions).sum();
    let p21_plus: f64 = active_pages.iter().filter(|p| p.position > 20.0).map(|p| p.impressions).sum();

    let pos_hist = PositionHistogram {
        p1_3_share: if total_impressions > 0.0 { p1_3 / total_impressions } else { 0.0 },
        p4_6_share: if total_impressions > 0.0 { p4_6 / total_impressions } else { 0.0 },
        p7_10_share: if total_impressions > 0.0 { p7_10 / total_impressions } else { 0.0 },
        p11_20_share: if total_impressions > 0.0 { p11_20 / total_impressions } else { 0.0 },
        p21_plus_share: if total_impressions > 0.0 { p21_plus / total_impressions } else { 0.0 },
    };

    // ── Page cohorts ──────────────────────────────────────────────────────────
    let mut by_clicks: Vec<&GscPageMetric> = active_pages.clone();
    by_clicks.sort_by(|a, b| b.clicks.partial_cmp(&a.clicks).unwrap_or(std::cmp::Ordering::Equal));

    let click_80_threshold = total_clicks * 0.8;
    let mut cumulative_clicks = 0.0;
    let mut star_pages: Vec<CohortPage> = Vec::new();
    for p in &by_clicks {
        if cumulative_clicks >= click_80_threshold {
            break;
        }
        star_pages.push(CohortPage {
            page: p.page.clone(),
            clicks: p.clicks,
            impressions: p.impressions,
            ctr: p.ctr,
            position: p.position,
        });
        cumulative_clicks += p.clicks;
    }

    let mut by_opp: Vec<&GscPageMetric> = active_pages
        .iter()
        .filter(|p| p.impressions > 1000.0 && p.ctr < expected_ctr_for_position(p.position) * 0.5)
        .copied()
        .collect();
    by_opp.sort_by(|a, b| b.impressions.partial_cmp(&a.impressions).unwrap_or(std::cmp::Ordering::Equal));

    let opportunity_pages: Vec<CohortPage> = by_opp.iter().take(10).map(|p| CohortPage {
        page: p.page.clone(),
        clicks: p.clicks,
        impressions: p.impressions,
        ctr: p.ctr,
        position: p.position,
    }).collect();

    let zero_click: Vec<CohortPage> = active_pages
        .iter()
        .filter(|p| p.clicks == 0.0 && p.impressions > 100.0)
        .take(10)
        .map(|p| CohortPage {
            page: p.page.clone(), clicks: p.clicks, impressions: p.impressions,
            ctr: p.ctr, position: p.position,
        })
        .collect();

    // ── Stuck signals ─────────────────────────────────────────────────────────
    let no_clear_winners = top_page_share < 0.15;
    let ctr_bleed = {
        let low_ctr_impressions: f64 = active_pages
            .iter()
            .filter(|p| p.ctr < 0.002)
            .map(|p| p.impressions)
            .sum();
        total_impressions > 0.0 && (low_ctr_impressions / total_impressions) > 0.30
    };
    let position_suppressed = {
        let suppressed_impressions: f64 = active_pages
            .iter()
            .filter(|p| p.position > 7.0)
            .map(|p| p.impressions)
            .sum();
        total_impressions > 0.0 && (suppressed_impressions / total_impressions) > 0.50
    };
    let flat_distribution = top_10_share < 0.50;
    let zombie_heavy = !zombies.is_empty() && (zombies.len() as f64 / active_pages.len() as f64) > 0.10;

    let stuck_score = {
        let signals = [no_clear_winners, ctr_bleed, position_suppressed, flat_distribution, zombie_heavy];
        signals.iter().filter(|&&s| s).count() as f64 / signals.len() as f64
    };

    let stuck_signals = StuckSignals {
        no_clear_winners,
        ctr_bleed,
        position_suppressed,
        flat_distribution,
        zombie_heavy,
        stuck_score,
    };

    // ── Diagnosis ─────────────────────────────────────────────────────────────
    let (primary_pattern, summary, confidence) = diagnose(
        ctr_ratio,
        &stuck_signals,
        &pos_hist,
        actual_ctr,
        total_impressions,
        active_pages.len(),
    );

    PlateauAnalysis {
        diagnosis: PlateauDiagnosis { primary_pattern, summary, confidence },
        ctr_anomaly: CtrAnomalyReport {
            actual_ctr,
            expected_ctr,
            ctr_ratio,
            underperforming_pages,
            bucket_breakdown,
        },
        impression_distribution: ImpressionDistribution {
            active_pages: active_pages.len(),
            top_10pct_share: top_10_share,
            top_page_share: top_page_share,
            gini_coefficient: gini,
            long_tail_pages,
        },
        zombie_pages: zombies,
        position_histogram: pos_hist,
        page_cohorts: PageCohorts {
            star_pages,
            opportunity_pages,
            zero_click_pages: zero_click,
        },
        stuck_signals,
    }
}

fn diagnose(
    ctr_ratio: f64,
    stuck: &StuckSignals,
    pos_hist: &PositionHistogram,
    actual_ctr: f64,
    total_impressions: f64,
    active_pages: usize,
) -> (String, String, f64) {
    // Priority order matters: CTR crisis is usually the most actionable
    if ctr_ratio < 0.3 && stuck.ctr_bleed {
        return (
            "ctr_crisis".to_string(),
            format!(
                "Site CTR is {:.2}% vs {:.2}% expected ({:.0}% of benchmark). \
                {:.1}% of impressions are on pages with near-zero CTR. \
                This is not a content volume problem — it's a click-through problem. \
                Fix titles, meta descriptions, and SERP rendering before chasing more impressions.",
                actual_ctr * 100.0,
                (actual_ctr / ctr_ratio.max(0.001)) * 100.0,
                ctr_ratio * 100.0,
                if total_impressions > 0.0 {
                    stuck.ctr_bleed as u8 as f64 * 30.0 // rough proxy
                } else { 0.0 }
            ),
            0.95,
        );
    }

    if stuck.position_suppressed && pos_hist.p7_10_share + pos_hist.p11_20_share > 0.6 {
        return (
            "position_suppressed".to_string(),
            "Most impressions come from positions 7–20. The site ranks but doesn't break into \
            the top results. This usually indicates weak topical authority, poor internal linking, \
            or content that doesn't fully satisfy intent.".to_string(),
            0.85,
        );
    }

    if stuck.zombie_heavy {
        return (
            "zombie_dominated".to_string(),
            "A large share of active pages get impressions but earn almost no clicks. \
            These 'zombie pages' drag down average CTR and may signal poor title/description quality \
            or SERP feature displacement (featured snippets, PAA).".to_string(),
            0.80,
        );
    }

    if stuck.flat_distribution && stuck.no_clear_winners {
        return (
            "flat_distribution".to_string(),
            format!(
                "Impressions are spread thinly across {} pages with no clear winners. \
                This suggests shallow topical clusters or lack of concentrated authority \
                on high-value queries.",
                active_pages
            ),
            0.75,
        );
    }

    if stuck.stuck_score >= 0.4 {
        return (
            "mixed_stuck".to_string(),
            "Multiple stuck signals are present. The site isn't growing because several \
            factors compound: CTR underperformance, flat distribution, and/or position suppression. \
            A multi-pronged fix (template + content depth + linking) is needed.".to_string(),
            0.70,
        );
    }

    (
        "healthy_growth".to_string(),
        "No strong stuck signals detected. Site may be growing normally or the plateau \
        is due to external factors (seasonality, competition).".to_string(),
        0.50,
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool wrapper for Rig integration (future)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlateauAnalysisArgs {
    /// JSON array of GscPageMetric objects (usually from gsc_performance tool)
    pub pages_json: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlateauAnalysisOutput {
    pub analysis: PlateauAnalysis,
}

/// Standalone function that can be wired into the investigation toolset.
pub fn run_plateau_analysis_from_json(pages_json: &str) -> Result<PlateauAnalysisOutput, String> {
    let pages: Vec<GscPageMetric> = serde_json::from_str(pages_json)
        .map_err(|e| format!("Failed to parse page metrics: {e}"))?;
    if pages.is_empty() {
        return Err("No pages provided for analysis".to_string());
    }
    Ok(PlateauAnalysisOutput { analysis: analyze_plateau(&pages) })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn page(clicks: f64, impressions: f64, position: f64) -> GscPageMetric {
        let ctr = if impressions > 0.0 { clicks / impressions } else { 0.0 };
        GscPageMetric {
            page: format!("/test-{}-{}", clicks as i64, impressions as i64),
            clicks,
            impressions,
            ctr,
            position,
        }
    }

    #[test]
    fn test_ctr_crisis_detection() {
        // Simulate daystoexpiry.com-like data: lots of impressions, terrible CTR
        let pages = vec![
            page(6.0, 5089.0, 6.8),    // typical page from screenshot
            page(4.0, 3200.0, 7.2),
            page(2.0, 4100.0, 6.5),
            page(8.0, 2800.0, 5.9),
            page(1.0, 1500.0, 8.1),
            page(0.0, 900.0, 9.3),
            page(3.0, 2200.0, 7.0),
            page(5.0, 1800.0, 6.2),
            page(2.0, 1200.0, 7.8),
            page(0.0, 800.0, 10.5),
        ];

        let result = analyze_plateau(&pages);

        assert_eq!(result.diagnosis.primary_pattern, "ctr_crisis");
        assert!(result.ctr_anomaly.ctr_ratio < 0.3);
        assert!(result.stuck_signals.ctr_bleed);
        assert!(!result.zombie_pages.is_empty());
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    }

    #[test]
    fn test_winner_take_all() {
        let pages = vec![
            page(500.0, 5000.0, 2.5),
            page(50.0, 800.0, 5.0),
            page(30.0, 600.0, 6.0),
            page(20.0, 400.0, 7.0),
            page(10.0, 200.0, 8.0),
        ];
        let result = analyze_plateau(&pages);
        assert!(result.impression_distribution.top_page_share > 0.40);
        assert!(!result.stuck_signals.no_clear_winners);
    }

    #[test]
    fn test_flat_distribution() {
        let pages: Vec<GscPageMetric> = (0..50)
            .map(|i| page(5.0 + i as f64, 200.0 + i as f64 * 10.0, 6.0 + (i % 5) as f64))
            .collect();
        let result = analyze_plateau(&pages);
        assert!(result.stuck_signals.flat_distribution);
        assert!(result.stuck_signals.no_clear_winners);
    }
}
