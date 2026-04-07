use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub url_slug: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_keyword: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_difficulty: Option<String>,
    #[serde(default)]
    pub target_volume: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    #[serde(default)]
    pub word_count: i64,
    pub status: String,
    #[serde(default)]
    pub content_gaps_addressed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_traffic_monthly: Option<String>,
    #[serde(default)]
    pub project_id: String,
    
    // NEW: Content Quality Rating fields
    /// Overall quality score 0-100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<u8>,
    /// Letter grade (A, B, C, D, F)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_grade: Option<String>,
    /// When quality was last rated (ISO 8601 string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_rated_at: Option<String>,
    /// Whether article is ready to publish
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publishing_ready: Option<bool>,
    /// Category scores breakdown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_breakdown: Option<QualityBreakdown>,
}

/// Category scores for quality breakdown
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QualityBreakdown {
    /// Content length and structure score
    pub content: u8,
    /// Keyword optimization score
    pub keywords: u8,
    /// Meta elements score
    pub meta_elements: u8,
    /// Content structure score
    pub structure: u8,
    /// Internal/external links score
    pub links: u8,
    /// Readability score
    pub readability: u8,
}
