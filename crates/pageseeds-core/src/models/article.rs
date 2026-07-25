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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reviewed_at: Option<String>,
    #[serde(default)]
    pub review_count: i64,
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
    /// Page type: "hub", "pillar", "spoke", "landing", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    /// Content hash (SHA-256 of body) for change detection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// When the article content was last modified (file mtime or fix apply)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_edited_at: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct RepairPathResult {
    pub checked: usize,
    pub repaired: usize,
    pub removed: usize,
    pub not_found: Vec<String>,
}
