use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct RedditOpportunity {
    pub post_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subreddit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posted_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upvotes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relevance_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engagement_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_relevant: Option<String>,
    #[serde(default)]
    pub key_pain_points: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_fit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_stance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>,
    pub reply_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_upvotes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_replies: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posted_at: Option<String>,
    pub project_id: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Raw Reddit post returned from the search API (before agent scoring).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct SubmissionSummary {
    pub post_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subreddit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upvotes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_old: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selftext: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ValidationResult {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct RedditStats {
    pub total_opportunities: i64,
    pub by_status: HashMap<String, i64>,
    pub pending_by_severity: HashMap<String, i64>,
    pub average_score: f64,
    pub max_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct MigrationResult {
    pub migrated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}
