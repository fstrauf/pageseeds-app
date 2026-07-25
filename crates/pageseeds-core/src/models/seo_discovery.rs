use serde::{Deserialize, Serialize};

/// A single unified SEO opportunity produced by the `RankOpportunities` step.
///
/// This struct crosses the IPC boundary only as raw JSON inside a task artifact,
/// so it does not need a TS export in Phase 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeoOpportunity {
    pub article_id: i64,
    pub url_slug: String,
    pub title: String,
    pub file: String,
    pub target_keyword: String,
    pub opportunity_score: i64,
    pub effort: String,
    pub recommended_action: String,
    pub primary_signal: String,
    pub signals_json: serde_json::Value,
}

/// Container for the ranked opportunity list written to `seo_opportunities.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeoOpportunitiesDoc {
    pub generated_at: String,
    pub total_opportunities: usize,
    pub opportunities: Vec<SeoOpportunity>,
}
