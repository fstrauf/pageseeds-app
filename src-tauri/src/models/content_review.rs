use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Structured output from the content review recommendation agent.
/// Uses rig's Extractor<T> for type-safe generation via tool calling.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ContentReviewRecommendations {
    pub generated_at: String,
    pub total_articles: usize,
    pub articles: Vec<ReviewArticleRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ReviewArticleRecommendation {
    pub article_id: i64,
    pub article_title: String,
    pub article_file: String,
    pub url_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_keyword: Option<String>,
    pub suggestions: Vec<ReviewSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ReviewSuggestion {
    pub category: String,
    pub current: String,
    pub proposed: String,
    pub reason: String,
}
